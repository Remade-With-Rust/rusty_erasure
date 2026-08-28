//! x86-64 kernels: PSHUFB nibble-table GF(2^8) multiply, SSSE3 (16 B/step)
//! and AVX2 (32 B/step), with up-to-4-row fusion on encode (one walk of the
//! source data feeds four parity accumulators — ISA-L's cache-bandwidth play).
//!
//! Baseline x86-64 is SSE2, which lacks `pshufb`, so even the SSSE3 set is
//! behind runtime detection; the scalar core is the fallback everywhere.

use core::sync::atomic::AtomicU64;

use rusty_erasure_core::kernel::Kernels;

#[cfg(target_arch = "x86_64")]
use core::sync::atomic::Ordering;
#[cfg(target_arch = "x86_64")]
use rusty_erasure_core::tables::{TABLE_BYTES, table_mul};

/// Census counter for the accel kernel sets: source bytes processed. One
/// relaxed add per call, never per element.
pub static ACCEL_CENSUS_BYTES: AtomicU64 = AtomicU64::new(0);

/// The ISA levels this module can provide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// 16 bytes per step, `pshufb` nibble tables.
    Ssse3,
    /// 32 bytes per step, `vpshufb` nibble tables on both lanes.
    Avx2,
    /// 32 bytes per step, one `GF2P8AFFINEQB` per multiply (affine tables).
    Gfni,
}

/// The best kernel set the running CPU supports, or `None` when even SSSE3 is
/// absent (or on a non-x86_64 target) — callers fall back to
/// `Kernels::scalar()`. Detection runs once and is cached.
pub fn kernels() -> Option<Kernels> {
    #[cfg(target_arch = "x86_64")]
    {
        static CHOICE: std::sync::OnceLock<Option<Kernels>> = std::sync::OnceLock::new();
        *CHOICE.get_or_init(|| {
            kernels_at(Level::Gfni)
                .or_else(|| kernels_at(Level::Avx2))
                .or_else(|| kernels_at(Level::Ssse3))
        })
    }
    #[cfg(not(target_arch = "x86_64"))]
    None
}

/// The best kernel set that consumes ISA-L NIBBLE-format tables (never GFNI —
/// its tables are affine-format). The compat layer's fast path requires this:
/// its callers hand it `ec_init_tables`-format tables by contract, and mixed
/// formats produce wrong parity at plausible speed (LEDGER M4).
pub fn kernels_nibble() -> Option<Kernels> {
    #[cfg(target_arch = "x86_64")]
    {
        static CHOICE: std::sync::OnceLock<Option<Kernels>> = std::sync::OnceLock::new();
        *CHOICE.get_or_init(|| kernels_at(Level::Avx2).or_else(|| kernels_at(Level::Ssse3)))
    }
    #[cfg(not(target_arch = "x86_64"))]
    None
}

/// The kernel set for one specific level, or `None` when the CPU lacks it.
/// Used by the oracle tests and the bench arms; production wants [`kernels`].
pub fn kernels_at(level: Level) -> Option<Kernels> {
    #[cfg(target_arch = "x86_64")]
    {
        use rusty_erasure_core::tables;
        match level {
            Level::Gfni
                if std::arch::is_x86_feature_detected!("gfni")
                    && std::arch::is_x86_feature_detected!("avx2") =>
            {
                Some(Kernels {
                    init: tables::init_tables_gfni,
                    table_bytes: tables::GFNI_TABLE_BYTES,
                    encode: imp::encode_gfni,
                    mad: imp::mad_gfni,
                    name: "x86_64/avx2_gfni",
                    census: &ACCEL_CENSUS_BYTES,
                })
            }
            Level::Avx2 if std::arch::is_x86_feature_detected!("avx2") => Some(Kernels {
                init: tables::init_tables,
                table_bytes: tables::TABLE_BYTES,
                encode: imp::encode_avx2,
                mad: imp::mad_avx2,
                name: "x86_64/avx2",
                census: &ACCEL_CENSUS_BYTES,
            }),
            Level::Ssse3 if std::arch::is_x86_feature_detected!("ssse3") => Some(Kernels {
                init: tables::init_tables,
                table_bytes: tables::TABLE_BYTES,
                encode: imp::encode_ssse3,
                mad: imp::mad_ssse3,
                name: "x86_64/ssse3",
                census: &ACCEL_CENSUS_BYTES,
            }),
            _ => None,
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = level;
        None
    }
}

/// Shared validation for every safe wrapper: the Coder validates upstream,
/// but a kernel must be un-OOB-able even on contract violation — these
/// asserts run once per call, not per element.
#[cfg(target_arch = "x86_64")]
fn check_encode(gftbls: &[u8], data: &[&[u8]], out: &[&mut [u8]], table_bytes: usize) -> usize {
    let k = data.len();
    let len = out.first().map_or(0, |b| b.len());
    assert!(
        gftbls.len() >= out.len() * k * table_bytes,
        "gftbls too short for {} rows x {} sources",
        out.len(),
        k
    );
    for s in data {
        assert_eq!(s.len(), len, "source length mismatch");
    }
    for o in out {
        assert_eq!(o.len(), len, "output length mismatch");
    }
    ACCEL_CENSUS_BYTES.fetch_add((k * len) as u64, Ordering::Relaxed);
    len
}

/// Scalar tail shared by both ISA widths: finish bytes `[i..len)` exactly as
/// the scalar oracle would.
#[cfg(target_arch = "x86_64")]
fn encode_tail(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]], i: usize) {
    let k = data.len();
    for (r, dest) in out.iter_mut().enumerate() {
        for (x, d) in dest[i..].iter_mut().enumerate() {
            let mut acc = 0u8;
            for (j, src) in data.iter().enumerate() {
                let start = (r * k + j) * TABLE_BYTES;
                let tbl: &[u8; TABLE_BYTES] =
                    gftbls[start..start + TABLE_BYTES].try_into().expect("checked");
                acc ^= table_mul(tbl, src[i + x]);
            }
            *d = acc;
        }
    }
}

#[cfg(target_arch = "x86_64")]
pub(crate) mod imp {
    use super::*;
    use core::arch::x86_64::*;
    use rusty_erasure_core::tables;

    // ---------------- AVX2 ----------------

    /// Encode a group of `R` rows fused: one walk of the sources feeds `R`
    /// accumulator registers. 32 bytes per step, scalar tail.
    ///
    /// # Safety
    /// Caller guarantees AVX2 is available, `out.len() == R`, every slice in
    /// `data`/`out` has the same length, and `gftbls` holds `R * k` tables.
    #[target_feature(enable = "avx2")]
    unsafe fn encode_group_avx2<const R: usize>(
        gftbls: &[u8],
        data: &[&[u8]],
        out: &mut [&mut [u8]],
    ) {
        let k = data.len();
        let len = out[0].len();
        let mask = _mm256_set1_epi8(0x0f);
        let mut i = 0usize;
        while i + 32 <= len {
            let mut acc = [_mm256_setzero_si256(); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 32 <= len == src.len() (asserted by the wrapper).
                let s = unsafe { _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i) };
                let lo = _mm256_and_si256(s, mask);
                let hi = _mm256_and_si256(_mm256_srli_epi64::<4>(s), mask);
                for r in 0..R {
                    // SAFETY: (r*k + j + 1) * 32 <= gftbls.len() (asserted).
                    let t = unsafe { gftbls.as_ptr().add((r * k + j) * TABLE_BYTES) };
                    let (tlo, thi) = unsafe {
                        (
                            _mm256_broadcastsi128_si256(_mm_loadu_si128(t as *const __m128i)),
                            _mm256_broadcastsi128_si256(_mm_loadu_si128(
                                t.add(16) as *const __m128i
                            )),
                        )
                    };
                    acc[r] = _mm256_xor_si256(
                        acc[r],
                        _mm256_xor_si256(
                            _mm256_shuffle_epi8(tlo, lo),
                            _mm256_shuffle_epi8(thi, hi),
                        ),
                    );
                }
            }
            for r in 0..R {
                // SAFETY: i + 32 <= len == out[r].len() (asserted).
                unsafe {
                    _mm256_storeu_si256(out[r].as_mut_ptr().add(i) as *mut __m256i, acc[r]);
                }
            }
            i += 32;
        }
        encode_tail(gftbls, data, out, i);
    }

    /// Safe wrapper: groups rows by 4 and dispatches the monomorphized fused
    /// kernels. SAFETY invariant for every unsafe call below: this fn pointer
    /// is only handed out by `kernels_at` after `avx2` detection succeeded,
    /// and `check_encode` has re-validated every slice length.
    pub(crate) fn encode_avx2(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
        let k = data.len();
        check_encode(gftbls, data, out, TABLE_BYTES);
        let mut row = 0usize;
        while row < out.len() {
            let g = (out.len() - row).min(4);
            let tb = &gftbls[row * k * TABLE_BYTES..(row + g) * k * TABLE_BYTES];
            let group = &mut out[row..row + g];
            // SAFETY: see the wrapper doc above.
            unsafe {
                match g {
                    4 => encode_group_avx2::<4>(tb, data, group),
                    3 => encode_group_avx2::<3>(tb, data, group),
                    2 => encode_group_avx2::<2>(tb, data, group),
                    _ => encode_group_avx2::<1>(tb, data, group),
                }
            }
            row += g;
        }
    }

    /// `dest ^= c · src`, 32 bytes per step.
    ///
    /// # Safety
    /// Caller guarantees AVX2 and equal slice lengths.
    #[target_feature(enable = "avx2")]
    unsafe fn mad_avx2_inner(tbl: &[u8; TABLE_BYTES], src: &[u8], dest: &mut [u8]) {
        let n = dest.len();
        // SAFETY: the table is exactly 32 bytes by type.
        let (tlo, thi) = unsafe {
            (
                _mm256_broadcastsi128_si256(_mm_loadu_si128(tbl.as_ptr() as *const __m128i)),
                _mm256_broadcastsi128_si256(_mm_loadu_si128(
                    tbl.as_ptr().add(16) as *const __m128i
                )),
            )
        };
        let mask = _mm256_set1_epi8(0x0f);
        let mut i = 0usize;
        while i + 32 <= n {
            // SAFETY: i + 32 <= n == src.len() == dest.len() (asserted).
            unsafe {
                let s = _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i);
                let d = _mm256_loadu_si256(dest.as_ptr().add(i) as *const __m256i);
                let lo = _mm256_and_si256(s, mask);
                let hi = _mm256_and_si256(_mm256_srli_epi64::<4>(s), mask);
                let p = _mm256_xor_si256(
                    _mm256_shuffle_epi8(tlo, lo),
                    _mm256_shuffle_epi8(thi, hi),
                );
                _mm256_storeu_si256(
                    dest.as_mut_ptr().add(i) as *mut __m256i,
                    _mm256_xor_si256(d, p),
                );
            }
            i += 32;
        }
        for (d, &s) in dest[i..].iter_mut().zip(&src[i..]) {
            *d ^= table_mul(tbl, s);
        }
    }

    pub(crate) fn mad_avx2(tbl: &[u8], src: &[u8], dest: &mut [u8]) {
        let tbl: &[u8; TABLE_BYTES] = tbl.try_into().expect("nibble mad takes a 32-byte table");
        assert_eq!(src.len(), dest.len(), "mad length mismatch");
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
        // SAFETY: handed out only after avx2 detection; lengths asserted.
        unsafe { mad_avx2_inner(tbl, src, dest) }
    }

    // ---------------- SSSE3 ----------------

    /// # Safety
    /// Caller guarantees SSSE3, `out.len() == R`, equal slice lengths, and
    /// `R * k` tables in `gftbls`.
    #[target_feature(enable = "ssse3")]
    unsafe fn encode_group_ssse3<const R: usize>(
        gftbls: &[u8],
        data: &[&[u8]],
        out: &mut [&mut [u8]],
    ) {
        let k = data.len();
        let len = out[0].len();
        let mask = _mm_set1_epi8(0x0f);
        let mut i = 0usize;
        while i + 16 <= len {
            let mut acc = [_mm_setzero_si128(); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 16 <= len == src.len() (asserted by the wrapper).
                let s = unsafe { _mm_loadu_si128(src.as_ptr().add(i) as *const __m128i) };
                let lo = _mm_and_si128(s, mask);
                let hi = _mm_and_si128(_mm_srli_epi64::<4>(s), mask);
                for r in 0..R {
                    // SAFETY: (r*k + j + 1) * 32 <= gftbls.len() (asserted).
                    let t = unsafe { gftbls.as_ptr().add((r * k + j) * TABLE_BYTES) };
                    let (tlo, thi) = unsafe {
                        (
                            _mm_loadu_si128(t as *const __m128i),
                            _mm_loadu_si128(t.add(16) as *const __m128i),
                        )
                    };
                    acc[r] = _mm_xor_si128(
                        acc[r],
                        _mm_xor_si128(_mm_shuffle_epi8(tlo, lo), _mm_shuffle_epi8(thi, hi)),
                    );
                }
            }
            for r in 0..R {
                // SAFETY: i + 16 <= len == out[r].len() (asserted).
                unsafe { _mm_storeu_si128(out[r].as_mut_ptr().add(i) as *mut __m128i, acc[r]) };
            }
            i += 16;
        }
        encode_tail(gftbls, data, out, i);
    }

    pub(crate) fn encode_ssse3(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
        let k = data.len();
        check_encode(gftbls, data, out, TABLE_BYTES);
        let mut row = 0usize;
        while row < out.len() {
            let g = (out.len() - row).min(4);
            let tb = &gftbls[row * k * TABLE_BYTES..(row + g) * k * TABLE_BYTES];
            let group = &mut out[row..row + g];
            // SAFETY: handed out only after ssse3 detection; lengths checked.
            unsafe {
                match g {
                    4 => encode_group_ssse3::<4>(tb, data, group),
                    3 => encode_group_ssse3::<3>(tb, data, group),
                    2 => encode_group_ssse3::<2>(tb, data, group),
                    _ => encode_group_ssse3::<1>(tb, data, group),
                }
            }
            row += g;
        }
    }

    /// # Safety
    /// Caller guarantees SSSE3 and equal slice lengths.
    #[target_feature(enable = "ssse3")]
    unsafe fn mad_ssse3_inner(tbl: &[u8; TABLE_BYTES], src: &[u8], dest: &mut [u8]) {
        let n = dest.len();
        // SAFETY: the table is exactly 32 bytes by type.
        let (tlo, thi) = unsafe {
            (
                _mm_loadu_si128(tbl.as_ptr() as *const __m128i),
                _mm_loadu_si128(tbl.as_ptr().add(16) as *const __m128i),
            )
        };
        let mask = _mm_set1_epi8(0x0f);
        let mut i = 0usize;
        while i + 16 <= n {
            // SAFETY: i + 16 <= n == src.len() == dest.len() (asserted).
            unsafe {
                let s = _mm_loadu_si128(src.as_ptr().add(i) as *const __m128i);
                let d = _mm_loadu_si128(dest.as_ptr().add(i) as *const __m128i);
                let lo = _mm_and_si128(s, mask);
                let hi = _mm_and_si128(_mm_srli_epi64::<4>(s), mask);
                let p = _mm_xor_si128(_mm_shuffle_epi8(tlo, lo), _mm_shuffle_epi8(thi, hi));
                _mm_storeu_si128(dest.as_mut_ptr().add(i) as *mut __m128i, _mm_xor_si128(d, p));
            }
            i += 16;
        }
        for (d, &s) in dest[i..].iter_mut().zip(&src[i..]) {
            *d ^= table_mul(tbl, s);
        }
    }

    pub(crate) fn mad_ssse3(tbl: &[u8], src: &[u8], dest: &mut [u8]) {
        let tbl: &[u8; TABLE_BYTES] = tbl.try_into().expect("nibble mad takes a 32-byte table");
        assert_eq!(src.len(), dest.len(), "mad length mismatch");
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
        // SAFETY: handed out only after ssse3 detection; lengths asserted.
        unsafe { mad_ssse3_inner(tbl, src, dest) }
    }

    // ---------------- GFNI (VEX-256) ----------------

    /// Recover the coefficient from its affine matrix: `c = c·1` is column 0,
    /// so bit `j` of `c` = bit 0 of row `j` = bit 0 of qword byte `7−j`.
    /// Used by the scalar tails, which then multiply via the proven `gf::mul`.
    fn coeff_from_affine(m: u64) -> u8 {
        let mut c = 0u8;
        for j in 0..8 {
            if (m >> (8 * (7 - j))) & 1 != 0 {
                c |= 1 << j;
            }
        }
        c
    }

    fn affine_at(gftbls: &[u8], index: usize) -> u64 {
        u64::from_le_bytes(
            gftbls[index * 8..index * 8 + 8].try_into().expect("checked by wrapper"),
        )
    }

    /// Scalar tail for the GFNI encode: exact via coefficient + `gf::mul`.
    fn encode_tail_gfni(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]], i: usize) {
        let k = data.len();
        for (r, dest) in out.iter_mut().enumerate() {
            for (x, d) in dest[i..].iter_mut().enumerate() {
                let mut acc = 0u8;
                for (j, src) in data.iter().enumerate() {
                    let c = coeff_from_affine(affine_at(gftbls, r * k + j));
                    acc ^= rusty_erasure_core::gf::mul(c, src[i + x]);
                }
                *d = acc;
            }
        }
    }

    /// Encode a group of `R` rows fused, one `GF2P8AFFINEQB` per (source,
    /// row) per 32-byte step.
    ///
    /// # Safety
    /// Caller guarantees GFNI+AVX2, `out.len() == R`, equal slice lengths,
    /// and `R * k` 8-byte tables in `gftbls`.
    #[target_feature(enable = "gfni,avx2")]
    unsafe fn encode_group_gfni<const R: usize>(
        gftbls: &[u8],
        data: &[&[u8]],
        out: &mut [&mut [u8]],
    ) {
        let k = data.len();
        let len = out[0].len();
        let mut i = 0usize;
        while i + 32 <= len {
            let mut acc = [_mm256_setzero_si256(); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 32 <= len == src.len() (asserted by the wrapper).
                let s = unsafe { _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i) };
                for r in 0..R {
                    let a = _mm256_set1_epi64x(affine_at(gftbls, r * k + j) as i64);
                    acc[r] = _mm256_xor_si256(acc[r], _mm256_gf2p8affine_epi64_epi8::<0>(s, a));
                }
            }
            for r in 0..R {
                // SAFETY: i + 32 <= len == out[r].len() (asserted).
                unsafe {
                    _mm256_storeu_si256(out[r].as_mut_ptr().add(i) as *mut __m256i, acc[r]);
                }
            }
            i += 32;
        }
        encode_tail_gfni(gftbls, data, out, i);
    }

    /// Safe wrapper. SAFETY invariant: this fn pointer is only handed out by
    /// `kernels_at` after gfni+avx2 detection, and `check_encode` re-validated
    /// every slice length.
    pub(crate) fn encode_gfni(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
        let k = data.len();
        check_encode(gftbls, data, out, tables::GFNI_TABLE_BYTES);
        let mut row = 0usize;
        while row < out.len() {
            let g = (out.len() - row).min(4);
            let tb = &gftbls[row * k * 8..(row + g) * k * 8];
            let group = &mut out[row..row + g];
            // SAFETY: see the wrapper doc above.
            unsafe {
                match g {
                    4 => encode_group_gfni::<4>(tb, data, group),
                    3 => encode_group_gfni::<3>(tb, data, group),
                    2 => encode_group_gfni::<2>(tb, data, group),
                    _ => encode_group_gfni::<1>(tb, data, group),
                }
            }
            row += g;
        }
    }

    /// # Safety
    /// Caller guarantees GFNI+AVX2 and equal slice lengths.
    #[target_feature(enable = "gfni,avx2")]
    unsafe fn mad_gfni_inner(matrix: u64, src: &[u8], dest: &mut [u8]) {
        let n = dest.len();
        let a = _mm256_set1_epi64x(matrix as i64);
        let mut i = 0usize;
        while i + 32 <= n {
            // SAFETY: i + 32 <= n == src.len() == dest.len() (asserted).
            unsafe {
                let s = _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i);
                let d = _mm256_loadu_si256(dest.as_ptr().add(i) as *const __m256i);
                _mm256_storeu_si256(
                    dest.as_mut_ptr().add(i) as *mut __m256i,
                    _mm256_xor_si256(d, _mm256_gf2p8affine_epi64_epi8::<0>(s, a)),
                );
            }
            i += 32;
        }
        let c = coeff_from_affine(matrix);
        for (d, &s) in dest[i..].iter_mut().zip(&src[i..]) {
            *d ^= rusty_erasure_core::gf::mul(c, s);
        }
    }

    pub(crate) fn mad_gfni(tbl: &[u8], src: &[u8], dest: &mut [u8]) {
        assert_eq!(tbl.len(), tables::GFNI_TABLE_BYTES, "gfni mad takes an 8-byte table");
        assert_eq!(src.len(), dest.len(), "mad length mismatch");
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
        let m = u64::from_le_bytes(tbl.try_into().expect("length asserted"));
        // SAFETY: handed out only after gfni+avx2 detection; lengths asserted.
        unsafe { mad_gfni_inner(m, src, dest) }
    }
}

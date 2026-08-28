//! x86-64 kernels: PSHUFB nibble-table GF(2^8) multiply, SSSE3 (16 B/step)
//! and AVX2 (32 B/step), with up-to-4-row fusion on encode (one walk of the
//! source data feeds four parity accumulators — ISA-L's cache-bandwidth play).
//!
//! Baseline x86-64 is SSE2, which lacks `pshufb`, so even the SSSE3 set is
//! behind runtime detection; the scalar core is the fallback everywhere.

use rusty_erasure_core::kernel::Kernels;

#[cfg(target_arch = "x86_64")]
use core::sync::atomic::Ordering;
#[cfg(target_arch = "x86_64")]
use rusty_erasure_core::tables::{TABLE_BYTES, table_mul};

pub use crate::ACCEL_CENSUS_BYTES;

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
                    update: imp::update_gfni,
                    name: "x86_64/avx2_gfni",
                    census: &ACCEL_CENSUS_BYTES,
                })
            }
            Level::Avx2 if std::arch::is_x86_feature_detected!("avx2") => Some(Kernels {
                init: tables::init_tables,
                table_bytes: tables::TABLE_BYTES,
                encode: imp::encode_avx2,
                mad: imp::mad_avx2,
                update: imp::update_avx2,
                name: "x86_64/avx2",
                census: &ACCEL_CENSUS_BYTES,
            }),
            Level::Ssse3 if std::arch::is_x86_feature_detected!("ssse3") => Some(Kernels {
                init: tables::init_tables,
                table_bytes: tables::TABLE_BYTES,
                encode: imp::encode_ssse3,
                mad: imp::mad_ssse3,
                update: imp::update_ssse3,
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

/// Scalar tail for the nibble kernels — the arch-shared implementation
/// (`tail.rs`), so every architecture finishes its tail with the same code.
#[cfg(target_arch = "x86_64")]
use crate::tail::encode_tail_nibble as encode_tail;

/// The AVX2 RAID kernels (xor / P+Q fold), or `None` off-x86 / without AVX2.
/// Byte-identical to `rusty_erasure_core::raid` (oracle-tested); the facade's
/// `raid` module dispatches through this.
pub fn raid_kernels() -> Option<crate::RaidKernels> {
    #[cfg(target_arch = "x86_64")]
    {
        static CHOICE: std::sync::OnceLock<Option<crate::RaidKernels>> =
            std::sync::OnceLock::new();
        *CHOICE.get_or_init(|| {
            if std::arch::is_x86_feature_detected!("avx2") {
                Some((imp::raid_xor_avx2, imp::raid_pq_avx2))
            } else {
                // SSE2 is the x86-64 baseline: always available, no detection.
                Some((imp::raid_xor_sse2, imp::raid_pq_sse2))
            }
        })
    }
    #[cfg(not(target_arch = "x86_64"))]
    None
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
        // Double-chunk main loop: each (row, source) table load serves BOTH
        // 32-byte chunks — table traffic per byte halved (the deploy of the
        // GFNI double-chunk brick to the nibble sets).
        while i + 64 <= len {
            let mut acc0 = [_mm256_setzero_si256(); R];
            let mut acc1 = [_mm256_setzero_si256(); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 64 <= len == src.len() (asserted by the wrapper).
                let (s0, s1) = unsafe {
                    (
                        _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i),
                        _mm256_loadu_si256(src.as_ptr().add(i + 32) as *const __m256i),
                    )
                };
                let lo0 = _mm256_and_si256(s0, mask);
                let hi0 = _mm256_and_si256(_mm256_srli_epi64::<4>(s0), mask);
                let lo1 = _mm256_and_si256(s1, mask);
                let hi1 = _mm256_and_si256(_mm256_srli_epi64::<4>(s1), mask);
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
                    acc0[r] = _mm256_xor_si256(
                        acc0[r],
                        _mm256_xor_si256(
                            _mm256_shuffle_epi8(tlo, lo0),
                            _mm256_shuffle_epi8(thi, hi0),
                        ),
                    );
                    acc1[r] = _mm256_xor_si256(
                        acc1[r],
                        _mm256_xor_si256(
                            _mm256_shuffle_epi8(tlo, lo1),
                            _mm256_shuffle_epi8(thi, hi1),
                        ),
                    );
                }
            }
            for r in 0..R {
                // SAFETY: i + 64 <= len == out[r].len() (asserted).
                unsafe {
                    _mm256_storeu_si256(out[r].as_mut_ptr().add(i) as *mut __m256i, acc0[r]);
                    _mm256_storeu_si256(
                        out[r].as_mut_ptr().add(i + 32) as *mut __m256i,
                        acc1[r],
                    );
                }
            }
            i += 64;
        }
        if i + 32 <= len {
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
        // Double-chunk main loop: each (row, source) table load serves BOTH
        // 16-byte chunks (the GFNI double-chunk brick; 15 xmm live, fits).
        while i + 32 <= len {
            let mut acc0 = [_mm_setzero_si128(); R];
            let mut acc1 = [_mm_setzero_si128(); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 32 <= len == src.len() (asserted by the wrapper).
                let (s0, s1) = unsafe {
                    (
                        _mm_loadu_si128(src.as_ptr().add(i) as *const __m128i),
                        _mm_loadu_si128(src.as_ptr().add(i + 16) as *const __m128i),
                    )
                };
                let lo0 = _mm_and_si128(s0, mask);
                let hi0 = _mm_and_si128(_mm_srli_epi64::<4>(s0), mask);
                let lo1 = _mm_and_si128(s1, mask);
                let hi1 = _mm_and_si128(_mm_srli_epi64::<4>(s1), mask);
                for r in 0..R {
                    // SAFETY: (r*k + j + 1) * 32 <= gftbls.len() (asserted).
                    let t = unsafe { gftbls.as_ptr().add((r * k + j) * TABLE_BYTES) };
                    let (tlo, thi) = unsafe {
                        (
                            _mm_loadu_si128(t as *const __m128i),
                            _mm_loadu_si128(t.add(16) as *const __m128i),
                        )
                    };
                    acc0[r] = _mm_xor_si128(
                        acc0[r],
                        _mm_xor_si128(_mm_shuffle_epi8(tlo, lo0), _mm_shuffle_epi8(thi, hi0)),
                    );
                    acc1[r] = _mm_xor_si128(
                        acc1[r],
                        _mm_xor_si128(_mm_shuffle_epi8(tlo, lo1), _mm_shuffle_epi8(thi, hi1)),
                    );
                }
            }
            for r in 0..R {
                // SAFETY: i + 32 <= len == out[r].len() (asserted).
                unsafe {
                    _mm_storeu_si128(out[r].as_mut_ptr().add(i) as *mut __m128i, acc0[r]);
                    _mm_storeu_si128(
                        out[r].as_mut_ptr().add(i + 16) as *mut __m128i,
                        acc1[r],
                    );
                }
            }
            i += 32;
        }
        if i + 16 <= len {
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

    /// Register-resident small-stripe variant: `K` and `R` monomorphized
    /// (`K*R <= 8`) so every matrix stays in a ymm register across the whole
    /// stripe and the source indexing is bounds-check-free. Motivated by the
    /// M5 ceiling probe: per-chunk matrix loads dominated the 4 KiB cell.
    ///
    /// # Safety
    /// Caller guarantees GFNI+AVX2, `data.len() == K`, `out.len() == R`,
    /// equal slice lengths, and `R * K` 8-byte tables in `gftbls`.
    #[target_feature(enable = "gfni,avx2")]
    unsafe fn encode_group_gfni_reg<const K: usize, const R: usize>(
        gftbls: &[u8],
        data: &[&[u8]],
        out: &mut [&mut [u8]],
    ) {
        let len = out[0].len();
        let srcs: &[&[u8]; K] = data.try_into().expect("caller guarantees data.len() == K");
        let mut mats = [_mm256_setzero_si256(); 8];
        for r in 0..R {
            for j in 0..K {
                mats[r * K + j] = _mm256_set1_epi64x(affine_at(gftbls, r * K + j) as i64);
            }
        }
        let mut i = 0usize;
        while i + 32 <= len {
            let mut acc = [_mm256_setzero_si256(); R];
            for (j, src) in srcs.iter().enumerate() {
                // SAFETY: i + 32 <= len == src.len() (asserted by the wrapper).
                let s = unsafe { _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i) };
                for r in 0..R {
                    acc[r] = _mm256_xor_si256(acc[r], _mm256_gf2p8affine_epi64_epi8::<0>(s, mats[r * K + j]));
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
    /// Double-chunk variant for the reload path (k > 4, R <= 4): 64 bytes per
    /// iteration, each per-(row,source) matrix broadcast serves BOTH chunks —
    /// broadcasts per byte halved (the wide-k mechanism the full benchmark
    /// named), plus doubled ILP on the affine port.
    ///
    /// # Safety
    /// Caller guarantees GFNI+AVX2, `out.len() == R <= 4`, equal slice
    /// lengths, `R * k` tables (re-asserted by the wrapper).
    #[target_feature(enable = "gfni,avx2")]
    unsafe fn encode_group_gfni2<const R: usize>(
        gftbls: &[u8],
        data: &[&[u8]],
        out: &mut [&mut [u8]],
    ) {
        let k = data.len();
        let len = out[0].len();
        let mut i = 0usize;
        while i + 64 <= len {
            let mut acc0 = [_mm256_setzero_si256(); R];
            let mut acc1 = [_mm256_setzero_si256(); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 64 <= len == src.len() (asserted by the wrapper).
                let (s0, s1) = unsafe {
                    (
                        _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i),
                        _mm256_loadu_si256(src.as_ptr().add(i + 32) as *const __m256i),
                    )
                };
                for r in 0..R {
                    let a = _mm256_set1_epi64x(affine_at(gftbls, r * k + j) as i64);
                    acc0[r] =
                        _mm256_xor_si256(acc0[r], _mm256_gf2p8affine_epi64_epi8::<0>(s0, a));
                    acc1[r] =
                        _mm256_xor_si256(acc1[r], _mm256_gf2p8affine_epi64_epi8::<0>(s1, a));
                }
            }
            for r in 0..R {
                // SAFETY: i + 64 <= len == out[r].len() (asserted).
                unsafe {
                    _mm256_storeu_si256(out[r].as_mut_ptr().add(i) as *mut __m256i, acc0[r]);
                    _mm256_storeu_si256(
                        out[r].as_mut_ptr().add(i + 32) as *mut __m256i,
                        acc1[r],
                    );
                }
            }
            i += 64;
        }
        if i + 32 <= len {
            let mut acc = [_mm256_setzero_si256(); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 32 <= len == src.len() (asserted).
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

    pub(crate) fn encode_gfni(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
        let k = data.len();
        check_encode(gftbls, data, out, tables::GFNI_TABLE_BYTES);
        // Small-k lane: registers hold all matrices (groups of 2 keep K*R <= 8).
        if (1..=4).contains(&k) {
            let mut row = 0usize;
            while row < out.len() {
                let g = (out.len() - row).min(2);
                let tb = &gftbls[row * k * 8..(row + g) * k * 8];
                let group = &mut out[row..row + g];
                // SAFETY: see the wrapper doc above; k and g bounds hold by construction.
                unsafe {
                    match (k, g) {
                        (1, 1) => encode_group_gfni_reg::<1, 1>(tb, data, group),
                        (1, _) => encode_group_gfni_reg::<1, 2>(tb, data, group),
                        (2, 1) => encode_group_gfni_reg::<2, 1>(tb, data, group),
                        (2, _) => encode_group_gfni_reg::<2, 2>(tb, data, group),
                        (3, 1) => encode_group_gfni_reg::<3, 1>(tb, data, group),
                        (3, _) => encode_group_gfni_reg::<3, 2>(tb, data, group),
                        (4, 1) => encode_group_gfni_reg::<4, 1>(tb, data, group),
                        _ => encode_group_gfni_reg::<4, 2>(tb, data, group),
                    }
                }
                row += g;
            }
            return;
        }
        // Wide-k path: R >= 5 takes single-chunk groups of up to 8 (one
        // source pass for p <= 8 — reads halved vs 4+4 grouping); R <= 4
        // takes the double-chunk variant (broadcasts per byte halved).
        let mut row = 0usize;
        while row < out.len() {
            let g = (out.len() - row).min(8);
            let tb = &gftbls[row * k * 8..(row + g) * k * 8];
            let group = &mut out[row..row + g];
            // SAFETY: see the wrapper doc above.
            unsafe {
                match g {
                    8 => encode_group_gfni::<8>(tb, data, group),
                    7 => encode_group_gfni::<7>(tb, data, group),
                    6 => encode_group_gfni::<6>(tb, data, group),
                    5 => encode_group_gfni::<5>(tb, data, group),
                    4 => encode_group_gfni2::<4>(tb, data, group),
                    3 => encode_group_gfni2::<3>(tb, data, group),
                    2 => encode_group_gfni2::<2>(tb, data, group),
                    _ => encode_group_gfni2::<1>(tb, data, group),
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

    /// AVX2 XOR parity: single pass, two ymm streams per 64-byte chunk — the
    /// fold shape that lost as scalar u64 code (codegen) but wins expressed
    /// directly: N loads + N−1 xors + ONE store per chunk.
    ///
    /// # Safety
    /// Caller guarantees AVX2 and equal slice lengths (asserted).
    #[target_feature(enable = "avx2")]
    unsafe fn raid_xor_avx2_inner(sources: &[&[u8]], parity: &mut [u8]) {
        let n = parity.len();
        let mut i = 0usize;
        while i + 64 <= n {
            // SAFETY: i + 64 <= n == every slice's len (asserted by wrapper).
            unsafe {
                let mut a0 = _mm256_loadu_si256(sources[0].as_ptr().add(i) as *const __m256i);
                let mut a1 =
                    _mm256_loadu_si256(sources[0].as_ptr().add(i + 32) as *const __m256i);
                for s in &sources[1..] {
                    a0 = _mm256_xor_si256(
                        a0,
                        _mm256_loadu_si256(s.as_ptr().add(i) as *const __m256i),
                    );
                    a1 = _mm256_xor_si256(
                        a1,
                        _mm256_loadu_si256(s.as_ptr().add(i + 32) as *const __m256i),
                    );
                }
                _mm256_storeu_si256(parity.as_mut_ptr().add(i) as *mut __m256i, a0);
                _mm256_storeu_si256(parity.as_mut_ptr().add(i + 32) as *mut __m256i, a1);
            }
            i += 64;
        }
        for x in i..n {
            let mut acc = 0u8;
            for s in sources {
                acc ^= s[x];
            }
            parity[x] = acc;
        }
    }

    pub(crate) fn raid_xor_avx2(sources: &[&[u8]], parity: &mut [u8]) {
        for s in sources {
            assert_eq!(s.len(), parity.len(), "xor length mismatch");
        }
        assert!(sources.len() >= 2, "xor needs two sources");
        ACCEL_CENSUS_BYTES
            .fetch_add((sources.len() * parity.len()) as u64, Ordering::Relaxed);
        // SAFETY: handed out only after avx2 detection; lengths asserted.
        unsafe { raid_xor_avx2_inner(sources, parity) }
    }

    /// AVX2 P+Q: 32 byte lanes per step. The ×2-in-GF(2^8) recurrence per
    /// byte lane: `q<<1` is a byte-wise add(q,q); the 0x1d fold mask comes
    /// from sign-compare (byte high bit ⇒ lane 0xff).
    ///
    /// # Safety
    /// Caller guarantees AVX2 and equal slice lengths (asserted).
    #[target_feature(enable = "avx2")]
    unsafe fn raid_pq_avx2_inner(sources: &[&[u8]], p: &mut [u8], q: &mut [u8]) {
        let n = p.len();
        let last = sources.len() - 1;
        let poly = _mm256_set1_epi8(0x1d);
        let zero = _mm256_setzero_si256();
        let mut i = 0usize;
        while i + 32 <= n {
            // SAFETY: i + 32 <= n == every slice's len (asserted by wrapper).
            unsafe {
                let s_last = _mm256_loadu_si256(sources[last].as_ptr().add(i) as *const __m256i);
                let mut pw = s_last;
                let mut qw = s_last;
                for j in (0..last).rev() {
                    let s = _mm256_loadu_si256(sources[j].as_ptr().add(i) as *const __m256i);
                    pw = _mm256_xor_si256(pw, s);
                    let doubled = _mm256_xor_si256(
                        _mm256_add_epi8(qw, qw),
                        _mm256_and_si256(_mm256_cmpgt_epi8(zero, qw), poly),
                    );
                    qw = _mm256_xor_si256(s, doubled);
                }
                _mm256_storeu_si256(p.as_mut_ptr().add(i) as *mut __m256i, pw);
                _mm256_storeu_si256(q.as_mut_ptr().add(i) as *mut __m256i, qw);
            }
            i += 32;
        }
        for x in i..n {
            let mut pb = sources[last][x];
            let mut qb = pb;
            for j in (0..last).rev() {
                let s = sources[j][x];
                pb ^= s;
                qb = s ^ ((qb << 1) ^ (if qb & 0x80 != 0 { 0x1d } else { 0 }));
            }
            p[x] = pb;
            q[x] = qb;
        }
    }

    pub(crate) fn raid_pq_avx2(sources: &[&[u8]], p: &mut [u8], q: &mut [u8]) {
        assert_eq!(p.len(), q.len(), "p/q length mismatch");
        for s in sources {
            assert_eq!(s.len(), p.len(), "pq length mismatch");
        }
        assert!(sources.len() >= 2, "pq needs two sources");
        ACCEL_CENSUS_BYTES.fetch_add((sources.len() * p.len()) as u64, Ordering::Relaxed);
        // SAFETY: handed out only after avx2 detection; lengths asserted.
        unsafe { raid_pq_avx2_inner(sources, p, q) }
    }

    /// SSE2 (baseline) RAID fallback: same single-pass fold shapes at 16-byte
    /// width, two streams per 32-byte chunk. No detection — SSE2 is x86-64
    /// baseline, so these are safe plain functions.
    pub(crate) fn raid_xor_sse2(sources: &[&[u8]], parity: &mut [u8]) {
        for s in sources {
            assert_eq!(s.len(), parity.len(), "xor length mismatch");
        }
        assert!(sources.len() >= 2, "xor needs two sources");
        ACCEL_CENSUS_BYTES
            .fetch_add((sources.len() * parity.len()) as u64, Ordering::Relaxed);
        let n = parity.len();
        let mut i = 0usize;
        // SAFETY: SSE2 is baseline on x86-64; i + 32 <= n bounds every access.
        unsafe {
            while i + 32 <= n {
                let mut a0 = _mm_loadu_si128(sources[0].as_ptr().add(i) as *const __m128i);
                let mut a1 = _mm_loadu_si128(sources[0].as_ptr().add(i + 16) as *const __m128i);
                for s in &sources[1..] {
                    a0 = _mm_xor_si128(a0, _mm_loadu_si128(s.as_ptr().add(i) as *const __m128i));
                    a1 = _mm_xor_si128(
                        a1,
                        _mm_loadu_si128(s.as_ptr().add(i + 16) as *const __m128i),
                    );
                }
                _mm_storeu_si128(parity.as_mut_ptr().add(i) as *mut __m128i, a0);
                _mm_storeu_si128(parity.as_mut_ptr().add(i + 16) as *mut __m128i, a1);
                i += 32;
            }
        }
        for x in i..n {
            let mut acc = 0u8;
            for s in sources {
                acc ^= s[x];
            }
            parity[x] = acc;
        }
    }

    pub(crate) fn raid_pq_sse2(sources: &[&[u8]], p: &mut [u8], q: &mut [u8]) {
        assert_eq!(p.len(), q.len(), "p/q length mismatch");
        for s in sources {
            assert_eq!(s.len(), p.len(), "pq length mismatch");
        }
        assert!(sources.len() >= 2, "pq needs two sources");
        ACCEL_CENSUS_BYTES.fetch_add((sources.len() * p.len()) as u64, Ordering::Relaxed);
        let n = p.len();
        let last = sources.len() - 1;
        let mut i = 0usize;
        // SAFETY: SSE2 is baseline on x86-64; i + 16 <= n bounds every access.
        unsafe {
            let poly = _mm_set1_epi8(0x1d);
            let zero = _mm_setzero_si128();
            while i + 16 <= n {
                let s_last = _mm_loadu_si128(sources[last].as_ptr().add(i) as *const __m128i);
                let mut pw = s_last;
                let mut qw = s_last;
                for j in (0..last).rev() {
                    let s = _mm_loadu_si128(sources[j].as_ptr().add(i) as *const __m128i);
                    pw = _mm_xor_si128(pw, s);
                    let doubled = _mm_xor_si128(
                        _mm_add_epi8(qw, qw),
                        _mm_and_si128(_mm_cmpgt_epi8(zero, qw), poly),
                    );
                    qw = _mm_xor_si128(s, doubled);
                }
                _mm_storeu_si128(p.as_mut_ptr().add(i) as *mut __m128i, pw);
                _mm_storeu_si128(q.as_mut_ptr().add(i) as *mut __m128i, qw);
                i += 16;
            }
        }
        for x in i..n {
            let mut pb = sources[last][x];
            let mut qb = pb;
            for j in (0..last).rev() {
                let s = sources[j][x];
                pb ^= s;
                qb = s ^ ((qb << 1) ^ (if qb & 0x80 != 0 { 0x1d } else { 0 }));
            }
            p[x] = pb;
            q[x] = qb;
        }
    }

    /// Shared validation for fused update wrappers.
    fn check_update(gftbls: &[u8], k: usize, vec_i: usize, src: &[u8], outs: &[&mut [u8]], tb: usize) {
        assert!(vec_i < k, "source index in range");
        assert!(
            gftbls.len() >= ((outs.len().saturating_sub(1)) * k + vec_i + 1) * tb,
            "gftbls too short"
        );
        for o in outs {
            assert_eq!(o.len(), src.len(), "update length mismatch");
        }
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
    }

    /// Fused update, GFNI: p matrices broadcast ONCE PER CALL (amortized over
    /// the whole source — the placement where preloading genuinely pays),
    /// then one pass over the source updating every row per 32-byte chunk.
    ///
    /// # Safety
    /// Caller guarantees GFNI+AVX2, `out.len() == R`, equal lengths, tables
    /// present (all re-asserted by the wrapper).
    #[target_feature(enable = "gfni,avx2")]
    unsafe fn update_group_gfni<const R: usize>(
        mats: &[__m256i; 8],
        src: &[u8],
        outs: &mut [&mut [u8]],
    ) {
        let n = src.len();
        let mut i = 0usize;
        while i + 32 <= n {
            // SAFETY: i + 32 <= n == every slice's len (asserted by wrapper).
            unsafe {
                let s = _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i);
                for r in 0..R {
                    let d = _mm256_loadu_si256(outs[r].as_ptr().add(i) as *const __m256i);
                    _mm256_storeu_si256(
                        outs[r].as_mut_ptr().add(i) as *mut __m256i,
                        _mm256_xor_si256(d, _mm256_gf2p8affine_epi64_epi8::<0>(s, mats[r])),
                    );
                }
            }
            i += 32;
        }
        if i < n {
            for r in 0..R {
                let m = unsafe { core::mem::transmute::<__m256i, [u64; 4]>(mats[r]) }[0];
                let c = coeff_from_affine(m);
                for (d, &s) in outs[r][i..].iter_mut().zip(&src[i..]) {
                    *d ^= rusty_erasure_core::gf::mul(c, s);
                }
            }
        }
    }

    pub(crate) fn update_gfni(gftbls: &[u8], k: usize, vec_i: usize, src: &[u8], outs: &mut [&mut [u8]]) {
        check_update(gftbls, k, vec_i, src, outs, 8);
        let mut row = 0usize;
        while row < outs.len() {
            let g = (outs.len() - row).min(8);
            // SAFETY: kernels_at gated on gfni+avx2; indices bounded by check_update.
            unsafe {
                let mut mats = [_mm256_setzero_si256(); 8];
                for (ri, m) in mats.iter_mut().enumerate().take(g) {
                    *m = _mm256_set1_epi64x(affine_at(gftbls, (row + ri) * k + vec_i) as i64);
                }
                let group = &mut outs[row..row + g];
                match g {
                    8 => update_group_gfni::<8>(&mats, src, group),
                    7 => update_group_gfni::<7>(&mats, src, group),
                    6 => update_group_gfni::<6>(&mats, src, group),
                    5 => update_group_gfni::<5>(&mats, src, group),
                    4 => update_group_gfni::<4>(&mats, src, group),
                    3 => update_group_gfni::<3>(&mats, src, group),
                    2 => update_group_gfni::<2>(&mats, src, group),
                    _ => update_group_gfni::<1>(&mats, src, group),
                }
            }
            row += g;
        }
    }

    /// Fused update, nibble sets: tables for up to 4 rows held in registers
    /// per group, one source pass.
    ///
    /// # Safety
    /// Caller guarantees the ISA and validated lengths.
    #[target_feature(enable = "avx2")]
    unsafe fn update_group_avx2<const R: usize>(
        tbls: &[(__m256i, __m256i); 4],
        src: &[u8],
        outs: &mut [&mut [u8]],
        scalar_tbls: &[&[u8; TABLE_BYTES]],
    ) {
        let n = src.len();
        let mask = _mm256_set1_epi8(0x0f);
        let mut i = 0usize;
        while i + 32 <= n {
            // SAFETY: bounds asserted by wrapper.
            unsafe {
                let s = _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i);
                let lo = _mm256_and_si256(s, mask);
                let hi = _mm256_and_si256(_mm256_srli_epi64::<4>(s), mask);
                for r in 0..R {
                    let p = _mm256_xor_si256(
                        _mm256_shuffle_epi8(tbls[r].0, lo),
                        _mm256_shuffle_epi8(tbls[r].1, hi),
                    );
                    let d = _mm256_loadu_si256(outs[r].as_ptr().add(i) as *const __m256i);
                    _mm256_storeu_si256(
                        outs[r].as_mut_ptr().add(i) as *mut __m256i,
                        _mm256_xor_si256(d, p),
                    );
                }
            }
            i += 32;
        }
        for r in 0..R {
            for (d, &s) in outs[r][i..].iter_mut().zip(&src[i..]) {
                *d ^= table_mul(scalar_tbls[r], s);
            }
        }
    }

    pub(crate) fn update_avx2(gftbls: &[u8], k: usize, vec_i: usize, src: &[u8], outs: &mut [&mut [u8]]) {
        check_update(gftbls, k, vec_i, src, outs, TABLE_BYTES);
        let mut row = 0usize;
        while row < outs.len() {
            let g = (outs.len() - row).min(4);
            let mut scalar_tbls: Vec<&[u8; TABLE_BYTES]> = Vec::with_capacity(g);
            for ri in 0..g {
                let start = ((row + ri) * k + vec_i) * TABLE_BYTES;
                scalar_tbls.push(gftbls[start..start + TABLE_BYTES].try_into().expect("checked"));
            }
            // SAFETY: kernels_at gated on avx2; lengths asserted.
            unsafe {
                let mut tbls = [(_mm256_setzero_si256(), _mm256_setzero_si256()); 4];
                for (ri, t) in tbls.iter_mut().enumerate().take(g) {
                    let p = scalar_tbls[ri].as_ptr();
                    *t = (
                        _mm256_broadcastsi128_si256(_mm_loadu_si128(p as *const __m128i)),
                        _mm256_broadcastsi128_si256(_mm_loadu_si128(p.add(16) as *const __m128i)),
                    );
                }
                let group = &mut outs[row..row + g];
                match g {
                    4 => update_group_avx2::<4>(&tbls, src, group, &scalar_tbls),
                    3 => update_group_avx2::<3>(&tbls, src, group, &scalar_tbls),
                    2 => update_group_avx2::<2>(&tbls, src, group, &scalar_tbls),
                    _ => update_group_avx2::<1>(&tbls, src, group, &scalar_tbls),
                }
            }
            row += g;
        }
    }

    /// SSSE3 fused update: same structure at 16 bytes per step.
    ///
    /// # Safety
    /// Caller guarantees SSSE3 and validated lengths.
    #[target_feature(enable = "ssse3")]
    unsafe fn update_group_ssse3<const R: usize>(
        tbls: &[(__m128i, __m128i); 4],
        src: &[u8],
        outs: &mut [&mut [u8]],
        scalar_tbls: &[&[u8; TABLE_BYTES]],
    ) {
        let n = src.len();
        let mask = _mm_set1_epi8(0x0f);
        let mut i = 0usize;
        while i + 16 <= n {
            // SAFETY: bounds asserted by wrapper.
            unsafe {
                let s = _mm_loadu_si128(src.as_ptr().add(i) as *const __m128i);
                let lo = _mm_and_si128(s, mask);
                let hi = _mm_and_si128(_mm_srli_epi64::<4>(s), mask);
                for r in 0..R {
                    let p = _mm_xor_si128(
                        _mm_shuffle_epi8(tbls[r].0, lo),
                        _mm_shuffle_epi8(tbls[r].1, hi),
                    );
                    let d = _mm_loadu_si128(outs[r].as_ptr().add(i) as *const __m128i);
                    _mm_storeu_si128(outs[r].as_mut_ptr().add(i) as *mut __m128i, _mm_xor_si128(d, p));
                }
            }
            i += 16;
        }
        for r in 0..R {
            for (d, &s) in outs[r][i..].iter_mut().zip(&src[i..]) {
                *d ^= table_mul(scalar_tbls[r], s);
            }
        }
    }

    pub(crate) fn update_ssse3(gftbls: &[u8], k: usize, vec_i: usize, src: &[u8], outs: &mut [&mut [u8]]) {
        check_update(gftbls, k, vec_i, src, outs, TABLE_BYTES);
        let mut row = 0usize;
        while row < outs.len() {
            let g = (outs.len() - row).min(4);
            let mut scalar_tbls: Vec<&[u8; TABLE_BYTES]> = Vec::with_capacity(g);
            for ri in 0..g {
                let start = ((row + ri) * k + vec_i) * TABLE_BYTES;
                scalar_tbls.push(gftbls[start..start + TABLE_BYTES].try_into().expect("checked"));
            }
            // SAFETY: kernels_at gated on ssse3; lengths asserted.
            unsafe {
                let mut tbls = [(_mm_setzero_si128(), _mm_setzero_si128()); 4];
                for (ri, t) in tbls.iter_mut().enumerate().take(g) {
                    let p = scalar_tbls[ri].as_ptr();
                    *t = (
                        _mm_loadu_si128(p as *const __m128i),
                        _mm_loadu_si128(p.add(16) as *const __m128i),
                    );
                }
                let group = &mut outs[row..row + g];
                match g {
                    4 => update_group_ssse3::<4>(&tbls, src, group, &scalar_tbls),
                    3 => update_group_ssse3::<3>(&tbls, src, group, &scalar_tbls),
                    2 => update_group_ssse3::<2>(&tbls, src, group, &scalar_tbls),
                    _ => update_group_ssse3::<1>(&tbls, src, group, &scalar_tbls),
                }
            }
            row += g;
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

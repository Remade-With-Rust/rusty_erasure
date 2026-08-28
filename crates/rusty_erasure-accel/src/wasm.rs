//! wasm32 SIMD128 kernels: the nibble technique via `u8x16_swizzle` —
//! PSHUFB's twin (out-of-range lane indices select zero; ours are always
//! 0..=15). 16 bytes per step, 4-row fused encode.
//!
//! wasm has no runtime feature detection: the module only exists when the
//! build enables `+simd128` (`RUSTFLAGS="-C target-feature=+simd128"`), and
//! [`kernels`] returns `None` otherwise — the scalar core is the fallback.
//! This is the org differentiator: a browser SpaceDB peer erasure-coding
//! shards locally, something no C erasure library offers (mission plan §1).

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) mod imp {
    use core::arch::wasm32::*;
    use core::sync::atomic::Ordering;

    use rusty_erasure_core::tables::{TABLE_BYTES, table_mul};

    use crate::ACCEL_CENSUS_BYTES;
    use crate::tail::encode_tail_nibble;

    fn check_encode(gftbls: &[u8], data: &[&[u8]], out: &[&mut [u8]]) -> usize {
        let k = data.len();
        let len = out.first().map_or(0, |b| b.len());
        assert!(
            gftbls.len() >= out.len() * k * TABLE_BYTES,
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

    /// # Safety
    /// Caller guarantees `out.len() == R`, equal slice lengths, and `R * k`
    /// nibble tables in `gftbls` (all re-asserted by the safe wrapper).
    unsafe fn encode_group_simd128<const R: usize>(
        gftbls: &[u8],
        data: &[&[u8]],
        out: &mut [&mut [u8]],
    ) {
        let k = data.len();
        let len = out[0].len();
        let mask = u8x16_splat(0x0f);
        let mut i = 0usize;
        while i + 16 <= len {
            let mut acc = [u8x16_splat(0); R];
            for (j, src) in data.iter().enumerate() {
                // SAFETY: i + 16 <= len == src.len() (asserted by the wrapper).
                let s = unsafe { v128_load(src.as_ptr().add(i) as *const v128) };
                let lo = v128_and(s, mask);
                let hi = v128_and(u8x16_shr(s, 4), mask);
                for r in 0..R {
                    let start = (r * k + j) * TABLE_BYTES;
                    // SAFETY: start + 32 <= gftbls.len() (asserted).
                    let (tlo, thi) = unsafe {
                        (
                            v128_load(gftbls.as_ptr().add(start) as *const v128),
                            v128_load(gftbls.as_ptr().add(start + 16) as *const v128),
                        )
                    };
                    acc[r] = v128_xor(
                        acc[r],
                        v128_xor(u8x16_swizzle(tlo, lo), u8x16_swizzle(thi, hi)),
                    );
                }
            }
            for r in 0..R {
                // SAFETY: i + 16 <= len == out[r].len() (asserted).
                unsafe { v128_store(out[r].as_mut_ptr().add(i) as *mut v128, acc[r]) };
            }
            i += 16;
        }
        encode_tail_nibble(gftbls, data, out, i);
    }

    pub(crate) fn encode_simd128(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
        let k = data.len();
        check_encode(gftbls, data, out);
        let mut row = 0usize;
        while row < out.len() {
            let g = (out.len() - row).min(4);
            let tb = &gftbls[row * k * TABLE_BYTES..(row + g) * k * TABLE_BYTES];
            let group = &mut out[row..row + g];
            // SAFETY: simd128 statically enabled for this module; lengths checked.
            unsafe {
                match g {
                    4 => encode_group_simd128::<4>(tb, data, group),
                    3 => encode_group_simd128::<3>(tb, data, group),
                    2 => encode_group_simd128::<2>(tb, data, group),
                    _ => encode_group_simd128::<1>(tb, data, group),
                }
            }
            row += g;
        }
    }

    /// # Safety
    /// Caller guarantees equal slice lengths (asserted by the wrapper).
    unsafe fn mad_simd128_inner(tbl: &[u8; TABLE_BYTES], src: &[u8], dest: &mut [u8]) {
        let n = dest.len();
        // SAFETY: the table is exactly 32 bytes by type.
        let (tlo, thi) = unsafe {
            (
                v128_load(tbl.as_ptr() as *const v128),
                v128_load(tbl.as_ptr().add(16) as *const v128),
            )
        };
        let mask = u8x16_splat(0x0f);
        let mut i = 0usize;
        while i + 16 <= n {
            // SAFETY: i + 16 <= n == src.len() == dest.len() (asserted).
            unsafe {
                let s = v128_load(src.as_ptr().add(i) as *const v128);
                let d = v128_load(dest.as_ptr().add(i) as *const v128);
                let lo = v128_and(s, mask);
                let hi = v128_and(u8x16_shr(s, 4), mask);
                let p = v128_xor(u8x16_swizzle(tlo, lo), u8x16_swizzle(thi, hi));
                v128_store(dest.as_mut_ptr().add(i) as *mut v128, v128_xor(d, p));
            }
            i += 16;
        }
        for (d, &s) in dest[i..].iter_mut().zip(&src[i..]) {
            *d ^= table_mul(tbl, s);
        }
    }

    /// Fused update: one pass over the source folds it into every row.
    pub(crate) fn update_simd128(gftbls: &[u8], k: usize, vec_i: usize, src: &[u8], outs: &mut [&mut [u8]]) {
        assert!(vec_i < k, "source index in range");
        assert!(
            gftbls.len() >= ((outs.len().saturating_sub(1)) * k + vec_i + 1) * TABLE_BYTES,
            "gftbls too short"
        );
        for o in outs.iter() {
            assert_eq!(o.len(), src.len(), "update length mismatch");
        }
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
        let n = src.len();
        for row_base in (0..outs.len()).step_by(4) {
            let g = (outs.len() - row_base).min(4);
            let mut stbls: [&[u8; TABLE_BYTES]; 4] = [gftbls[..TABLE_BYTES].try_into().expect("checked"); 4];
            for (ri, st) in stbls.iter_mut().enumerate().take(g) {
                let start = ((row_base + ri) * k + vec_i) * TABLE_BYTES;
                *st = gftbls[start..start + TABLE_BYTES].try_into().expect("checked");
            }
            let group = &mut outs[row_base..row_base + g];
            let mut i = 0usize;
            // SAFETY: simd128 statically enabled; i + 16 <= n bounds every access.
            unsafe {
                let mask = u8x16_splat(0x0f);
                let mut tl = [u8x16_splat(0); 4];
                let mut th = [u8x16_splat(0); 4];
                for ri in 0..g {
                    tl[ri] = v128_load(stbls[ri].as_ptr() as *const v128);
                    th[ri] = v128_load(stbls[ri].as_ptr().add(16) as *const v128);
                }
                while i + 16 <= n {
                    let s = v128_load(src.as_ptr().add(i) as *const v128);
                    let lo = v128_and(s, mask);
                    let hi = v128_and(u8x16_shr(s, 4), mask);
                    for (ri, out) in group.iter_mut().enumerate() {
                        let p = v128_xor(u8x16_swizzle(tl[ri], lo), u8x16_swizzle(th[ri], hi));
                        let d = v128_load(out.as_ptr().add(i) as *const v128);
                        v128_store(out.as_mut_ptr().add(i) as *mut v128, v128_xor(d, p));
                    }
                    i += 16;
                }
            }
            for (ri, out) in group.iter_mut().enumerate() {
                for (d, &s) in out[i..].iter_mut().zip(&src[i..]) {
                    *d ^= table_mul(stbls[ri], s);
                }
            }
        }
    }

    pub(crate) fn mad_simd128(tbl: &[u8], src: &[u8], dest: &mut [u8]) {
        let tbl: &[u8; TABLE_BYTES] = tbl.try_into().expect("nibble mad takes a 32-byte table");
        assert_eq!(src.len(), dest.len(), "mad length mismatch");
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
        // SAFETY: simd128 statically enabled for this module; lengths asserted.
        unsafe { mad_simd128_inner(tbl, src, dest) }
    }
}

/// The SIMD128 kernel set (wasm32 built with `+simd128` only; `None`
/// otherwise, including wasm builds without the feature).
pub fn kernels() -> Option<rusty_erasure_core::kernel::Kernels> {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        use rusty_erasure_core::tables;
        Some(rusty_erasure_core::kernel::Kernels {
            init: tables::init_tables,
            table_bytes: tables::TABLE_BYTES,
            encode: imp::encode_simd128,
            mad: imp::mad_simd128,
            update: imp::update_simd128,
            name: "wasm32/simd128",
            census: &crate::ACCEL_CENSUS_BYTES,
        })
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    None
}

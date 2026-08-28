//! aarch64 NEON kernels: the PSHUFB nibble technique via `vqtbl1q_u8`
//! (TBL), 16 bytes per step, 4-row fused encode — the per-arch mirror the
//! codec-vectorize-kernel non-negotiable demands.
//!
//! NEON is architecturally mandatory on AArch64, so unlike x86 there is no
//! runtime dispatch: on an aarch64 target these kernels always apply.
//! Execution proof runs on the arm64 CI job (M6); this module is written to
//! the same oracle discipline as x86 and gated by the same portable
//! `*_matches_scalar` test.

#[cfg(target_arch = "aarch64")]
pub(crate) mod imp {
    use core::arch::aarch64::*;
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

    /// Encode a group of `R` rows fused, 16 bytes per step.
    ///
    /// # Safety
    /// Caller guarantees `out.len() == R`, equal slice lengths, and `R * k`
    /// nibble tables in `gftbls` (all re-asserted by the safe wrapper).
    unsafe fn encode_group_neon<const R: usize>(
        gftbls: &[u8],
        data: &[&[u8]],
        out: &mut [&mut [u8]],
    ) {
        let k = data.len();
        let len = out[0].len();
        let mut i = 0usize;
        // SAFETY: NEON is baseline on every aarch64 target we ship; every
        // load/store index is bounded by the wrapper's length assertions
        // (i + 16 <= len == each slice's len; table offsets < gftbls.len()).
        unsafe {
            let mask = vdupq_n_u8(0x0f);
            while i + 16 <= len {
                let mut acc = [vdupq_n_u8(0); R];
                for (j, src) in data.iter().enumerate() {
                    let s = vld1q_u8(src.as_ptr().add(i));
                    let lo = vandq_u8(s, mask);
                    let hi = vandq_u8(vshrq_n_u8::<4>(s), mask);
                    for r in 0..R {
                        let start = (r * k + j) * TABLE_BYTES;
                        let tlo = vld1q_u8(gftbls.as_ptr().add(start));
                        let thi = vld1q_u8(gftbls.as_ptr().add(start + 16));
                        acc[r] = veorq_u8(
                            acc[r],
                            veorq_u8(vqtbl1q_u8(tlo, lo), vqtbl1q_u8(thi, hi)),
                        );
                    }
                }
                for r in 0..R {
                    vst1q_u8(out[r].as_mut_ptr().add(i), acc[r]);
                }
                i += 16;
            }
        }
        encode_tail_nibble(gftbls, data, out, i);
    }

    pub(crate) fn encode_neon(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
        let k = data.len();
        check_encode(gftbls, data, out);
        let mut row = 0usize;
        while row < out.len() {
            let g = (out.len() - row).min(4);
            let tb = &gftbls[row * k * TABLE_BYTES..(row + g) * k * TABLE_BYTES];
            let group = &mut out[row..row + g];
            // SAFETY: NEON is baseline on aarch64; lengths checked above.
            unsafe {
                match g {
                    4 => encode_group_neon::<4>(tb, data, group),
                    3 => encode_group_neon::<3>(tb, data, group),
                    2 => encode_group_neon::<2>(tb, data, group),
                    _ => encode_group_neon::<1>(tb, data, group),
                }
            }
            row += g;
        }
    }

    /// # Safety
    /// Caller guarantees equal slice lengths (asserted by the wrapper).
    unsafe fn mad_neon_inner(tbl: &[u8; TABLE_BYTES], src: &[u8], dest: &mut [u8]) {
        let n = dest.len();
        let mut i = 0usize;
        // SAFETY: NEON is baseline on every aarch64 target we ship; the table
        // is exactly 32 bytes by type and i + 16 <= n bounds every access
        // (src.len() == dest.len() asserted by the wrapper).
        unsafe {
            let tlo = vld1q_u8(tbl.as_ptr());
            let thi = vld1q_u8(tbl.as_ptr().add(16));
            let mask = vdupq_n_u8(0x0f);
            while i + 16 <= n {
                let s = vld1q_u8(src.as_ptr().add(i));
                let d = vld1q_u8(dest.as_ptr().add(i));
                let lo = vandq_u8(s, mask);
                let hi = vandq_u8(vshrq_n_u8::<4>(s), mask);
                let p = veorq_u8(vqtbl1q_u8(tlo, lo), vqtbl1q_u8(thi, hi));
                vst1q_u8(dest.as_mut_ptr().add(i), veorq_u8(d, p));
                i += 16;
            }
        }
        for (d, &s) in dest[i..].iter_mut().zip(&src[i..]) {
            *d ^= table_mul(tbl, s);
        }
    }

    pub(crate) fn mad_neon(tbl: &[u8], src: &[u8], dest: &mut [u8]) {
        let tbl: &[u8; TABLE_BYTES] = tbl.try_into().expect("nibble mad takes a 32-byte table");
        assert_eq!(src.len(), dest.len(), "mad length mismatch");
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
        // SAFETY: NEON is baseline on aarch64; lengths asserted.
        unsafe { mad_neon_inner(tbl, src, dest) }
    }
}

/// The NEON kernel set (aarch64 targets only; `None` elsewhere).
pub fn kernels() -> Option<rusty_erasure_core::kernel::Kernels> {
    #[cfg(target_arch = "aarch64")]
    {
        use rusty_erasure_core::tables;
        Some(rusty_erasure_core::kernel::Kernels {
            init: tables::init_tables,
            table_bytes: tables::TABLE_BYTES,
            encode: imp::encode_neon,
            mad: imp::mad_neon,
            name: "aarch64/neon",
            census: &crate::ACCEL_CENSUS_BYTES,
        })
    }
    #[cfg(not(target_arch = "aarch64"))]
    None
}

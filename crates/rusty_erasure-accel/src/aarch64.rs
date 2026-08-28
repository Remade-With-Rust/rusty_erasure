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
            // Double-chunk main loop: each (row, source) table load serves
            // BOTH 16-byte chunks — the GFNI double-chunk brick deployed to
            // the NEON nibble set (32 vector regs hold it easily).
            while i + 32 <= len {
                let mut acc0 = [vdupq_n_u8(0); R];
                let mut acc1 = [vdupq_n_u8(0); R];
                for (j, src) in data.iter().enumerate() {
                    let s0 = vld1q_u8(src.as_ptr().add(i));
                    let s1 = vld1q_u8(src.as_ptr().add(i + 16));
                    let lo0 = vandq_u8(s0, mask);
                    let hi0 = vandq_u8(vshrq_n_u8::<4>(s0), mask);
                    let lo1 = vandq_u8(s1, mask);
                    let hi1 = vandq_u8(vshrq_n_u8::<4>(s1), mask);
                    for r in 0..R {
                        let start = (r * k + j) * TABLE_BYTES;
                        let tlo = vld1q_u8(gftbls.as_ptr().add(start));
                        let thi = vld1q_u8(gftbls.as_ptr().add(start + 16));
                        acc0[r] = veorq_u8(
                            acc0[r],
                            veorq_u8(vqtbl1q_u8(tlo, lo0), vqtbl1q_u8(thi, hi0)),
                        );
                        acc1[r] = veorq_u8(
                            acc1[r],
                            veorq_u8(vqtbl1q_u8(tlo, lo1), vqtbl1q_u8(thi, hi1)),
                        );
                    }
                }
                for r in 0..R {
                    vst1q_u8(out[r].as_mut_ptr().add(i), acc0[r]);
                    vst1q_u8(out[r].as_mut_ptr().add(i + 16), acc1[r]);
                }
                i += 32;
            }
            if i + 16 <= len {
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

    /// NEON RAID xor: single-pass fold, two 16-byte streams per chunk.
    pub(crate) fn raid_xor_neon(sources: &[&[u8]], parity: &mut [u8]) {
        for s in sources {
            assert_eq!(s.len(), parity.len(), "xor length mismatch");
        }
        assert!(sources.len() >= 2, "xor needs two sources");
        ACCEL_CENSUS_BYTES
            .fetch_add((sources.len() * parity.len()) as u64, Ordering::Relaxed);
        let n = parity.len();
        let mut i = 0usize;
        // SAFETY: NEON is baseline aarch64; i + 32 <= n bounds every access.
        unsafe {
            while i + 32 <= n {
                let mut a0 = vld1q_u8(sources[0].as_ptr().add(i));
                let mut a1 = vld1q_u8(sources[0].as_ptr().add(i + 16));
                for s in &sources[1..] {
                    a0 = veorq_u8(a0, vld1q_u8(s.as_ptr().add(i)));
                    a1 = veorq_u8(a1, vld1q_u8(s.as_ptr().add(i + 16)));
                }
                vst1q_u8(parity.as_mut_ptr().add(i), a0);
                vst1q_u8(parity.as_mut_ptr().add(i + 16), a1);
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

    /// NEON RAID P+Q: the ×2 recurrence per byte lane — `q<<1` via
    /// `vshlq_n_u8`, the 0x1d fold mask from an arithmetic sign shift.
    pub(crate) fn raid_pq_neon(sources: &[&[u8]], p: &mut [u8], q: &mut [u8]) {
        assert_eq!(p.len(), q.len(), "p/q length mismatch");
        for s in sources {
            assert_eq!(s.len(), p.len(), "pq length mismatch");
        }
        assert!(sources.len() >= 2, "pq needs two sources");
        ACCEL_CENSUS_BYTES.fetch_add((sources.len() * p.len()) as u64, Ordering::Relaxed);
        let n = p.len();
        let last = sources.len() - 1;
        let mut i = 0usize;
        // SAFETY: NEON is baseline aarch64; i + 16 <= n bounds every access.
        unsafe {
            let poly = vdupq_n_u8(0x1d);
            while i + 16 <= n {
                let s_last = vld1q_u8(sources[last].as_ptr().add(i));
                let mut pw = s_last;
                let mut qw = s_last;
                for j in (0..last).rev() {
                    let s = vld1q_u8(sources[j].as_ptr().add(i));
                    pw = veorq_u8(pw, s);
                    let mask = vreinterpretq_u8_s8(vshrq_n_s8::<7>(vreinterpretq_s8_u8(qw)));
                    let doubled = veorq_u8(vshlq_n_u8::<1>(qw), vandq_u8(mask, poly));
                    qw = veorq_u8(s, doubled);
                }
                vst1q_u8(p.as_mut_ptr().add(i), pw);
                vst1q_u8(q.as_mut_ptr().add(i), qw);
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

    /// Fused update: one pass over the source folds it into every row.
    pub(crate) fn update_neon(gftbls: &[u8], k: usize, vec_i: usize, src: &[u8], outs: &mut [&mut [u8]]) {
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
            // SAFETY: NEON is baseline aarch64; i + 16 <= n bounds every access.
            unsafe {
                let mask = vdupq_n_u8(0x0f);
                let mut tl = [vdupq_n_u8(0); 4];
                let mut th = [vdupq_n_u8(0); 4];
                for ri in 0..g {
                    tl[ri] = vld1q_u8(stbls[ri].as_ptr());
                    th[ri] = vld1q_u8(stbls[ri].as_ptr().add(16));
                }
                while i + 16 <= n {
                    let s = vld1q_u8(src.as_ptr().add(i));
                    let lo = vandq_u8(s, mask);
                    let hi = vandq_u8(vshrq_n_u8::<4>(s), mask);
                    for (ri, out) in group.iter_mut().enumerate() {
                        let p = veorq_u8(vqtbl1q_u8(tl[ri], lo), vqtbl1q_u8(th[ri], hi));
                        let d = vld1q_u8(out.as_ptr().add(i));
                        vst1q_u8(out.as_mut_ptr().add(i), veorq_u8(d, p));
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

    pub(crate) fn mad_neon(tbl: &[u8], src: &[u8], dest: &mut [u8]) {
        let tbl: &[u8; TABLE_BYTES] = tbl.try_into().expect("nibble mad takes a 32-byte table");
        assert_eq!(src.len(), dest.len(), "mad length mismatch");
        ACCEL_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
        // SAFETY: NEON is baseline on aarch64; lengths asserted.
        unsafe { mad_neon_inner(tbl, src, dest) }
    }
}

/// The NEON RAID kernels (aarch64 only; NEON is baseline — always `Some`).
pub fn raid_kernels() -> Option<crate::RaidKernels> {
    #[cfg(target_arch = "aarch64")]
    {
        Some((imp::raid_xor_neon, imp::raid_pq_neon))
    }
    #[cfg(not(target_arch = "aarch64"))]
    None
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
            update: imp::update_neon,
            name: "aarch64/neon",
            census: &crate::ACCEL_CENSUS_BYTES,
        })
    }
    #[cfg(not(target_arch = "aarch64"))]
    None
}

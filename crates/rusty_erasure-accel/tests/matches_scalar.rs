//! The scalar-twin oracle gate: every SIMD kernel, at every ISA level this
//! box supports, must be byte-identical to `Kernels::scalar()` over random +
//! edge inputs — lengths straddling every vector width and tail, row counts
//! straddling the fusion group size.

#![cfg(target_arch = "x86_64")]

use rusty_erasure_accel::x86::{Level, kernels_at};
use rusty_erasure_core::kernel::Kernels;
use rusty_erasure_core::tables;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

const EDGE_LENS: &[usize] = &[
    0, 1, 15, 16, 17, 31, 32, 33, 47, 63, 64, 65, 96, 255, 256, 1024, 4113,
];

fn level_or_skip(level: Level) -> Option<rusty_erasure_core::kernel::Kernels> {
    let k = kernels_at(level);
    if k.is_none() {
        // On x86-64 SSSE3 is effectively universal; require it so this gate
        // can never silently skip everything. AVX2 may honestly be absent.
        assert!(
            level != Level::Ssse3,
            "x86-64 host without SSSE3 — oracle gate cannot run"
        );
        eprintln!("skipping {level:?}: not supported on this CPU");
    }
    k
}

fn check_encode_matches(level: Level) {
    let Some(simd) = level_or_skip(level) else {
        return;
    };
    let scalar = Kernels::scalar();
    let mut rng = Rng(0x4C0D_E001 ^ level as u64);
    for &len in EDGE_LENS {
        for &(k, rows) in &[
            (1usize, 1usize),
            (2, 3),
            (4, 4),
            (5, 5),
            (10, 4),
            (10, 9),
            (16, 8),
            (32, 8),
        ] {
            let coeffs = rng.bytes(rows * k);
            let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();
            let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();

            let mut a = vec![vec![0u8; len]; rows];
            let mut b = vec![vec![0xAAu8; len]; rows]; // different init: encode must overwrite
            {
                // Each set builds tables in ITS OWN format from the same
                // coefficients — formats are kernel-private by design.
                let gftbls = (scalar.init)(&coeffs);
                let mut ar: Vec<&mut [u8]> = a.iter_mut().map(|x| x.as_mut_slice()).collect();
                (scalar.encode)(&gftbls, &data_refs, &mut ar);
            }
            {
                let gftbls = (simd.init)(&coeffs);
                let mut br: Vec<&mut [u8]> = b.iter_mut().map(|x| x.as_mut_slice()).collect();
                (simd.encode)(&gftbls, &data_refs, &mut br);
            }
            assert_eq!(a, b, "{level:?} encode k={k} rows={rows} len={len}");
        }
    }
}

fn check_mad_matches(level: Level) {
    let Some(simd) = level_or_skip(level) else {
        return;
    };
    let scalar = Kernels::scalar();
    let mut rng = Rng(0x4C0D_E002 ^ level as u64);
    for &len in EDGE_LENS {
        for _ in 0..4 {
            let c = rng.next() as u8;
            let stbl = (scalar.init)(&[c]);
            let vtbl = (simd.init)(&[c]);
            let src = rng.bytes(len);
            let base = rng.bytes(len);
            let mut a = base.clone();
            let mut b = base;
            (scalar.mad)(&stbl, &src, &mut a);
            (simd.mad)(&vtbl, &src, &mut b);
            assert_eq!(a, b, "{level:?} mad c={c} len={len}");
        }
    }
}

#[test]
fn ssse3_encode_matches_scalar() {
    check_encode_matches(Level::Ssse3);
}

#[test]
fn ssse3_mad_matches_scalar() {
    check_mad_matches(Level::Ssse3);
}

#[test]
fn avx2_encode_matches_scalar() {
    check_encode_matches(Level::Avx2);
}

#[test]
fn avx2_mad_matches_scalar() {
    check_mad_matches(Level::Avx2);
}

#[test]
fn gfni_encode_matches_scalar() {
    check_encode_matches(Level::Gfni);
}

#[test]
fn gfni_mad_matches_scalar() {
    check_mad_matches(Level::Gfni);
}

#[test]
fn gfni_mad_matches_scalar_for_every_coefficient() {
    // Exhaustive over c: a 256-byte buffer holding 0..=255 covers every
    // (c, x) pair — the affine matrices are proven against gf::mul in full.
    let Some(simd) = kernels_at(Level::Gfni) else {
        eprintln!("skipping exhaustive gfni: not supported on this CPU");
        return;
    };
    let scalar = rusty_erasure_core::kernel::Kernels::scalar();
    let src: Vec<u8> = (0..=255u8).collect();
    for c in 0..=255u8 {
        let stbl = (scalar.init)(&[c]);
        let vtbl = (simd.init)(&[c]);
        let mut a = vec![0u8; 256];
        let mut b = vec![0u8; 256];
        (scalar.mad)(&stbl, &src, &mut a);
        (simd.mad)(&vtbl, &src, &mut b);
        assert_eq!(a, b, "gfni mul for c={c}");
    }
}

fn check_update_matches(level: Level) {
    let Some(simd) = level_or_skip(level) else {
        return;
    };
    let scalar = Kernels::scalar();
    let mut rng = Rng(0x4C0D_E003 ^ level as u64);
    for &len in EDGE_LENS {
        for &(k, rows) in &[(1usize, 1usize), (3, 2), (5, 5), (10, 4), (10, 9), (16, 8)] {
            let coeffs = rng.bytes(rows * k);
            let vec_i = (rng.next() as usize) % k;
            let src = rng.bytes(len);
            let base: Vec<Vec<u8>> = (0..rows).map(|_| rng.bytes(len)).collect();

            // Reference: the per-row mad sequence on the SIMD set's own
            // tables (mad is itself oracle-gated against scalar).
            let g_simd = (simd.init)(&coeffs);
            let tb = simd.table_bytes;
            let mut want = base.clone();
            for (l, out) in want.iter_mut().enumerate() {
                let start = (l * k + vec_i) * tb;
                (simd.mad)(&g_simd[start..start + tb], &src, out);
            }

            // Fused update on the SIMD set.
            let mut got = base.clone();
            {
                let mut refs: Vec<&mut [u8]> = got.iter_mut().map(|b| b.as_mut_slice()).collect();
                (simd.update)(&g_simd, k, vec_i, &src, &mut refs);
            }
            assert_eq!(
                got, want,
                "{level:?} fused update k={k} rows={rows} len={len} vec_i={vec_i}"
            );

            // And the scalar set's fused update against its own mad sequence.
            let g_s = (scalar.init)(&coeffs);
            let mut want_s = base.clone();
            for (l, out) in want_s.iter_mut().enumerate() {
                let start = (l * k + vec_i) * scalar.table_bytes;
                (scalar.mad)(&g_s[start..start + scalar.table_bytes], &src, out);
            }
            let mut got_s = base.clone();
            {
                let mut refs: Vec<&mut [u8]> = got_s.iter_mut().map(|b| b.as_mut_slice()).collect();
                (scalar.update)(&g_s, k, vec_i, &src, &mut refs);
            }
            assert_eq!(
                got_s, want_s,
                "scalar fused update k={k} rows={rows} len={len}"
            );
            assert_eq!(got_s, got, "cross-set update disagreement");
        }
    }
}

#[test]
fn ssse3_update_matches_mad_sequence() {
    check_update_matches(Level::Ssse3);
}

#[test]
fn avx2_update_matches_mad_sequence() {
    check_update_matches(Level::Avx2);
}

#[test]
fn gfni_update_matches_mad_sequence() {
    check_update_matches(Level::Gfni);
}

/// Per-level RAID oracle: every (xor, pq) pair — including the ones dispatch
/// currently shadows (SSE2 under AVX2, AVX2-pq under GFNI-pq) — must be
/// byte-identical to the core scalar raid module. Lengths straddle the 128-
/// and 64-byte unroll boundaries the quad-chunk loops introduced.
fn check_raid_matches(level: Level) {
    let Some((xor, pq)) = rusty_erasure_accel::x86::raid_kernels_at(level) else {
        eprintln!("skipping raid {level:?}: not supported on this CPU");
        return;
    };
    let mut rng = Rng(0xC0DE_5EED_0BAD_F00D);
    for &len in &[
        0usize, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 96, 127, 128, 129, 191, 192, 193, 255, 256,
        257, 1024, 4113,
    ] {
        for &n in &[2usize, 3, 5, 8, 17] {
            let sources: Vec<Vec<u8>> = (0..n).map(|_| rng.bytes(len)).collect();
            let refs: Vec<&[u8]> = sources.iter().map(|s| s.as_slice()).collect();

            let mut want_x = vec![0u8; len];
            rusty_erasure_core::raid::xor_gen(&refs, &mut want_x).expect("core xor");
            let mut got_x = vec![0xAAu8; len];
            xor(&refs, &mut got_x);
            assert_eq!(want_x, got_x, "{level:?} raid xor n={n} len={len}");

            let mut want_p = vec![0u8; len];
            let mut want_q = vec![0u8; len];
            rusty_erasure_core::raid::pq_gen(&refs, &mut want_p, &mut want_q).expect("core pq");
            let mut got_p = vec![0xAAu8; len];
            let mut got_q = vec![0x55u8; len];
            pq(&refs, &mut got_p, &mut got_q);
            assert_eq!(want_p, got_p, "{level:?} raid p n={n} len={len}");
            assert_eq!(want_q, got_q, "{level:?} raid q n={n} len={len}");
        }
    }
}

#[test]
fn sse2_raid_matches_core() {
    check_raid_matches(Level::Ssse3);
}

#[test]
fn avx2_raid_matches_core() {
    check_raid_matches(Level::Avx2);
}

#[test]
fn gfni_raid_matches_core() {
    check_raid_matches(Level::Gfni);
}

#[test]
fn census_counts_accel_bytes() {
    let Some(simd) = kernels_at(Level::Ssse3).or_else(|| kernels_at(Level::Avx2)) else {
        return;
    };
    let before = simd.census.load(core::sync::atomic::Ordering::Relaxed);
    let gftbls = tables::init_tables(&[7u8, 9]);
    let data = vec![vec![1u8; 100]; 2];
    let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
    let mut out = vec![vec![0u8; 100]; 1];
    let mut or: Vec<&mut [u8]> = out.iter_mut().map(|x| x.as_mut_slice()).collect();
    (simd.encode)(&gftbls, &data_refs, &mut or);
    let after = simd.census.load(core::sync::atomic::Ordering::Relaxed);
    assert!(after >= before + 200, "census must count source bytes");
}

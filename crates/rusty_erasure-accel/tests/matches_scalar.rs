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

const EDGE_LENS: &[usize] =
    &[0, 1, 15, 16, 17, 31, 32, 33, 47, 63, 64, 65, 96, 255, 256, 1024, 4113];

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
    let Some(simd) = level_or_skip(level) else { return };
    let scalar = Kernels::scalar();
    let mut rng = Rng(0x4C0D_E001 ^ level as u64);
    for &len in EDGE_LENS {
        for &(k, rows) in
            &[(1usize, 1usize), (2, 3), (4, 4), (5, 5), (10, 4), (10, 9), (16, 8), (32, 8)]
        {
            let coeffs = rng.bytes(rows * k);
            let gftbls = tables::init_tables(&coeffs);
            let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();
            let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();

            let mut a = vec![vec![0u8; len]; rows];
            let mut b = vec![vec![0xAAu8; len]; rows]; // different init: encode must overwrite
            {
                let mut ar: Vec<&mut [u8]> = a.iter_mut().map(|x| x.as_mut_slice()).collect();
                (scalar.encode)(&gftbls, &data_refs, &mut ar);
            }
            {
                let mut br: Vec<&mut [u8]> = b.iter_mut().map(|x| x.as_mut_slice()).collect();
                (simd.encode)(&gftbls, &data_refs, &mut br);
            }
            assert_eq!(a, b, "{level:?} encode k={k} rows={rows} len={len}");
        }
    }
}

fn check_mad_matches(level: Level) {
    let Some(simd) = level_or_skip(level) else { return };
    let scalar = Kernels::scalar();
    let mut rng = Rng(0x4C0D_E002 ^ level as u64);
    for &len in EDGE_LENS {
        for _ in 0..4 {
            let c = rng.next() as u8;
            let mut tbl = [0u8; 32];
            tables::mul_table32(c, &mut tbl);
            let src = rng.bytes(len);
            let base = rng.bytes(len);
            let mut a = base.clone();
            let mut b = base;
            (scalar.mad)(&tbl, &src, &mut a);
            (simd.mad)(&tbl, &src, &mut b);
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

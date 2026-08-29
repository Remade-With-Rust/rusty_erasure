//! The portable oracle gate: whatever kernel set `accel::kernels()` returns
//! on THIS architecture must be byte-identical to the scalar oracle. On
//! x86-64 this duplicates the per-level gates; on the arm64 and wasm CI jobs
//! it IS the gate (M6).

use rusty_erasure_core::kernel::Kernels;

/// Lengths that straddle every vector width, unroll tier and tail.
///
/// Miri is ~100x slower than native, and this gate is the one that actually
/// executes the SIMD kernels under it, so the long lengths are dropped there.
/// Nothing is lost: 0..=96 already crosses every 16/32/64/128-byte boundary
/// the kernels tier on, and the big values only repeat those crossings with
/// more iterations. Same pattern the ported ISA-L suite already uses.
const ENCODE_LENS: &[usize] = if cfg!(miri) {
    &[0, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 96]
} else {
    &[
        0, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 96, 255, 1024, 4113,
    ]
};

const RAID_LENS: &[usize] = if cfg!(miri) {
    &[0, 1, 7, 8, 15, 16, 31, 32, 33, 63, 64, 65, 96]
} else {
    &[
        0, 1, 7, 8, 15, 16, 31, 32, 33, 63, 64, 65, 96, 255, 1024, 4113,
    ]
};

/// Refuse to pass vacuously where a SIMD set is expected.
///
/// These tests skip when `kernels()` returns `None`, which is correct on a
/// wasm build without `+simd128` — and is exactly how the Miri gate stayed
/// green for weeks without ever executing a kernel: under Miri
/// `is_x86_feature_detected!` returns false, so the skip fired silently.
///
/// A caller that KNOWS acceleration should be available sets
/// `RUSTY_ERASURE_REQUIRE_ACCEL=1` and the skip becomes a failure. Reading the
/// log for a reach line would not be enough: the scalar-fallback run of this
/// same test takes 97 s under Miri and the real one 104 s, so neither the
/// duration nor a passing result distinguishes them. The check has to be
/// mechanical.
fn require_accel_or_skip(set: Option<rusty_erasure_core::kernel::Kernels>) -> Option<Kernels> {
    if set.is_none() && std::env::var_os("RUSTY_ERASURE_REQUIRE_ACCEL").is_some() {
        panic!(
            "RUSTY_ERASURE_REQUIRE_ACCEL is set but accel::kernels() returned None — \
             this gate would have passed WITHOUT executing any SIMD kernel. Check the \
             target features the build was given."
        );
    }
    set
}

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

#[test]
fn best_arch_kernels_match_scalar() {
    let Some(simd) = require_accel_or_skip(rusty_erasure_accel::kernels()) else {
        eprintln!(
            "no accel set on this arch/build (e.g. wasm without +simd128) — scalar is the path"
        );
        return;
    };
    eprintln!("testing {}", simd.name);
    let census_before = simd.census.load(core::sync::atomic::Ordering::Relaxed);
    let scalar = Kernels::scalar();
    let mut rng = Rng(0x9047_AB1E);
    for &len in ENCODE_LENS {
        for &(k, rows) in &[(1usize, 1usize), (2, 3), (4, 4), (5, 5), (10, 4), (16, 8)] {
            let coeffs = rng.bytes(rows * k);
            let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();
            let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
            let mut a = vec![vec![0u8; len]; rows];
            let mut b = vec![vec![0xAAu8; len]; rows];
            {
                let g = (scalar.init)(&coeffs);
                let mut ar: Vec<&mut [u8]> = a.iter_mut().map(|x| x.as_mut_slice()).collect();
                (scalar.encode)(&g, &data_refs, &mut ar);
            }
            {
                let g = (simd.init)(&coeffs);
                let mut br: Vec<&mut [u8]> = b.iter_mut().map(|x| x.as_mut_slice()).collect();
                (simd.encode)(&g, &data_refs, &mut br);
            }
            assert_eq!(a, b, "{} encode k={k} rows={rows} len={len}", simd.name);

            // mad on the first coefficient.
            let sg = (scalar.init)(&coeffs[..1]);
            let vg = (simd.init)(&coeffs[..1]);
            let src = rng.bytes(len);
            let base = rng.bytes(len);
            let mut ma = base.clone();
            let mut mb = base;
            (scalar.mad)(&sg, &src, &mut ma);
            (simd.mad)(&vg, &src, &mut mb);
            assert_eq!(ma, mb, "{} mad len={len}", simd.name);

            // Fused update: fold source 0 into every row, both sets, from the
            // same encoded starting state.
            if k >= 1 && rows >= 1 && len > 0 {
                let sg = (scalar.init)(&coeffs);
                let vg = (simd.init)(&coeffs);
                let mut ua = a.clone();
                let mut ub = a.clone();
                {
                    let mut ur: Vec<&mut [u8]> = ua.iter_mut().map(|x| x.as_mut_slice()).collect();
                    (scalar.update)(&sg, k, 0, data_refs[0], &mut ur);
                }
                {
                    let mut ur: Vec<&mut [u8]> = ub.iter_mut().map(|x| x.as_mut_slice()).collect();
                    (simd.update)(&vg, k, 0, data_refs[0], &mut ur);
                }
                assert_eq!(ua, ub, "{} update k={k} rows={rows} len={len}", simd.name);
            }
        }
    }
    // The per-arch reach census: this arch's SIMD set must have counted the
    // bytes it just processed (an uncounted kernel is invisible to the
    // shipping census — the rusty_zstd law).
    let census_after = simd.census.load(core::sync::atomic::Ordering::Relaxed);
    assert!(
        census_after > census_before,
        "{}: census did not advance",
        simd.name
    );
}

/// The dispatched RAID pair on this arch must be byte-identical to the core
/// scalar raid module — the same oracle law, applied to xor_gen/pq_gen.
#[test]
fn arch_raid_kernels_match_core() {
    let raid = rusty_erasure_accel::raid_kernels();
    if raid.is_none() && std::env::var_os("RUSTY_ERASURE_REQUIRE_ACCEL").is_some() {
        panic!(
            "RUSTY_ERASURE_REQUIRE_ACCEL is set but accel::raid_kernels() returned None — \
             this gate would have passed WITHOUT executing any RAID kernel."
        );
    }
    let Some((xor, pq)) = raid else {
        eprintln!("no accel RAID pair on this arch/build — the core path is the path");
        return;
    };
    let mut rng = Rng(0x7A1D_5EED);
    for &len in RAID_LENS {
        for &n in &[2usize, 3, 5, 8, 17] {
            let sources: Vec<Vec<u8>> = (0..n).map(|_| rng.bytes(len)).collect();
            let refs: Vec<&[u8]> = sources.iter().map(|s| s.as_slice()).collect();

            let mut want_x = vec![0u8; len];
            rusty_erasure_core::raid::xor_gen(&refs, &mut want_x).expect("core xor");
            let mut got_x = vec![0xAAu8; len];
            xor(&refs, &mut got_x);
            assert_eq!(want_x, got_x, "raid xor n={n} len={len}");

            let mut want_p = vec![0u8; len];
            let mut want_q = vec![0u8; len];
            rusty_erasure_core::raid::pq_gen(&refs, &mut want_p, &mut want_q).expect("core pq");
            let mut got_p = vec![0xAAu8; len];
            let mut got_q = vec![0x55u8; len];
            pq(&refs, &mut got_p, &mut got_q);
            assert_eq!(want_p, got_p, "raid p n={n} len={len}");
            assert_eq!(want_q, got_q, "raid q n={n} len={len}");
        }
    }
}

//! The portable oracle gate: whatever kernel set `accel::kernels()` returns
//! on THIS architecture must be byte-identical to the scalar oracle. On
//! x86-64 this duplicates the per-level gates; on the arm64 and wasm CI jobs
//! it IS the gate (M6).

use rusty_erasure_core::kernel::Kernels;

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
    let Some(simd) = rusty_erasure_accel::kernels() else {
        eprintln!("no accel set on this arch/build (e.g. wasm without +simd128) — scalar is the path");
        return;
    };
    eprintln!("testing {}", simd.name);
    let census_before = simd.census.load(core::sync::atomic::Ordering::Relaxed);
    let scalar = Kernels::scalar();
    let mut rng = Rng(0x9047_AB1E);
    for &len in &[0usize, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 96, 255, 1024, 4113] {
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
        }
    }
    // The per-arch reach census: this arch's SIMD set must have counted the
    // bytes it just processed (an uncounted kernel is invisible to the
    // shipping census — the rusty_zstd law).
    let census_after = simd.census.load(core::sync::atomic::Ordering::Relaxed);
    assert!(census_after > census_before, "{}: census did not advance", simd.name);
}

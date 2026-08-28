//! Field axioms, matrix invariants, and deterministic no-panic sweeps.
//! Everything here is seeded — the same run on any machine — and paired with
//! the cargo-fuzz targets in `fuzz/`, which explore the same surfaces
//! coverage-guided.

use rusty_erasure_core::{Matrix, MatrixError, gf};

/// splitmix64 — tiny deterministic RNG, no dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// A random `k`-subset of `0..m`, ascending.
    fn subset(&mut self, k: usize, m: usize) -> Vec<usize> {
        let mut all: Vec<usize> = (0..m).collect();
        for i in 0..k {
            let j = i + self.below(m - i);
            all.swap(i, j);
        }
        let mut pick = all[..k].to_vec();
        pick.sort_unstable();
        pick
    }
}

#[test]
fn every_nonzero_element_has_a_true_inverse() {
    for a in 1..=255u8 {
        assert_eq!(gf::mul(a, gf::inv(a)), 1, "a * inv(a) for a={a}");
    }
    assert_eq!(gf::inv(0), 0, "ISA-L behavior: inv(0) == 0");
}

/// Sweep sizes shrink under Miri (interpreter speed): every code path still
/// runs; the exhaustive/native sizes are the real gates.
const fn miri_scaled(native: usize, miri: usize) -> usize {
    if cfg!(miri) { miri } else { native }
}

#[test]
fn multiplication_is_commutative_and_distributive_exhaustively() {
    let step = miri_scaled(1, 37);
    for a in (0..=255usize).step_by(step) {
        let a = a as u8;
        for b in 0..=255u8 {
            assert_eq!(gf::mul(a, b), gf::mul(b, a));
            for c in [0u8, 1, 2, 0x1d, 0x80, 0xff, a ^ b] {
                assert_eq!(
                    gf::mul(a, b ^ c),
                    gf::mul(a, b) ^ gf::mul(a, c),
                    "distributivity a={a} b={b} c={c}"
                );
            }
        }
    }
}

#[test]
fn multiplication_is_associative_on_a_dense_sample() {
    let mut rng = Rng(0xE7A5_0001);
    for _ in 0..miri_scaled(200_000, 2_000) {
        let (a, b, c) = (rng.next() as u8, rng.next() as u8, rng.next() as u8);
        assert_eq!(
            gf::mul(gf::mul(a, b), c),
            gf::mul(a, gf::mul(b, c)),
            "associativity a={a} b={b} c={c}"
        );
    }
}

#[test]
fn cauchy_every_sampled_recovery_submatrix_inverts() {
    // The property Cauchy exists to guarantee: ANY k surviving rows form an
    // invertible decode matrix. Sampled across the corpus config grid.
    let mut rng = Rng(0xCAFE_0002);
    let configs: &[(usize, usize)] =
        if cfg!(miri) { &[(4, 2), (8, 2)] } else { &[(4, 2), (8, 2), (10, 4), (16, 4), (20, 8), (32, 8), (64, 8)] };
    for &(k, p) in configs {
        let m = Matrix::cauchy(k, p).unwrap();
        for round in 0..miri_scaled(200, 3) {
            let rows = rng.subset(k, k + p);
            let sub = m.select_rows(&rows).unwrap();
            let inv = sub
                .invert()
                .unwrap_or_else(|e| panic!("k={k} p={p} round={round} rows={rows:?}: {e}"));
            assert!(sub.multiply(&inv).unwrap().is_identity(), "k={k} p={p} rows={rows:?}");
        }
    }
}

#[test]
fn vandermonde_safe_region_sampled_submatrices_invert() {
    // Inside the documented safe region the same property must hold — that is
    // what "safe" means.
    let mut rng = Rng(0xBEEF_0003);
    let configs: &[(usize, usize)] =
        if cfg!(miri) { &[(5, 5), (10, 4)] } else { &[(3, 20), (4, 21), (5, 5), (10, 4), (21, 4), (15, 3)] };
    for &(k, p) in configs {
        let m = Matrix::reed_solomon(k, p).unwrap();
        for round in 0..miri_scaled(200, 3) {
            let rows = rng.subset(k, k + p);
            let sub = m.select_rows(&rows).unwrap();
            let inv = sub
                .invert()
                .unwrap_or_else(|e| panic!("k={k} p={p} round={round} rows={rows:?}: {e}"));
            assert!(sub.multiply(&inv).unwrap().is_identity(), "k={k} p={p} rows={rows:?}");
        }
    }
}

#[test]
fn singular_matrices_report_singular_not_garbage() {
    // Duplicate rows.
    let m = Matrix::from_bytes(2, 2, vec![3, 7, 3, 7]).unwrap();
    assert_eq!(m.invert().unwrap_err(), MatrixError::Singular);
    // All zeros.
    let z = Matrix::from_bytes(3, 3, vec![0; 9]).unwrap();
    assert_eq!(z.invert().unwrap_err(), MatrixError::Singular);
    // A row that is the XOR of two others (GF(2^8) linear dependence).
    let m = Matrix::from_bytes(3, 3, vec![1, 2, 3, 4, 5, 6, 1 ^ 4, 2 ^ 5, 3 ^ 6]).unwrap();
    assert_eq!(m.invert().unwrap_err(), MatrixError::Singular);
}

#[test]
fn random_square_matrices_invert_or_report_singular_never_panic() {
    let mut rng = Rng(0x5EED_0004);
    let mut inverted = 0u32;
    let rounds = miri_scaled(500, 20);
    for _ in 0..rounds {
        let n = 1 + rng.below(miri_scaled(32, 8));
        let data: Vec<u8> = (0..n * n).map(|_| rng.next() as u8).collect();
        let m = Matrix::from_bytes(n, n, data).unwrap();
        if let Ok(inv) = m.invert() {
            inverted += 1;
            assert!(m.multiply(&inv).unwrap().is_identity(), "n={n}");
        }
    }
    // Random matrices over GF(2^8) are overwhelmingly invertible; if this is
    // ever low the test went vacuous, not the math wrong.
    assert!(inverted as usize > rounds * 4 / 5, "only {inverted}/{rounds} inverted — probe broken?");
}

#[test]
fn dimension_sweep_never_panics_and_errors_are_typed() {
    for k in 0..=40usize {
        for p in 0..=12usize {
            let rs = Matrix::reed_solomon(k, p);
            let cy = Matrix::cauchy(k, p);
            if k == 0 || p == 0 {
                assert!(matches!(rs, Err(MatrixError::Dimensions { .. })), "rs k={k} p={p}");
                assert!(matches!(cy, Err(MatrixError::Dimensions { .. })), "cauchy k={k} p={p}");
            } else {
                assert!(cy.is_ok(), "cauchy k={k} p={p}");
            }
        }
    }
    // Field-size limit: k + p > 255 refused, boundary accepted.
    assert!(matches!(Matrix::cauchy(250, 10), Err(MatrixError::Dimensions { .. })));
    assert!(Matrix::cauchy(247, 8).is_ok());
    assert!(matches!(Matrix::cauchy(usize::MAX, 1), Err(MatrixError::Dimensions { .. })));
}

#[test]
fn misuse_is_an_error_never_a_panic() {
    let m = Matrix::cauchy(4, 2).unwrap();
    assert!(m.select_rows(&[0, 1, 2, 6]).is_err(), "row index == rows is out of range");
    assert!(m.select_rows(&[]).is_err());
    assert!(m.invert().is_err(), "non-square invert is a dimension error");
    assert!(Matrix::from_bytes(2, 2, vec![0; 3]).is_err(), "length mismatch");
    assert!(Matrix::from_bytes(0, 2, vec![]).is_err());
    assert!(Matrix::from_bytes(300, 1, vec![0; 300]).is_err(), "dimension over 255");
    let a = Matrix::cauchy(3, 2).unwrap();
    assert!(m.multiply(&a).is_err(), "inner-dimension mismatch");
}

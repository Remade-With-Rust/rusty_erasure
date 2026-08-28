//! Conformance gates against ISA-L v2.32.1's own GF(2^8) tables, checked in
//! as golden data (`corpus/golden/`, provenance in `PROVENANCE.md`).
//!
//! These tests are exhaustive and deterministic — one run is a verdict, on any
//! machine, at any load (codec-measurement: counts before times). Together
//! they pin the field polynomial by proof: every one of the 65,536 products
//! and 256 inverses must match the reference byte-for-byte.

use rusty_erasure_core::{gf, matrix::Matrix, tables};

static GMUL: &[u8; 65536] = include_bytes!("../../../corpus/golden/gf_mul_table_base.bin");
static GINV: &[u8; 256] = include_bytes!("../../../corpus/golden/gf_inv_table_base.bin");
static GFF: &[u8; 256] = include_bytes!("../../../corpus/golden/gff_base.bin");
static GLOG: &[u8; 256] = include_bytes!("../../../corpus/golden/gflog_base.bin");

/// ISA-L's table is indexed `gf_mul_table_base[b * 256 + a]`.
fn isal_mul(a: u8, b: u8) -> u8 {
    GMUL[(b as usize) * 256 + a as usize]
}

#[test]
fn mul_matches_isal_for_all_65536_pairs() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            assert_eq!(gf::mul(a, b), isal_mul(a, b), "mul({a}, {b})");
        }
    }
}

#[test]
fn mul_shift_matches_isal_for_all_65536_pairs() {
    // The table-free construction must agree with the reference independently
    // of the const tables — two derivations, one truth.
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            assert_eq!(gf::mul_shift(a, b), isal_mul(a, b), "mul_shift({a}, {b})");
        }
    }
}

#[test]
fn inv_matches_isal_for_all_256_values() {
    // Includes ISA-L's documented quirk: inv(0) == 0.
    for a in 0..=255u8 {
        assert_eq!(gf::inv(a), GINV[a as usize], "inv({a})");
    }
}

#[test]
fn generator_powers_match_isal_exp_table() {
    let mut x: u8 = 1;
    for i in 0..255 {
        assert_eq!(x, GFF[i], "2^{i}");
        x = gf::mul(x, 2);
    }
    assert_eq!(x, 1, "generator order must be 255");
}

#[test]
fn isal_log_table_is_inverse_of_exp_table() {
    for a in 1..=255u8 {
        assert_eq!(GFF[GLOG[a as usize] as usize], a, "exp(log({a}))");
    }
}

#[test]
fn rs_matrix_matches_reference_construction() {
    // Independent transcription of gf_gen_rs_matrix, multiplied through the
    // GOLDEN table (never our own gf), for safe-region configs.
    for &(k, p) in &[(2usize, 2usize), (3, 30), (4, 21), (5, 5), (10, 4), (21, 4), (200, 3)] {
        let m = k + p;
        let mut expect = vec![0u8; m * k];
        for i in 0..k {
            expect[k * i + i] = 1;
        }
        let mut row_gen: u8 = 1;
        for i in k..m {
            let mut coeff: u8 = 1;
            for j in 0..k {
                expect[k * i + j] = coeff;
                coeff = isal_mul(coeff, row_gen);
            }
            row_gen = isal_mul(row_gen, 2);
        }
        let ours = Matrix::reed_solomon(k, p).expect("safe-region config must generate");
        assert_eq!(ours.as_bytes(), &expect[..], "rs matrix k={k} p={p}");
    }
}

#[test]
fn cauchy_matrix_matches_reference_construction() {
    for &(k, p) in &[(2usize, 2usize), (10, 4), (17, 3), (32, 8), (247, 8)] {
        let m = k + p;
        let mut expect = vec![0u8; m * k];
        for i in 0..k {
            expect[k * i + i] = 1;
        }
        for i in k..m {
            for j in 0..k {
                expect[k * i + j] = GINV[i ^ j];
            }
        }
        let ours = Matrix::cauchy(k, p).expect("cauchy must generate");
        assert_eq!(ours.as_bytes(), &expect[..], "cauchy matrix k={k} p={p}");
    }
}

#[test]
fn expanded_tables_reproduce_every_product() {
    // The 32-byte nibble-table layout: tbl[j] = c*j, tbl[16+j] = c*(j<<4), and
    // table_mul recombines to c*x — proven for every (c, x) against the golden
    // table. This is the contract the SIMD kernels (M4) rest on.
    let mut tbl = [0u8; tables::TABLE_BYTES];
    for c in 0..=255u8 {
        tables::mul_table32(c, &mut tbl);
        for j in 0..16u8 {
            assert_eq!(tbl[j as usize], isal_mul(c, j), "tbl low c={c} j={j}");
            assert_eq!(tbl[16 + j as usize], isal_mul(c, j << 4), "tbl high c={c} j={j}");
        }
        for x in 0..=255u8 {
            assert_eq!(tables::table_mul(&tbl, x), isal_mul(c, x), "table_mul c={c} x={x}");
        }
    }
}

#[test]
fn init_tables_expands_row_major_like_isal() {
    let m = Matrix::cauchy(4, 2).unwrap();
    let g = tables::init_tables(m.parity_bytes());
    assert_eq!(g.len(), 2 * 4 * tables::TABLE_BYTES);
    // ISA-L's ec_encode_data_base reads the coefficient back at
    // v[j*32 + l*srcs*32 + 1] — table byte 1 is the coefficient itself.
    for l in 0..2 {
        for j in 0..4 {
            assert_eq!(
                g[j * 32 + l * 4 * 32 + 1],
                m.get(4 + l, j).unwrap(),
                "coefficient position l={l} j={j}"
            );
        }
    }
}

#[test]
fn affine_matrices_match_isal_gfni_table() {
    // ISA-L's gf_table_gfni: 256 precomputed GF2P8AFFINEQB matrices, stored
    // as little-endian u64s in the golden data. Our const-generated table
    // must match every one — pinning the bit convention by proof.
    static GFNI: &[u8; 2048] = include_bytes!("../../../corpus/golden/gf_table_gfni.bin");
    for c in 0..256usize {
        let want = u64::from_le_bytes(GFNI[c * 8..(c + 1) * 8].try_into().expect("8 bytes"));
        assert_eq!(tables::AFFINE[c], want, "affine matrix for c={c}");
    }
}

#[test]
fn vandermonde_safe_region_is_enforced() {
    // Inside the documented region: accepted.
    for &(k, p) in &[(1usize, 200usize), (3, 100), (4, 21), (5, 5), (21, 4), (10, 4), (200, 3), (252, 3)] {
        assert!(Matrix::reed_solomon(k, p).is_ok(), "k={k} p={p} should be safe");
    }
    // Outside it: refused with the typed error.
    for &(k, p) in &[(4usize, 22usize), (5, 6), (6, 5), (22, 4), (10, 5), (100, 8)] {
        assert!(
            matches!(
                Matrix::reed_solomon(k, p),
                Err(rusty_erasure_core::MatrixError::VandermondeUnsafe { .. })
            ),
            "k={k} p={p} should be refused"
        );
    }
}

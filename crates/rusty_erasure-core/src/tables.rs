//! Expanded multiplication tables — ISA-L's `ec_init_tables` layout,
//! bit-identical: 32 bytes per coefficient `c`, being the two PSHUFB nibble
//! tables `{c*0, c*1, ..., c*15}` then `{c*0x00, c*0x10, ..., c*0xf0}`.
//!
//! Any product then decomposes as `c*x = tbl[x & 0xf] ^ tbl[16 + (x >> 4)]` —
//! the identity the SIMD kernels (M4) and their scalar twins (M2) both rest
//! on, and which the tests prove for every coefficient and every byte.

use alloc::vec::Vec;

use crate::gf;

/// Bytes per expanded coefficient (ISA-L's contract).
pub const TABLE_BYTES: usize = 32;

/// Expand one coefficient into its 32-byte nibble table.
pub fn mul_table32(c: u8, out: &mut [u8; TABLE_BYTES]) {
    for j in 0u8..16 {
        out[j as usize] = gf::mul(c, j);
        out[16 + j as usize] = gf::mul(c, j << 4);
    }
}

/// Expand a row-major coefficient block (ISA-L's `ec_init_tables`: iterate
/// rows, then columns) into `coeffs.len() * 32` bytes. For encoding, pass the
/// parity block (`Matrix::parity_bytes`), matching ISA-L's convention of
/// calling `ec_init_tables(k, rows, &a[k*k], gftbls)`.
pub fn init_tables(coeffs: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(coeffs.len() * TABLE_BYTES);
    let mut tbl = [0u8; TABLE_BYTES];
    for &c in coeffs {
        mul_table32(c, &mut tbl);
        out.extend_from_slice(&tbl);
    }
    out
}

/// Multiply via an expanded table: `c*x` from `c`'s 32-byte table. The scalar
/// mirror of what the SIMD kernels do per lane; tests pin it against
/// [`gf::mul`] for every `(c, x)`.
#[inline]
pub fn table_mul(tbl: &[u8; TABLE_BYTES], x: u8) -> u8 {
    tbl[(x & 0x0f) as usize] ^ tbl[16 + (x >> 4) as usize]
}

/// Bytes per expanded GFNI coefficient: one `GF2P8AFFINEQB` 8×8 bit-matrix.
pub const GFNI_TABLE_BYTES: usize = 8;

/// Build the `GF2P8AFFINEQB` matrix computing multiply-by-`c`.
///
/// Multiplication by a constant is GF(2)-linear, so it is exactly an 8×8 bit
/// matrix: column `i` is `c·2^i`. Instruction layout (Intel SDM): output bit
/// `j` of a byte = parity(matrix byte `7−j` AND input byte), so qword byte
/// `7−j` holds row `j`, whose bit `i` is bit `j` of `c·2^i`. Conformance
/// tests pin all 256 matrices against ISA-L's own `gf_table_gfni` golden data.
const fn build_affine(c: u8) -> u64 {
    let mut m = 0u64;
    let mut j = 0;
    while j < 8 {
        let mut row = 0u8;
        let mut i = 0;
        while i < 8 {
            if crate::gf::mul(c, 1 << i) & (1 << j) != 0 {
                row |= 1 << i;
            }
            i += 1;
        }
        m |= (row as u64) << (8 * (7 - j));
        j += 1;
    }
    m
}

const fn build_affine_table() -> [u64; 256] {
    let mut t = [0u64; 256];
    let mut c = 0usize;
    while c < 256 {
        t[c] = build_affine(c as u8);
        c += 1;
    }
    t
}

/// All 256 multiply-by-`c` affine matrices, const-generated at compile time.
pub const AFFINE: [u64; 256] = build_affine_table();

/// Expand a row-major coefficient block into GFNI affine tables — 8
/// little-endian bytes per coefficient, the layout ISA-L's GFNI kernels use.
pub fn init_tables_gfni(coeffs: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(coeffs.len() * GFNI_TABLE_BYTES);
    for &c in coeffs {
        out.extend_from_slice(&AFFINE[c as usize].to_le_bytes());
    }
    out
}

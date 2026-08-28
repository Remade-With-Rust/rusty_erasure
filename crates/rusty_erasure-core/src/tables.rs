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

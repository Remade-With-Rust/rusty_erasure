//! Scalar erasure-coding kernels — the permanent oracles every SIMD twin (M4)
//! is gated against, and the fallback on every CPU.
//!
//! All kernels consume ISA-L's expanded 32-byte nibble tables
//! (`tables::init_tables` / `tables::mul_table32`), exactly as the vector
//! kernels will: per byte, `c*x = tbl[x & 0xf] ^ tbl[16 + (x >> 4)]`. Length
//! agreement is validated — a mismatch is a typed error, never a silent
//! truncation and never a panic.

use crate::error::CodeError;
use crate::tables::{TABLE_BYTES, table_mul};

fn tbl32(gftbls: &[u8], index: usize) -> Result<&[u8; TABLE_BYTES], CodeError> {
    let start = index * TABLE_BYTES;
    let slice = gftbls
        .get(start..start + TABLE_BYTES)
        .ok_or(CodeError::ShardCount { expected: index + 1, got: gftbls.len() / TABLE_BYTES })?;
    // Infallible: the slice is exactly TABLE_BYTES long.
    Ok(slice.try_into().expect("length checked above"))
}

/// `dest = c · src` where `tbl` is `c`'s expanded 32-byte table
/// (ISA-L's `gf_vect_mul`, without its len-multiple-of-32 restriction).
pub fn vect_mul(dest: &mut [u8], tbl: &[u8; TABLE_BYTES], src: &[u8]) -> Result<(), CodeError> {
    if dest.len() != src.len() {
        return Err(CodeError::ShardLength { index: 0, expected: dest.len(), got: src.len() });
    }
    for (d, &s) in dest.iter_mut().zip(src) {
        *d = table_mul(tbl, s);
    }
    Ok(())
}

/// `dest ^= c · src` — multiply-and-add, the incremental-update primitive
/// (ISA-L's `gf_vect_mad`).
pub fn vect_mad(dest: &mut [u8], tbl: &[u8; TABLE_BYTES], src: &[u8]) -> Result<(), CodeError> {
    if dest.len() != src.len() {
        return Err(CodeError::ShardLength { index: 0, expected: dest.len(), got: src.len() });
    }
    for (d, &s) in dest.iter_mut().zip(src) {
        *d ^= table_mul(tbl, s);
    }
    Ok(())
}

/// `dest[i] = XOR_j (c_j · srcs[j][i])` — the GF(2^8) dot product at the heart
/// of encode (ISA-L's `gf_vect_dot_prod`). `gftbls` holds one 32-byte table
/// per source, in source order.
pub fn vect_dot_prod(dest: &mut [u8], gftbls: &[u8], srcs: &[&[u8]]) -> Result<(), CodeError> {
    if gftbls.len() != srcs.len() * TABLE_BYTES {
        return Err(CodeError::ShardCount {
            expected: srcs.len(),
            got: gftbls.len() / TABLE_BYTES,
        });
    }
    for (index, src) in srcs.iter().enumerate() {
        if src.len() != dest.len() {
            return Err(CodeError::ShardLength { index, expected: dest.len(), got: src.len() });
        }
    }
    dest.fill(0);
    for (j, src) in srcs.iter().enumerate() {
        let tbl = tbl32(gftbls, j)?;
        for (d, &s) in dest.iter_mut().zip(*src) {
            *d ^= table_mul(tbl, s);
        }
    }
    Ok(())
}

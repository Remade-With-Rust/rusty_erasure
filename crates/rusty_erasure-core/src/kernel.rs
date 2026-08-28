//! Scalar erasure-coding kernels — the permanent oracles every SIMD twin (M4)
//! is gated against, and the fallback on every CPU.
//!
//! All kernels consume ISA-L's expanded 32-byte nibble tables
//! (`tables::init_tables` / `tables::mul_table32`), exactly as the vector
//! kernels will: per byte, `c*x = tbl[x & 0xf] ^ tbl[16 + (x >> 4)]`. Length
//! agreement is validated — a mismatch is a typed error, never a silent
//! truncation and never a panic.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::CodeError;
use crate::tables::{TABLE_BYTES, table_mul};

/// Census counter for the scalar kernel set: source bytes processed. The
/// reach census (mission plan §7.1) is always on — one relaxed add per call,
/// never per element — so "which kernels does production actually run" is a
/// measured fact, not an assumption.
pub static SCALAR_CENSUS_BYTES: AtomicU64 = AtomicU64::new(0);

/// A pluggable kernel set. The choice is made ONCE, at coder construction or
/// the facade surface (never inside a loop — the dispatch-placement law), and
/// core stays free of all detection machinery: this crate only ever provides
/// [`Kernels::scalar`]; SIMD sets come from `rusty_erasure-accel` via the
/// facade.
///
/// Contract for implementations: the CALLER has validated everything —
/// `data` slices all equal length, `out` slices all equal that length,
/// `gftbls.len() == out.len() * data.len() * 32`. Implementations must be
/// exact (byte-identical to the scalar set) and must add the source bytes
/// they process to their census counter.
#[derive(Clone, Copy, Debug)]
pub struct Kernels {
    /// Full encode: `out[l] = XOR_j (c[l][j] · data[j])` for every output row,
    /// tables in ISA-L layout (row-major, 32 bytes per coefficient).
    pub encode: fn(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]),
    /// `dest ^= c · src` for one expanded table (the update path).
    pub mad: fn(tbl: &[u8; TABLE_BYTES], src: &[u8], dest: &mut [u8]),
    /// Kernel-set name, for reporting.
    pub name: &'static str,
    /// The census counter this set accumulates into.
    pub census: &'static AtomicU64,
}

impl Kernels {
    /// The scalar set — the permanent oracle and the fallback on every CPU.
    pub const fn scalar() -> Self {
        Self {
            encode: scalar_encode,
            mad: scalar_mad,
            name: "scalar",
            census: &SCALAR_CENSUS_BYTES,
        }
    }
}

fn scalar_encode(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
    let k = data.len();
    let len = out.first().map_or(0, |b| b.len());
    SCALAR_CENSUS_BYTES.fetch_add((k * len) as u64, Ordering::Relaxed);
    for (l, dest) in out.iter_mut().enumerate() {
        dest.fill(0);
        for (j, src) in data.iter().enumerate() {
            let start = (l * k + j) * TABLE_BYTES;
            let tbl: &[u8; TABLE_BYTES] =
                gftbls[start..start + TABLE_BYTES].try_into().expect("caller-validated tables");
            for (d, &s) in dest.iter_mut().zip(*src) {
                *d ^= table_mul(tbl, s);
            }
        }
    }
}

fn scalar_mad(tbl: &[u8; TABLE_BYTES], src: &[u8], dest: &mut [u8]) {
    SCALAR_CENSUS_BYTES.fetch_add(src.len() as u64, Ordering::Relaxed);
    for (d, &s) in dest.iter_mut().zip(src) {
        *d ^= table_mul(tbl, s);
    }
}

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

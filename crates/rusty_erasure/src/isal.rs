//! The ISA-L compat layer: functions named and shaped like Intel ISA-L's
//! erasure-code API, so C callers and ISA-L's own tests port line-for-line.
//!
//! Semantics mirror ISA-L (including behaviors the typed API deliberately
//! tightens): `gf_gen_rs_matrix` fills for ANY `(m, k)` without the
//! safe-region refusal (ISA-L documents the unsafe region but generates
//! anyway; use [`crate::Matrix::reed_solomon`] for the enforced version),
//! `gf_invert_matrix` destroys its input, and `gf_vect_mul` keeps ISA-L's
//! len-multiple-of-32 contract. What does NOT carry over is the undefined
//! behavior: every function here validates its slices and returns a typed
//! error where ISA-L's documentation says "callers are responsible".
//!
//! Buffers may be longer than `len`; exactly `len` bytes are processed —
//! matching C pointer semantics safely.

use core::sync::atomic::Ordering;

use rusty_erasure_core::error::{CodeError, MatrixError};
use rusty_erasure_core::kernel::Kernels;
use rusty_erasure_core::kernel::SCALAR_CENSUS_BYTES;
use rusty_erasure_core::matrix::invert_gauss_jordan;
use rusty_erasure_core::tables::{TABLE_BYTES, mul_table32, table_mul};
use rusty_erasure_core::{gf, matrix};

/// The best kernel set consuming ISA-L NIBBLE-format tables — never GFNI,
/// whose tables are affine-format (see the module docs on format pairing).
fn nibble_kernels() -> Kernels {
    #[cfg(feature = "accel")]
    if let Some(k) = rusty_erasure_accel::kernels_nibble() {
        return k;
    }
    Kernels::scalar()
}

/// GF(2^8) multiply — ISA-L `gf_mul`.
pub const fn gf_mul(a: u8, b: u8) -> u8 {
    gf::mul(a, b)
}

/// GF(2^8) inverse — ISA-L `gf_inv` (returns 0 for 0).
pub const fn gf_inv(a: u8) -> u8 {
    gf::inv(a)
}

fn check_matrix_buf(a: &[u8], m: usize, k: usize) -> Result<(), MatrixError> {
    if k == 0 || m < k || !k.checked_mul(m).is_some_and(|need| a.len() >= need) {
        return Err(MatrixError::Dimensions {
            k,
            p: m.saturating_sub(k),
        });
    }
    Ok(())
}

/// Vandermonde-style encode matrix — ISA-L `gf_gen_rs_matrix`, verbatim:
/// identity in the top `k x k`, powers below, **no safe-region refusal**
/// (compat semantics; see the module docs). `a` needs at least `m * k` bytes.
pub fn gf_gen_rs_matrix(a: &mut [u8], m: usize, k: usize) -> Result<(), MatrixError> {
    check_matrix_buf(a, m, k)?;
    if m > 255 {
        return Err(MatrixError::Dimensions { k, p: m - k });
    }
    a[..m * k].fill(0);
    for i in 0..k {
        a[k * i + i] = 1;
    }
    let mut row_gen: u8 = 1;
    for i in k..m {
        let mut coeff: u8 = 1;
        for j in 0..k {
            a[k * i + j] = coeff;
            coeff = gf::mul(coeff, row_gen);
        }
        row_gen = gf::mul(row_gen, 2);
    }
    Ok(())
}

/// Cauchy encode matrix — ISA-L `gf_gen_cauchy1_matrix`.
pub fn gf_gen_cauchy1_matrix(a: &mut [u8], m: usize, k: usize) -> Result<(), MatrixError> {
    check_matrix_buf(a, m, k)?;
    if m > 255 {
        return Err(MatrixError::Dimensions { k, p: m - k });
    }
    a[..m * k].fill(0);
    for i in 0..k {
        a[k * i + i] = 1;
    }
    for i in k..m {
        for j in 0..k {
            a[k * i + j] = gf::inv((i ^ j) as u8);
        }
    }
    Ok(())
}

/// Invert an `n x n` matrix — ISA-L `gf_invert_matrix` semantics: `in_mat` is
/// DESTROYED, `out_mat` receives the inverse, singular input is a typed error
/// (where C returns -1).
pub fn gf_invert_matrix(
    in_mat: &mut [u8],
    out_mat: &mut [u8],
    n: usize,
) -> Result<(), MatrixError> {
    let nn = n
        .checked_mul(n)
        .filter(|&nn| in_mat.len() >= nn && out_mat.len() >= nn)
        .ok_or(MatrixError::Dimensions { k: n, p: 0 })?;
    invert_gauss_jordan(&mut in_mat[..nn], &mut out_mat[..nn], n)
}

/// Expand `rows * k` coefficients into `rows * k * 32` table bytes — ISA-L
/// `ec_init_tables`. Pass the parity block (`&a[k*k..]`), per their calling
/// convention.
pub fn ec_init_tables(k: usize, rows: usize, a: &[u8], gftbls: &mut [u8]) -> Result<(), CodeError> {
    let coeffs = k.checked_mul(rows).ok_or(CodeError::ShardCount {
        expected: rows,
        got: 0,
    })?;
    if a.len() < coeffs {
        return Err(CodeError::ShardCount {
            expected: coeffs,
            got: a.len(),
        });
    }
    let need = coeffs * TABLE_BYTES;
    if gftbls.len() < need {
        return Err(CodeError::ShardLength {
            index: 0,
            expected: need,
            got: gftbls.len(),
        });
    }
    let mut tbl = [0u8; TABLE_BYTES];
    for (i, &c) in a[..coeffs].iter().enumerate() {
        mul_table32(c, &mut tbl);
        gftbls[i * TABLE_BYTES..(i + 1) * TABLE_BYTES].copy_from_slice(&tbl);
    }
    Ok(())
}

fn tbl_at(gftbls: &[u8], index: usize) -> Result<&[u8; TABLE_BYTES], CodeError> {
    let start = index * TABLE_BYTES;
    gftbls
        .get(start..start + TABLE_BYTES)
        .and_then(|s| s.try_into().ok())
        .ok_or(CodeError::ShardCount {
            expected: index + 1,
            got: gftbls.len() / TABLE_BYTES,
        })
}

fn check_vects(len: usize, count: usize, bufs: &[&[u8]]) -> Result<(), CodeError> {
    if bufs.len() != count {
        return Err(CodeError::ShardCount {
            expected: count,
            got: bufs.len(),
        });
    }
    for (index, b) in bufs.iter().enumerate() {
        if b.len() < len {
            return Err(CodeError::ShardLength {
                index,
                expected: len,
                got: b.len(),
            });
        }
    }
    Ok(())
}

/// Encode `rows` parity vectors from `k` sources — ISA-L `ec_encode_data`.
/// `gftbls` is `rows * k * 32` bytes from [`ec_init_tables`].
pub fn ec_encode_data(
    len: usize,
    k: usize,
    rows: usize,
    gftbls: &[u8],
    data: &[&[u8]],
    coding: &mut [&mut [u8]],
) -> Result<(), CodeError> {
    check_vects(len, k, data)?;
    if coding.len() != rows {
        return Err(CodeError::ShardCount {
            expected: rows,
            got: coding.len(),
        });
    }
    for (index, b) in coding.iter().enumerate() {
        if b.len() < len {
            return Err(CodeError::ShardLength {
                index: k + index,
                expected: len,
                got: b.len(),
            });
        }
    }
    let need = rows * k * TABLE_BYTES;
    if gftbls.len() < need {
        return Err(CodeError::ShardLength {
            index: 0,
            expected: need,
            got: gftbls.len(),
        });
    }
    // Exact-length buffers (the common case) go through the dispatched
    // NIBBLE-format kernels — this function's `gftbls` contract IS the
    // ec_init_tables nibble layout, so the GFNI set (affine tables) must
    // never be used here (LEDGER M4's mixed-format lesson). Over-length
    // buffers take the trimmed scalar path below.
    if data.iter().all(|s| s.len() == len) && coding.iter().all(|c| c.len() == len) {
        (nibble_kernels().encode)(&gftbls[..need], data, coding);
        return Ok(());
    }
    SCALAR_CENSUS_BYTES.fetch_add((k * len) as u64, Ordering::Relaxed);
    for (l, out) in coding.iter_mut().enumerate() {
        let out = &mut out[..len];
        out.fill(0);
        for (j, src) in data.iter().enumerate() {
            let tbl = tbl_at(gftbls, l * k + j)?;
            for (d, &s) in out.iter_mut().zip(&src[..len]) {
                *d ^= table_mul(tbl, s);
            }
        }
    }
    Ok(())
}

/// Fold one source (index `vec_i`) into `rows` parity vectors — ISA-L
/// `ec_encode_data_update`.
pub fn ec_encode_data_update(
    len: usize,
    k: usize,
    rows: usize,
    vec_i: usize,
    gftbls: &[u8],
    data: &[u8],
    coding: &mut [&mut [u8]],
) -> Result<(), CodeError> {
    if vec_i >= k {
        return Err(CodeError::ShardIndex { index: vec_i, k });
    }
    if data.len() < len {
        return Err(CodeError::ShardLength {
            index: vec_i,
            expected: len,
            got: data.len(),
        });
    }
    if coding.len() != rows {
        return Err(CodeError::ShardCount {
            expected: rows,
            got: coding.len(),
        });
    }
    for (index, b) in coding.iter().enumerate() {
        if b.len() < len {
            return Err(CodeError::ShardLength {
                index: k + index,
                expected: len,
                got: b.len(),
            });
        }
    }
    if data.len() == len && coding.iter().all(|c| c.len() == len) {
        let need = ((rows.saturating_sub(1)) * k + vec_i + 1) * TABLE_BYTES;
        if gftbls.len() >= need {
            (nibble_kernels().update)(gftbls, k, vec_i, data, coding);
            return Ok(());
        }
    }
    SCALAR_CENSUS_BYTES.fetch_add(len as u64, Ordering::Relaxed);
    for (l, out) in coding.iter_mut().enumerate() {
        let tbl = tbl_at(gftbls, l * k + vec_i)?;
        for (d, &s) in out[..len].iter_mut().zip(&data[..len]) {
            *d ^= table_mul(tbl, s);
        }
    }
    Ok(())
}

/// GF(2^8) dot product across `vlen` sources — ISA-L `gf_vect_dot_prod`.
pub fn gf_vect_dot_prod(
    len: usize,
    vlen: usize,
    gftbls: &[u8],
    src: &[&[u8]],
    dest: &mut [u8],
) -> Result<(), CodeError> {
    check_vects(len, vlen, src)?;
    if dest.len() < len {
        return Err(CodeError::ShardLength {
            index: 0,
            expected: len,
            got: dest.len(),
        });
    }
    if gftbls.len() < vlen * TABLE_BYTES {
        return Err(CodeError::ShardCount {
            expected: vlen,
            got: gftbls.len() / TABLE_BYTES,
        });
    }
    if dest.len() == len && src.iter().all(|s| s.len() == len) {
        let mut out = [&mut dest[..len]];
        (nibble_kernels().encode)(&gftbls[..vlen * TABLE_BYTES], src, &mut out);
        return Ok(());
    }
    SCALAR_CENSUS_BYTES.fetch_add((vlen * len) as u64, Ordering::Relaxed);
    let dest = &mut dest[..len];
    dest.fill(0);
    for (j, s) in src.iter().enumerate() {
        let tbl = tbl_at(gftbls, j)?;
        for (d, &v) in dest.iter_mut().zip(&s[..len]) {
            *d ^= table_mul(tbl, v);
        }
    }
    Ok(())
}

/// Multiply-accumulate one source into `dest` — ISA-L `gf_vect_mad`.
/// `gftbls` holds `vec` tables; the one at `vec_i` is used.
pub fn gf_vect_mad(
    len: usize,
    vec: usize,
    vec_i: usize,
    gftbls: &[u8],
    src: &[u8],
    dest: &mut [u8],
) -> Result<(), CodeError> {
    if vec_i >= vec {
        return Err(CodeError::ShardIndex {
            index: vec_i,
            k: vec,
        });
    }
    if src.len() < len || dest.len() < len {
        return Err(CodeError::ShardLength {
            index: vec_i,
            expected: len,
            got: src.len().min(dest.len()),
        });
    }
    let tbl = tbl_at(gftbls, vec_i)?;
    // Exact-length buffers take the dispatched mad kernel (nibble tables by
    // this API's contract, so the nibble set — never GFNI's affine format).
    if src.len() == len && dest.len() == len {
        (nibble_kernels().mad)(tbl, src, dest);
        return Ok(());
    }
    SCALAR_CENSUS_BYTES.fetch_add(len as u64, Ordering::Relaxed);
    for (d, &s) in dest[..len].iter_mut().zip(&src[..len]) {
        *d ^= table_mul(tbl, s);
    }
    Ok(())
}

/// Constant multiply of a vector — ISA-L `gf_vect_mul`, INCLUDING its
/// documented contract that `len` must be a multiple of 32 (returning an error
/// otherwise, as ISA-L returns nonzero). The typed kernel
/// [`rusty_erasure_core::kernel::vect_mul`] has no such restriction.
pub fn gf_vect_mul(len: usize, gftbl: &[u8], src: &[u8], dest: &mut [u8]) -> Result<(), CodeError> {
    if !len.is_multiple_of(32) {
        return Err(CodeError::ShardLength {
            index: 0,
            expected: len - (len % 32),
            got: len,
        });
    }
    if src.len() < len || dest.len() < len {
        return Err(CodeError::ShardLength {
            index: 0,
            expected: len,
            got: src.len().min(dest.len()),
        });
    }
    let tbl = tbl_at(gftbl, 0)?;
    // Exact-length buffers take the dispatched encode kernel: one row, one
    // source is exactly overwrite-with-product.
    if src.len() == len && dest.len() == len {
        (nibble_kernels().encode)(&gftbl[..TABLE_BYTES], &[src], &mut [dest]);
        return Ok(());
    }
    SCALAR_CENSUS_BYTES.fetch_add(len as u64, Ordering::Relaxed);
    for (d, &s) in dest[..len].iter_mut().zip(&src[..len]) {
        *d = table_mul(tbl, s);
    }
    Ok(())
}

/// Re-exported so compat callers can reach the raw generator the typed
/// [`crate::Matrix`] wraps.
pub use matrix::invert_gauss_jordan as gf_invert_matrix_raw;

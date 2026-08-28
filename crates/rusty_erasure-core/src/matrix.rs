//! Coding matrices: generation (Vandermonde / Cauchy), inversion, and row
//! selection for recovery.
//!
//! Constructions are behavior-identical to ISA-L's `gf_gen_rs_matrix`,
//! `gf_gen_cauchy1_matrix`, and `gf_invert_matrix` — with the contract ISA-L
//! leaves to the caller enforced here as `Result`s: dimension checks, the
//! documented Vandermonde safe region, and singularity as a typed error
//! instead of a `-1`.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::MatrixError;
use crate::gf;

/// A row-major `rows x cols` matrix over GF(2^8).
///
/// For an encode matrix, `rows = k + p` (sources + parity, ISA-L's `m`) and
/// `cols = k`; the top `k x k` block is the identity, so source shards pass
/// through unchanged and the bottom `p` rows generate parity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<u8>,
}

/// ISA-L's documented safe region for Vandermonde matrices (`k` sources,
/// `m` total rows): inside it every recovery submatrix is invertible; outside
/// it singular decode matrices exist, so [`Matrix::reed_solomon`] refuses.
const fn vandermonde_safe(k: usize, m: usize) -> bool {
    k <= 3 || (k == 4 && m <= 25) || (k == 5 && m <= 10) || (k <= 21 && m - k == 4) || m - k <= 3
}

fn check_dims(k: usize, p: usize) -> Result<(), MatrixError> {
    // checked_add: k + p must not overflow for adversarial dimensions — the
    // no-panic sweep feeds usize::MAX here on purpose.
    if k == 0 || p == 0 || !k.checked_add(p).is_some_and(|m| m <= 255) {
        return Err(MatrixError::Dimensions { k, p });
    }
    Ok(())
}

impl Matrix {
    /// Vandermonde-style encode matrix for `k` sources and `p` parity rows —
    /// ISA-L's `gf_gen_rs_matrix`, refusing the configurations ISA-L's own
    /// documentation marks unsafe (where some decode submatrices are
    /// singular). Outside the safe region use [`Matrix::cauchy`].
    pub fn reed_solomon(k: usize, p: usize) -> Result<Self, MatrixError> {
        check_dims(k, p)?;
        let m = k + p;
        if !vandermonde_safe(k, m) {
            return Err(MatrixError::VandermondeUnsafe { k, p });
        }
        let mut data = vec![0u8; m * k];
        for i in 0..k {
            data[k * i + i] = 1;
        }
        let mut row_gen: u8 = 1;
        for i in k..m {
            let mut coeff: u8 = 1;
            for j in 0..k {
                data[k * i + j] = coeff;
                coeff = gf::mul(coeff, row_gen);
            }
            row_gen = gf::mul(row_gen, 2);
        }
        Ok(Self { rows: m, cols: k, data })
    }

    /// Cauchy encode matrix for `k` sources and `p` parity rows — ISA-L's
    /// `gf_gen_cauchy1_matrix`. Every square submatrix is invertible, so any
    /// `(k, p)` within the field limit is a valid configuration; this is the
    /// recommended general-purpose construction.
    pub fn cauchy(k: usize, p: usize) -> Result<Self, MatrixError> {
        check_dims(k, p)?;
        let m = k + p;
        let mut data = vec![0u8; m * k];
        for i in 0..k {
            data[k * i + i] = 1;
        }
        for i in k..m {
            for j in 0..k {
                // i >= k > j, so i ^ j is never zero and inv() never sees 0.
                data[k * i + j] = gf::inv((i ^ j) as u8);
            }
        }
        Ok(Self { rows: m, cols: k, data })
    }

    /// Build a matrix from raw row-major bytes. `data.len()` must equal
    /// `rows * cols`, and both dimensions must be in `1..=255`.
    pub fn from_bytes(rows: usize, cols: usize, data: Vec<u8>) -> Result<Self, MatrixError> {
        if rows == 0 || cols == 0 || rows > 255 || cols > 255 || data.len() != rows * cols {
            return Err(MatrixError::Dimensions { k: cols, p: rows.saturating_sub(cols) });
        }
        Ok(Self { rows, cols, data })
    }

    /// Number of rows (`k + p` for an encode matrix — ISA-L's `m`).
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns (`k`, the source count, for an encode matrix).
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// The coefficient at (`row`, `col`), or `None` out of bounds.
    pub fn get(&self, row: usize, col: usize) -> Option<u8> {
        if row < self.rows && col < self.cols {
            Some(self.data[self.cols * row + col])
        } else {
            None
        }
    }

    /// The raw row-major coefficient bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// The bottom `p` rows — the parity-generating block, in exactly the
    /// layout `tables::init_tables` expects.
    pub fn parity_bytes(&self) -> &[u8] {
        &self.data[self.cols * self.cols..]
    }

    /// Select `indices.len()` rows (in order) into a new matrix — the step
    /// that builds a decode matrix from the surviving shards' rows. Indices
    /// must be in range; duplicates are allowed here and will simply produce
    /// a singular matrix at inversion.
    pub fn select_rows(&self, indices: &[usize]) -> Result<Self, MatrixError> {
        if indices.is_empty() || indices.len() > 255 {
            return Err(MatrixError::Dimensions { k: self.cols, p: 0 });
        }
        let mut data = Vec::with_capacity(indices.len() * self.cols);
        for &r in indices {
            if r >= self.rows {
                return Err(MatrixError::Dimensions { k: self.cols, p: 0 });
            }
            data.extend_from_slice(&self.data[self.cols * r..self.cols * (r + 1)]);
        }
        Ok(Self { rows: indices.len(), cols: self.cols, data })
    }

    /// Invert a square matrix — ISA-L's `gf_invert_matrix` (Gauss-Jordan with
    /// row-swap pivoting), except non-destructive and with singularity as a
    /// typed error. Non-square input is a dimension error.
    pub fn invert(&self) -> Result<Self, MatrixError> {
        if self.rows != self.cols {
            return Err(MatrixError::Dimensions { k: self.cols, p: self.rows.saturating_sub(self.cols) });
        }
        let n = self.rows;
        let mut a = self.data.clone();
        let mut out = vec![0u8; n * n];
        invert_gauss_jordan(&mut a, &mut out, n)?;
        Ok(Self { rows: n, cols: n, data: out })
    }

    /// Matrix product `self * rhs` (used by tests and the recovery path).
    /// Dimension mismatch is an error, never a panic.
    pub fn multiply(&self, rhs: &Self) -> Result<Self, MatrixError> {
        if self.cols != rhs.rows {
            return Err(MatrixError::Dimensions { k: rhs.rows, p: 0 });
        }
        let mut data = vec![0u8; self.rows * rhs.cols];
        for i in 0..self.rows {
            for j in 0..rhs.cols {
                let mut s = 0u8;
                for t in 0..self.cols {
                    s ^= gf::mul(self.data[self.cols * i + t], rhs.data[rhs.cols * t + j]);
                }
                data[rhs.cols * i + j] = s;
            }
        }
        Ok(Self { rows: self.rows, cols: rhs.cols, data })
    }

    /// True if this is the identity matrix.
    pub fn is_identity(&self) -> bool {
        self.rows == self.cols
            && self
                .data
                .iter()
                .enumerate()
                .all(|(idx, &v)| v == u8::from(idx / self.cols == idx % self.cols))
    }
}

/// The raw Gauss-Jordan inversion under [`Matrix::invert`] and the compat
/// layer's `gf_invert_matrix` — ISA-L semantics: `a` (row-major `n x n`) is
/// DESTROYED (reduced to the identity on success), `out` receives the inverse,
/// and a singular input is a typed error. Slice lengths must be `n * n`.
pub fn invert_gauss_jordan(a: &mut [u8], out: &mut [u8], n: usize) -> Result<(), MatrixError> {
    if n == 0 || !n.checked_mul(n).is_some_and(|nn| a.len() == nn && out.len() == nn) {
        return Err(MatrixError::Dimensions { k: n, p: 0 });
    }
    out.fill(0);
    for i in 0..n {
        out[n * i + i] = 1;
    }

    for i in 0..n {
        if a[n * i + i] == 0 {
            // Find a lower row with a non-zero in this column and swap.
            let mut pivot = None;
            for j in i + 1..n {
                if a[n * j + i] != 0 {
                    pivot = Some(j);
                    break;
                }
            }
            let Some(j) = pivot else {
                return Err(MatrixError::Singular);
            };
            for c in 0..n {
                a.swap(n * i + c, n * j + c);
                out.swap(n * i + c, n * j + c);
            }
        }

        let scale = gf::inv(a[n * i + i]);
        for c in 0..n {
            a[n * i + c] = gf::mul(a[n * i + c], scale);
            out[n * i + c] = gf::mul(out[n * i + c], scale);
        }

        for j in 0..n {
            if j == i {
                continue;
            }
            let f = a[n * j + i];
            if f == 0 {
                continue;
            }
            for c in 0..n {
                out[n * j + c] ^= gf::mul(f, out[n * i + c]);
                a[n * j + c] ^= gf::mul(f, a[n * i + c]);
            }
        }
    }
    Ok(())
}


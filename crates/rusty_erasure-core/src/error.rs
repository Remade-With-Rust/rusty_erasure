//! Typed errors for every public operation.
//!
//! ISA-L's own README states that parameters passed to its functions are *not*
//! validated — callers are responsible for argument validity. This crate's
//! contract is the opposite (mission plan §3): every public entry point checks
//! its inputs and returns one of these errors instead of reading out of bounds
//! or panicking. Fuzzers hold that line from M1 onward.

use core::fmt;

/// Errors from coding-matrix construction and inversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatrixError {
    /// The requested dimensions are unusable: `k` (source shards) or `p`
    /// (parity shards) is zero, or `k + p` exceeds 255 — the Vandermonde and
    /// Cauchy constructions run out of distinct GF(2^8) elements past that.
    Dimensions {
        /// Requested source-shard count.
        k: usize,
        /// Requested parity-shard count.
        p: usize,
    },
    /// The requested `(k, p)` falls outside ISA-L's documented safe region for
    /// Vandermonde matrices (mission plan §3), where some recovery submatrices
    /// are singular. Use a Cauchy matrix instead — every submatrix inverts.
    VandermondeUnsafe {
        /// Requested source-shard count.
        k: usize,
        /// Requested parity-shard count.
        p: usize,
    },
    /// The submatrix selected for recovery is singular and cannot be inverted.
    Singular,
}

impl fmt::Display for MatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Dimensions { k, p } => write!(
                f,
                "unusable matrix dimensions k={k}, p={p}: need k >= 1, p >= 1, k + p <= 255"
            ),
            Self::VandermondeUnsafe { k, p } => write!(
                f,
                "k={k}, p={p} is outside the safe Vandermonde region; use Matrix::cauchy"
            ),
            Self::Singular => f.write_str("recovery submatrix is singular"),
        }
    }
}

impl core::error::Error for MatrixError {}

/// Errors from encode and update operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodeError {
    /// The number of shard buffers supplied does not match the coder's matrix.
    ShardCount {
        /// Shard count the coder's matrix requires.
        expected: usize,
        /// Shard count actually supplied.
        got: usize,
    },
    /// A shard buffer's length disagrees with the stripe's shard length.
    ShardLength {
        /// Index of the offending shard.
        index: usize,
        /// Length every shard in this call must have.
        expected: usize,
        /// Length actually supplied.
        got: usize,
    },
    /// A shard index argument is out of range for the coder's matrix.
    ShardIndex {
        /// The out-of-range index.
        index: usize,
        /// Number of source shards in the coder's matrix.
        k: usize,
    },
}

impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ShardCount { expected, got } => {
                write!(f, "wrong shard count: expected {expected}, got {got}")
            }
            Self::ShardLength {
                index,
                expected,
                got,
            } => write!(
                f,
                "shard {index} has length {got}, but this stripe's shard length is {expected}"
            ),
            Self::ShardIndex { index, k } => {
                write!(f, "shard index {index} out of range for k={k} sources")
            }
        }
    }
}

impl core::error::Error for CodeError {}

/// Errors from the recovery path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecoverError {
    /// More shards are missing than the code can repair.
    TooManyMissing {
        /// Number of missing shards.
        missing: usize,
        /// Number of parity shards (the repair capacity).
        p: usize,
    },
    /// The surviving-rows submatrix could not be inverted.
    Matrix(MatrixError),
    /// A shard buffer failed validation.
    Code(CodeError),
}

impl fmt::Display for RecoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TooManyMissing { missing, p } => {
                write!(
                    f,
                    "{missing} shards missing, but only {p} parity shards exist"
                )
            }
            Self::Matrix(e) => write!(f, "recovery matrix error: {e}"),
            Self::Code(e) => write!(f, "recovery shard error: {e}"),
        }
    }
}

impl core::error::Error for RecoverError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Matrix(e) => Some(e),
            Self::Code(e) => Some(e),
            Self::TooManyMissing { .. } => None,
        }
    }
}

impl From<MatrixError> for RecoverError {
    fn from(e: MatrixError) -> Self {
        Self::Matrix(e)
    }
}

impl From<CodeError> for RecoverError {
    fn from(e: CodeError) -> Self {
        Self::Code(e)
    }
}

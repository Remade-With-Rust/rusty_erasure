//! rusty_erasure — Intel ISA-L's erasure coding, remade with Rust.
//!
//! The facade crate: the typed public API (`Matrix`, `Coder`), parameter
//! validation, the ISA-L-named compat layer ([`isal`]), and kernel dispatch —
//! resolved ONCE at [`coder`]/[`best_kernels`], never inside a loop. The
//! always-on reach [`census`] reports which kernel set production bytes
//! actually flow through.
//!
//! Conformance is byte identity with ISA-L's `erasure_code` module across the
//! full config matrix — exact GF(2^8) integer math, no tolerance anywhere.
//! Mission plan: `docs/plans/erasure_mission.md`.

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod isal;

pub use rusty_erasure_core::{
    CodeError, Coder, Matrix, MatrixError, RecoverError, gf, kernel, matrix, tables,
};

/// The best kernel set for the running CPU: the accel SIMD sets when the
/// `accel` feature is on and the CPU supports them (detection cached), the
/// scalar oracle otherwise. Resolved once per call site — hold the result or
/// build a [`Coder`] with [`coder`] rather than re-asking in a loop.
pub fn best_kernels() -> kernel::Kernels {
    #[cfg(feature = "accel")]
    if let Some(k) = rusty_erasure_accel::x86::kernels() {
        return k;
    }
    kernel::Kernels::scalar()
}

/// Build a [`Coder`] driving the best kernel set for this CPU. This is the
/// constructor applications want; `Coder::new` in core is the always-scalar
/// variant.
pub fn coder(matrix: Matrix) -> Result<Coder, MatrixError> {
    Coder::with_kernels(matrix, best_kernels())
}

/// The always-on kernel-reach census (mission plan §7.1).
pub mod census {
    use core::sync::atomic::Ordering;

    /// Source bytes processed per kernel family since process start.
    #[derive(Debug, Clone, Copy)]
    pub struct Census {
        /// Bytes through the scalar oracle kernels.
        pub scalar_bytes: u64,
        /// Bytes through the accel (SIMD) kernels.
        pub accel_bytes: u64,
    }

    impl Census {
        /// Percentage of counted bytes that went through the accel kernels
        /// (`None` when nothing has been counted yet).
        pub fn accel_percent(&self) -> Option<f64> {
            let total = self.scalar_bytes + self.accel_bytes;
            if total == 0 {
                None
            } else {
                Some(self.accel_bytes as f64 * 100.0 / total as f64)
            }
        }
    }

    /// Read the counters. On the arch you ship, anything under 100% accel on
    /// the shipping path is a defect, not a statistic (the rusty_zstd law).
    pub fn read() -> Census {
        Census {
            scalar_bytes:
                crate::kernel::SCALAR_CENSUS_BYTES.load(Ordering::Relaxed),
            accel_bytes: {
                #[cfg(feature = "accel")]
                {
                    rusty_erasure_accel::x86::ACCEL_CENSUS_BYTES.load(Ordering::Relaxed)
                }
                #[cfg(not(feature = "accel"))]
                {
                    0
                }
            },
        }
    }
}

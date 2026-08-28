//! rusty_erasure-core — GF(2^8) arithmetic, coding matrices, and the scalar
//! erasure-coding kernels that serve as the permanent correctness oracles.
//!
//! This crate is `no_std + alloc`, has **zero dependencies**, and forbids
//! `unsafe`. Everything here must be usable by a developer who has never heard
//! of MATA: bytes in, bytes out. SIMD twins live in `rusty_erasure-accel`; the
//! typed public API and kernel dispatch live in the `rusty_erasure` facade.
//!
//! Conformance target: byte identity with Intel ISA-L's `erasure_code` module,
//! proven against checked-in golden vectors generated from real ISA-L on the
//! bench rig. Mission plan: `docs/plans/erasure_mission.md` (§7 for the gates).
//!
//! Milestone map for this crate: M1 lands `gf`, `matrix`, and `tables`; M2
//! lands the scalar kernels and `encode`/`update`/`recover`.

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

extern crate alloc;

pub mod encode;
pub mod error;
pub mod gf;
pub mod kernel;
pub mod matrix;
pub mod raid;
pub mod tables;

pub use encode::Coder;
pub use error::{CodeError, MatrixError, RecoverError};
pub use matrix::Matrix;

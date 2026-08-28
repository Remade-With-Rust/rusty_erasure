//! rusty_erasure — Intel ISA-L's erasure coding, remade with Rust.
//!
//! The facade crate: the typed public API (`Matrix`, `Coder` — M3), parameter
//! validation, the ISA-L-named compat layer for porting C callers and ISA-L's
//! own tests verbatim (M3), and kernel dispatch (scalar core everywhere;
//! SSSE3/AVX2 twins behind the `accel` feature from M4, census-proven).
//!
//! Conformance is byte identity with ISA-L's `erasure_code` module across the
//! full config matrix — exact GF(2^8) integer math, no tolerance anywhere.
//! Mission plan: `docs/plans/erasure_mission.md`.

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod isal;

pub use rusty_erasure_core::{Coder, CodeError, Matrix, MatrixError, RecoverError, gf, kernel, matrix, tables};

// The accel dependency edge exists from M0 so the feature graph and the
// `--no-default-features` CI gate are honest; dispatch wires it at M4.
#[cfg(feature = "accel")]
use rusty_erasure_accel as _;

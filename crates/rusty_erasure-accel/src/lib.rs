//! rusty_erasure-accel — the workspace's ONLY unsafe crate: hand-written SIMD
//! twins of the scalar kernels in `rusty_erasure-core`.
//!
//! Every kernel that lands here lands with (mission plan §3, §6):
//!
//! - a `*_matches_scalar` oracle test over random + edge inputs — the scalar
//!   twin in `rusty_erasure-core` stays in-tree forever as oracle and fallback;
//! - a `// SAFETY:` invariant on every unsafe block, adversarially reviewed;
//! - runtime feature detection resolved once at the call surface, never inside
//!   the hot loop (the `#[target_feature]` function owns the whole loop);
//! - a reach-census tap proving the shipping path actually calls it — an
//!   exported kernel nothing calls is the defect every other gate passes.
//!
//! M4 lands the first kernels (x86-64 SSSE3/AVX2 PSHUFB nibble tables, 1..6-row
//! fused dot products). Until then this crate is intentionally empty and the
//! facade routes everything to the scalar core.

#![no_std]
#![deny(missing_docs)]

// Re-exported so the facade's dispatch layer has a single upstream to name once
// kernels exist; harmless (and proof of the dependency edge) until M4.
pub use rusty_erasure_core as scalar;

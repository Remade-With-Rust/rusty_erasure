//! rusty_erasure-accel — the workspace's ONLY unsafe crate: hand-written SIMD
//! twins of the scalar kernels in `rusty_erasure-core`.
//!
//! The named reason these exist (codec-vectorize-kernel step 0): the GF(2^8)
//! multiply here is the PSHUFB nibble-table algorithm — two byte-shuffles and
//! an XOR per 16/32 bytes — which is a *different algorithm* from the scalar
//! per-byte table lookup, in the polynomial/table class the compiler cannot
//! derive from the scalar recurrence. Auto-vectorization is structurally
//! unavailable; hand-written kernels are the only route.
//!
//! Discipline (mission plan §3/§6):
//! - every kernel set is exposed ONLY through checked constructors
//!   ([`x86::kernels`] / [`x86::kernels_at`]) that verify the CPU feature —
//!   the `#[target_feature]` boundary sits at the call surface, whole loops
//!   inside (the dispatch-placement law);
//! - every unsafe block carries a `// SAFETY:` invariant, and the safe
//!   wrappers re-assert slice lengths so no contract violation can reach an
//!   out-of-bounds access;
//! - the scalar set in core stays the permanent oracle: `*_matches_scalar`
//!   tests cover random + edge inputs for every kernel and every level;
//! - each set counts the source bytes it processes into
//!   [`x86::ACCEL_CENSUS_BYTES`] — the reach census is always on.

#![deny(missing_docs)]

pub mod x86;

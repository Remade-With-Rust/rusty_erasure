//! rusty_erasure-accel — the workspace's ONLY unsafe crate: hand-written SIMD
//! twins of the scalar kernels in `rusty_erasure-core`.
//!
//! The named reason these exist (codec-vectorize-kernel step 0): the GF(2^8)
//! multiply here is either the PSHUFB/TBL/swizzle nibble-table algorithm or a
//! `GF2P8AFFINEQB` affine transform — *different algorithms* from the scalar
//! per-byte lookup, in the polynomial/table class the compiler cannot derive.
//! Auto-vectorization is structurally unavailable; hand-written kernels are
//! the only route.
//!
//! Per-arch inventory (the per-arch-parity non-negotiable):
//! - [`x86`] — SSSE3/AVX2 nibble + AVX2-GFNI affine (runtime-dispatched)
//! - [`aarch64`] — NEON TBL nibble (baseline, no dispatch needed)
//! - [`wasm`] — SIMD128 swizzle nibble (compile-time `+simd128`)
//!
//! Discipline: every kernel set is exposed only through checked constructors;
//! every unsafe block carries a `// SAFETY:` invariant and the safe wrappers
//! re-assert slice lengths; the scalar set in core stays the permanent oracle
//! (`*_matches_scalar` gates every set); every set counts its source bytes
//! into [`ACCEL_CENSUS_BYTES`] — the reach census is always on.

#![deny(missing_docs)]

use core::sync::atomic::AtomicU64;

use rusty_erasure_core::kernel::Kernels;

pub mod aarch64;
#[cfg(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    all(target_arch = "wasm32", target_feature = "simd128")
))]
mod tail;
pub mod wasm;
pub mod x86;

/// Census counter shared by every accel kernel set: source bytes processed.
/// One relaxed add per call, never per element.
pub static ACCEL_CENSUS_BYTES: AtomicU64 = AtomicU64::new(0);

/// The `(xor_gen, pq_gen)` function pair a RAID dispatch returns.
pub type RaidKernels = (fn(&[&[u8]], &mut [u8]), fn(&[&[u8]], &mut [u8], &mut [u8]));

/// The best kernel set for the running CPU on THIS architecture, or `None`
/// when no SIMD set applies — callers fall back to `Kernels::scalar()`.
pub fn kernels() -> Option<Kernels> {
    #[cfg(target_arch = "x86_64")]
    {
        x86::kernels()
    }
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::kernels()
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::kernels()
    }
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "wasm32"
    )))]
    {
        None
    }
}

/// The best RAID kernel pair (xor_gen, pq_gen) for this architecture, or
/// `None` when only the scalar core applies. Byte-identical to
/// `rusty_erasure_core::raid` on every arch (oracle-tested).
pub fn raid_kernels() -> Option<RaidKernels> {
    #[cfg(target_arch = "x86_64")]
    {
        x86::raid_kernels()
    }
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::raid_kernels()
    }
    #[cfg(target_arch = "wasm32")]
    {
        wasm::raid_kernels()
    }
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "wasm32"
    )))]
    {
        None
    }
}

/// The best kernel set consuming ISA-L NIBBLE-format tables (the compat
/// layer's requirement — its callers hand it `ec_init_tables`-format tables
/// by contract, and GFNI's affine tables must never be mixed in).
pub fn kernels_nibble() -> Option<Kernels> {
    #[cfg(target_arch = "x86_64")]
    {
        x86::kernels_nibble()
    }
    // Every non-x86 set is nibble-format already.
    #[cfg(not(target_arch = "x86_64"))]
    {
        kernels()
    }
}

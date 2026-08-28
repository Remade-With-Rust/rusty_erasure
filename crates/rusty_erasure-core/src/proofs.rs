//! Kani proof harnesses (use-protection-please H-30).
//!
//! **What these prove, and what they deliberately do not.** Kani has no model
//! for `core::arch` SIMD intrinsics, so the AVX2/GFNI/NEON/SIMD128 kernel
//! bodies cannot be symbolically executed. What CAN be proven — and what the
//! whole safety argument for those kernels actually rests on — is the
//! *arithmetic that justifies the unsafe*: that the index expressions the
//! kernels use stay inside the buffers their safe wrappers asserted.
//!
//! So the division of labour is:
//!
//! | Property | Checked by |
//! |---|---|
//! | Index arithmetic is in bounds for all valid shapes | **these proofs** (exhaustive over the symbolic domain) |
//! | Kernel output equals the scalar oracle | byte-identity gates, all architectures |
//! | No OOB access at runtime | ASan fuzzing + Miri |
//! | Dimension validation never panics | **these proofs** |
//!
//! Compiled only under `cfg(kani)`, so they cost the shipped build nothing.

#![allow(clippy::undocumented_unsafe_blocks)] // no unsafe here; lint is crate-wide

use crate::gf;
use crate::tables::TABLE_BYTES;

/// GF(2^8) multiplication is commutative — the field axiom every kernel's
/// re-association of the parity sum silently depends on.
#[kani::proof]
fn gf_mul_is_commutative() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    assert_eq!(gf::mul(a, b), gf::mul(b, a));
}

/// 1 is the multiplicative identity and 0 annihilates.
#[kani::proof]
fn gf_mul_identity_and_zero() {
    let a: u8 = kani::any();
    assert_eq!(gf::mul(a, 1), a);
    assert_eq!(gf::mul(a, 0), 0);
    assert_eq!(gf::mul(0, a), 0);
}

/// Every nonzero element has an inverse, and it really inverts. This is what
/// makes recovery total: a decode matrix over survivors is invertible, and a
/// wrong `inv` would silently reconstruct wrong bytes.
#[kani::proof]
fn gf_inverse_inverts() {
    let a: u8 = kani::any();
    kani::assume(a != 0);
    assert_eq!(gf::mul(a, gf::inv(a)), 1);
}

/// The two independent multiply implementations agree everywhere — the table
/// lookup the kernels use, and the shift-and-XOR twin that defines the field.
/// A table corrupted by a bad build would diverge here.
#[kani::proof]
// `mul_shift` is the 8-round shift-and-XOR definition; bounding the unwind to
// its exact trip count keeps this tractable instead of open-ended.
#[kani::unwind(9)]
fn gf_table_and_shift_multiplies_agree() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    assert_eq!(gf::mul(a, b), gf::mul_shift(a, b));
}

/// The dimension check is total over the WHOLE `usize` domain, with no
/// allocation and no loops — this is the proof form of the overflow defect
/// the M1 no-panic sweep found, where `k + p` wrapped for adversarial
/// dimensions before it became `checked_add`.
///
/// Deliberately expressed as pure arithmetic rather than by calling
/// `Matrix::reed_solomon`: that would loop `k * p` times over symbolic
/// bounds, which CBMC cannot unwind. The call itself is covered by the
/// bounded harness below and, over the full domain, by the no-panic sweeps
/// and fuzz targets.
#[kani::proof]
fn dimension_check_arithmetic_cannot_overflow() {
    let k: usize = kani::any();
    let p: usize = kani::any();

    // The exact predicate `check_dims` applies.
    let accepted = k != 0 && p != 0 && k.checked_add(p).is_some_and(|m| m <= 255);

    if accepted {
        // An accepted pair can be added without wrapping, and the sum is a
        // legal shard count. A plain `k + p` would have wrapped here.
        assert!(k <= 255 && p <= 255);
        assert!(k + p <= 255);
    } else {
        // Rejection is the only alternative — there is no third outcome and
        // no path that computes a wrapped sum before deciding.
        assert!(k == 0 || p == 0 || k > 255 || p > 255 || k + p > 255 || k > usize::MAX - p);
    }
}

/// The constructor itself never panics on small shapes, loops fully unwound.
#[kani::proof]
#[kani::unwind(6)]
fn matrix_construction_never_panics_bounded() {
    let k: usize = kani::any();
    let p: usize = kani::any();
    kani::assume(k <= 4 && p <= 4);
    // Both constructors must return a typed result, never panic.
    let _ = crate::Matrix::reed_solomon(k, p);
    let _ = crate::Matrix::cauchy(k, p);
}

/// **The kernel bounds lemma.** Every nibble kernel indexes its expanded
/// table block as `(r * k + j) * TABLE_BYTES`, then reads 32 bytes from
/// there. The safe wrappers assert `gftbls.len() >= rows * k * TABLE_BYTES`.
/// This proves those two facts compose: for every in-range `(r, j)`, the
/// whole 32-byte read lies inside the asserted region — which is exactly the
/// `// SAFETY:` claim written on those unsafe blocks.
#[kani::proof]
fn nibble_table_offsets_stay_in_bounds() {
    let rows: usize = kani::any();
    let k: usize = kani::any();
    let r: usize = kani::any();
    let j: usize = kani::any();

    // The domain the wrappers establish, bounded to the shipped config grid
    // — exactly the k = 1..32, rows = 1..8 that the 902-case full-grid
    // conformance covers. The bound is not cosmetic: `r * k` is a symbolic
    // multiply of two symbolic values, and nonlinear arithmetic is what
    // makes an SMT proof diverge rather than the range itself.
    kani::assume(rows >= 1 && rows <= 8);
    kani::assume(k >= 1 && k <= 32);
    kani::assume(r < rows);
    kani::assume(j < k);

    let len = rows * k * TABLE_BYTES; // what the wrapper asserted
    let start = (r * k + j) * TABLE_BYTES; // what the kernel computes

    // The read is [start, start + 32) and must lie wholly inside [0, len).
    assert!(start < len);
    assert!(start + TABLE_BYTES <= len);
    // And the high-nibble half the kernel loads at start + 16.
    assert!(start + 16 + 16 <= len);
}

/// The same lemma for GFNI's affine tables, which are 8 bytes per coefficient
/// rather than 32 — the format mismatch that produced wrong parity at
/// plausible speed in M4 lives in exactly this arithmetic.
#[kani::proof]
fn affine_table_offsets_stay_in_bounds() {
    const AFFINE_BYTES: usize = 8;
    let rows: usize = kani::any();
    let k: usize = kani::any();
    let r: usize = kani::any();
    let j: usize = kani::any();

    kani::assume(rows >= 1 && rows <= 8);
    kani::assume(k >= 1 && k <= 32);
    kani::assume(r < rows);
    kani::assume(j < k);

    let len = rows * k * AFFINE_BYTES;
    let start = (r * k + j) * AFFINE_BYTES;
    assert!(start + AFFINE_BYTES <= len);
}

/// The nibble table lookup indexes `tbl[x & 0x0f]` and `tbl[16 + (x >> 4)]`.
/// Both must be inside a 32-byte table for every possible byte — the reason
/// the kernels can use unchecked shuffles at all.
#[kani::proof]
fn table_mul_indices_are_in_range() {
    let x: u8 = kani::any();
    let lo = (x & 0x0f) as usize;
    let hi = 16 + (x >> 4) as usize;
    assert!(lo < TABLE_BYTES);
    assert!(hi < TABLE_BYTES);
}

/// A chunked kernel loop advances by `step` while `i + step <= n`. Proving
/// the loop invariant means no iteration can read past the buffer, for every
/// length and every unroll tier the kernels use (16/32/64/128).
#[kani::proof]
fn chunked_loop_never_reads_past_end() {
    let n: usize = kani::any();
    let i: usize = kani::any();
    let step: usize = kani::any();

    // Both bounds are needed BEFORE the guard is stated: Kani found that
    // writing `assume(i + step <= n)` with an unconstrained `i` overflows the
    // addition inside the assumption itself. A specification can have the bug
    // it is meant to rule out.
    kani::assume(n <= usize::MAX / 2);
    kani::assume(i <= usize::MAX / 2);
    kani::assume(step == 16 || step == 32 || step == 64 || step == 128);
    kani::assume(i + step <= n); // the loop guard, now overflow-free

    // Every byte the iteration touches is inside the buffer.
    assert!(i < n);
    assert!(i + step <= n);
    // The tail that remains is a valid, smaller problem.
    assert!(n - (i + step) < n);
}

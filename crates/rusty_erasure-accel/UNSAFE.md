# Unsafe inventory — `rusty_erasure-accel`

**Last audited**: 2026-08-28 · **Next review**: with any new kernel, or 2027-02-28

This is the workspace's **only** unsafe crate. `cargo geiger` confirms the
boundary holds: `rusty_erasure-core` 0/0, the `rusty_erasure` facade 0/0, and
every unsafe expression in the tree (19 functions / 2048 expressions) lives
here. Core and facade are `forbid(unsafe_code)`; the workspace lint table
denies `unsafe_code` and this crate is the single sanctioned override.

## Counts

| File | `unsafe {}` blocks | `// SAFETY:` comments | `unsafe fn` | `#[target_feature]` |
|---|---|---|---|---|
| `x86.rs` | 59 | 59 | 15 | 15 |
| `wasm.rs` | 13 | 13 | 2 | 5 |
| `aarch64.rs` | 7 | 7 | 2 | 0 (NEON is baseline) |
| `lib.rs` / `tail.rs` | 0 | — | 0 | — |

`clippy::undocumented_unsafe_blocks = "deny"` is set **in this crate's own
manifest** (a `[lints]` override drops the whole workspace table, so the lint
must be restated where the unsafe actually is) and the build is clean under
it — the count equality above is mechanically enforced, not a spot check.

## Why unsafe is unavoidable here

The GF(2^8) multiply is either the PSHUFB/TBL/swizzle nibble-table algorithm
or a `GF2P8AFFINEQB` affine transform — *different algorithms* from the scalar
per-byte lookup, in the polynomial/table class the compiler cannot derive.
Auto-vectorization is structurally unavailable; hand-written intrinsics are
the only route, and the intrinsics are `unsafe` by signature.

## The three classes of unsafe, and what backs each

1. **CPU feature preconditions** (`#[target_feature(enable = "avx2")]`, …).
   Discharged by construction: these fn pointers are handed out **only** by
   `kernels_at`/`raid_kernels_at` after `is_x86_feature_detected!`, cached in
   a `OnceLock`. NEON needs no detection (baseline on aarch64); wasm SIMD128
   is a compile-time `cfg`, so the module does not exist without it.
2. **Unchecked pointer arithmetic and loads/stores** (the bulk). Every safe
   wrapper re-asserts slice lengths *before* entering unsafe (`check_encode`
   and the per-kernel `assert_eq!`s), and every loop bound is written so the
   access window (`i + 32 <= len`, `i + 128 <= n`) is inside the asserted
   region. Each block's `// SAFETY:` names the specific inequality.
3. **One `transmute`** (`__m256i` → `[u64; 4]`, x86 register-lane tail): both
   types are 32-byte, 32-aligned POD with no niches or padding.

## What makes this auditable rather than merely commented

Comments are not proof. The load-bearing control is that **every kernel is
gated byte-identical against the `forbid(unsafe_code)` scalar core**:

- `matches_scalar.rs` — per-ISA-level encode/mad/update/RAID oracle gates,
  17 edge lengths straddling every vector width and unroll boundary, row
  counts straddling every fusion group, plus an exhaustive all-256-coefficient
  GFNI proof. Levels dispatch currently shadows (SSE2 under AVX2, AVX2-pq
  under GFNI-pq) are gated through `*_at` accessors so no kernel is untested.
- `portable_matches_scalar.rs` — the same law on whatever set this arch
  selects; it is *the* gate on the arm64 and wasm CI jobs, and it asserts the
  reach census advanced.
- Miri, ASan fuzzing (roundtrip + compat, ~1M execs), and the 902-case
  full-grid ISA-L conformance replay run over the dispatched path.

A memory-safety bug in a kernel that also changed a byte is caught by these.
A memory-safety bug that changes **no** byte (an over-read into readable
memory) is caught by ASan/Miri, not by the oracles — that gap is why both run.

## Residual risk

Recorded in [docs/plans/use-protection-please.md](../../docs/plans/use-protection-please.md).

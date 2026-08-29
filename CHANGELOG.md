# Changelog

All notable changes to this project are documented here. Security-relevant
changes are called out explicitly, per the hardening standard (H-38).

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.4.0] — 2026-08-28

The first release that carries the whole engine: every kernel on every
architecture, RAID-6 included, with the evidence to back it.

**Encoded output is unchanged from 0.1.0 and is frozen.** Shards written by
any build of this crate remain readable by every other, and remain
byte-identical to Intel ISA-L. Nothing here is a format change.

> Versioning note: 0.1.0 was the first publish; this jumps to 0.4.0 because
> the surface it covers is no longer a subset of the target. There were no
> 0.2.x or 0.3.x releases.

### Added

- **RAID-6 acceleration on every architecture.** `xor_gen`/`pq_gen` now
  dispatch to hand-written kernels on x86-64 (SSE2 baseline, AVX2, and a GFNI
  path that folds the ×2 recurrence into a single `GF2P8AFFINEQB`), aarch64
  NEON, and wasm SIMD128. Previously RAID was AVX2-or-scalar and x86-only.
- **Full SIMD parity across architectures.** The double-chunk table-load
  pattern, previously GFNI-only, now applies to every nibble encode kernel
  (AVX2, SSSE3, NEON, SIMD128), and a quad-chunk lane serves rebuilds of ≤2
  shards.
- **`DecodePlan`** — amortizes survivor selection, matrix inversion and table
  expansion across stripes, for steady-state repair of many stripes sharing
  one loss pattern.
- **Ten Kani proof harnesses** over the arithmetic the `// SAFETY:` comments
  assert: that every table offset and loop bound stays inside the region the
  safe wrappers established, plus the GF field laws and the overflow-freedom
  of dimension validation.
- **Cross-architecture fuzzing.** libFuzzer is x86-only, so the corpus is
  embedded at build time and replayed on wasm (wasmtime) and aarch64 (qemu),
  a seeded generator gives those architectures their own randomized coverage,
  and ASan now runs on aarch64 as well as x86.
- Six fuzz targets (added `raid` and `kernels`), a nightly corpus-persistent
  fuzzing workflow, a scheduled full-grid conformance job that regenerates its
  vectors from upstream ISA-L, and a release pipeline producing auditable
  binaries with a CycloneDX SBOM and build provenance.
- `SECURITY.md`, `deny.toml`, cargo-vet certifications, `rust-toolchain.toml`,
  `LICENSE-MIT` / `LICENSE-APACHE`, and a 41-gate hardening audit at
  `docs/plans/use-protection-please.md`.

### Changed

- **Performance: every measured cell against ISA-L's dispatched `avx2_gfni`
  assembly is now win-or-parity.** Encode 1.09×–1.64×, RAID `pq` 1.02×
  (from 0.744×), recovery 1.22×–1.26× (from ~0.76×), RAID `xor` 1.03×. Worst
  cell 0.99×. Method: pinned, interleaved ABBA, process CPU time, work
  identity gated by parity checksums, best-of-N.
- `overflow-checks = true` in the release profile — measured at 0.985× on the
  S2 GFNI cell, the deliberate price of the safety property.
- Every published crate now carries `documentation`, a README and complete
  keywords/categories.

### Security

- The workspace's only unsafe crate had inherited **no** lint policy, because
  a crate-level `[lints]` override replaces the workspace table rather than
  merging with it. `clippy::undocumented_unsafe_blocks = "deny"` is restated
  there and the five undocumented blocks it exposed are documented.
- **The Miri gate was vacuous** and is now real. Under Miri
  `is_x86_feature_detected!` returns false, so dispatch fell back to scalar
  and the job passed green having executed none of the unsafe code it exists
  to check. It now runs with `+ssse3,+avx2` and demonstrably reaches the
  kernels. Miri is the stated cover for a memory fault that changes no output
  byte — the fault the byte-identity oracles structurally cannot see — so this
  was the important half.
- Every dependency in the shipped tree carries a cargo-vet audit
  certification. The published library has **zero third-party dependencies**.

### Fixed

- `xor_check`/`pq_check` and the RAID generators handle any length, including
  the sub-word tails ISA-L's base code silently drops.

## [0.1.0] — 2026-08-28

Initial publish: matrix-flexible GF(2^8) Reed-Solomon encode, incremental
update and recovery, byte-identical to ISA-L, with SSSE3/AVX2/GFNI, NEON and
SIMD128 kernels behind runtime dispatch.

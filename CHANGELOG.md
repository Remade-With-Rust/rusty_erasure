# Changelog

All notable changes to this project are documented here. Security-relevant
changes are called out explicitly, per the hardening standard (H-38).

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **RAID SIMD on every architecture.** `xor_gen`/`pq_gen` now dispatch to
  hand-written kernels on x86-64 (SSE2 baseline, AVX2, and a GFNI path that
  folds the RAID-6 ×2 recurrence into a single `GF2P8AFFINEQB`), aarch64 NEON,
  and wasm SIMD128. Previously RAID was AVX2-or-scalar, x86-only.
- **A quad-chunk recovery lane** (`encode_group_gfni4`) for rebuilds of ≤2
  shards, where one matrix broadcast now serves four 32-byte chunks.
- **Two new fuzz targets**, `raid` and `kernels`, covering the newest unsafe
  code in the tree differentially against the scalar oracle.
- **Cross-architecture fuzz coverage.** The fuzz corpus is embedded at build
  time and replayed on wasm32 (wasmtime) and aarch64 (qemu), and a seeded
  generator gives those architectures their own randomized coverage. libFuzzer
  is x86-only; this is what makes "fuzzed" mean something for the SIMD128 and
  NEON kernels.
- Nightly continuous-fuzzing workflow with corpus persistence, and a release
  workflow producing auditable binaries, a CycloneDX SBOM, verified binary
  hardening and build provenance.
- `SECURITY.md`, `deny.toml`, `supply-chain/` (cargo-vet), `rust-toolchain.toml`,
  `crates/rusty_erasure-accel/UNSAFE.md`, and a full 41-gate hardening audit at
  `docs/plans/use-protection-please.md`.

### Changed

- `overflow-checks = true` in the release profile. Measured cost: 0.985× on the
  S2 GFNI encode cell — the deliberate price of the safety property.
- The double-chunk table-load pattern, previously GFNI-only, now applies to
  every nibble encode kernel (AVX2, SSSE3, NEON, SIMD128).

### Security

- The workspace's only unsafe crate had inherited **no** lint policy, because a
  crate-level `[lints]` override replaces the workspace table rather than
  merging with it. `clippy::undocumented_unsafe_blocks = "deny"` is now
  restated there, and the five unsafe blocks it exposed are documented.
- Every dependency in the shipped tree now carries a cargo-vet audit
  certification (34 audited, 2 documented exemptions).

### Notes

No public API changed. Encoded output is byte-identical to previous versions —
shards written by any earlier build remain readable, and the ISA-L and
`reed-solomon-erasure` conformance suites are unchanged and green.

## [0.1.0] — unreleased

Initial development. Not yet published to crates.io.

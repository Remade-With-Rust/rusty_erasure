# rusty_erasure

Intel [ISA-L](https://github.com/intel/isa-l)'s erasure coding, remade with Rust: a
matrix-flexible GF(2^8) Reed-Solomon engine — encode, incremental update, and first-class
recovery — with a `no_std`, zero-dependency, `forbid(unsafe)` core, validated parameters
(ISA-L validates nothing), and SIMD acceleration behind runtime dispatch. No C, no NASM,
no GPL, anywhere in the build. MIT OR Apache-2.0.

Part of the [Remade-With-Rust](https://github.com/remade-with-rust) family. The erasure
primitive under SpaceDB/disco shard placement: k+p shards across peers survive p losses at
(k+p)/k overhead instead of 3× replication.

**Status: pre-release scaffold (M0).** The mission plan — scope, architecture, conformance
oracle, and release gates — is [docs/plans/erasure_mission.md](docs/plans/erasure_mission.md).

Conformance is **byte identity** with ISA-L's `erasure_code` module across the full config
matrix, proven against golden vectors generated from real ISA-L. Performance claims appear
here only after they exist in [corpus/LEDGER.md](corpus/LEDGER.md) with the run that
produced them — there are **no claims yet**.

## Workspace

| Crate | What it is |
|---|---|
| `rusty_erasure-core` | GF(2^8) math, matrices, scalar kernels (the permanent oracles). `no_std + alloc`, zero deps, `forbid(unsafe)`. |
| `rusty_erasure-accel` | The only unsafe crate: SIMD twins (x86-64 SSSE3/AVX2 first), each with a scalar-oracle test and a reach census. |
| `rusty_erasure` | The facade: typed API, validation, ISA-L-named compat layer, kernel dispatch. `accel` feature default-on; `--no-default-features` is the pure-safe build. |
| `rusty_erasure-cli` | `rerasure`: encode / recover / verify / bench / census. |
| `rusty_erasure-alloc` | The rusty_alloc seam — one crate, one pin. |

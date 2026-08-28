[![crates.io](https://img.shields.io/crates/v/rusty_erasure.svg)](https://crates.io/crates/rusty_erasure)
[![docs.rs](https://img.shields.io/docsrs/rusty_erasure)](https://docs.rs/rusty_erasure)
[![CI](https://github.com/remade-with-rust/rusty_erasure/actions/workflows/ci.yml/badge.svg)](https://github.com/remade-with-rust/rusty_erasure/actions)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![remade with rust](https://img.shields.io/badge/remade--with--rust-portfolio-orange.svg)](https://github.com/remade-with-rust)

# rusty_erasure

Intel [ISA-L](https://github.com/intel/isa-l)'s **erasure coding**, rebuilt in
pure **Rust** — a matrix-flexible GF(2^8) **Reed-Solomon** engine with encode,
incremental update and first-class recovery, plus RAID-6 P+Q. Byte-identical to
ISA-L, faster than its hand-written AVX2-GFNI assembly, and it validates the
parameters ISA-L leaves undefined. No C, no NASM, no GPL anywhere in the build.

## ⚡ The headline

- **Faster than Intel's own assembly**, on the same silicon, interleaved and
  pinned: encode **1.09×–1.64×** across the corpus cells, RAID-6 `pq` **1.02×**,
  recovery **1.22×–1.26×**. Every measured cross-implementation cell is
  win-or-parity; the worst is 0.99×.
- **Byte-identical to ISA-L**, proven rather than asserted: **902/902**
  full-grid configurations, **65,536/65,536** GF products, **1470/1470**
  exhaustive loss patterns.
- **The whole engine runs in a browser.** SIMD128 kernels, no server and no C
  toolchain — something no C erasure library offers.
- **Misuse is a typed error, never undefined behaviour.** ISA-L documents that
  callers are responsible; here every entry point validates and returns a
  `Result`.

> **Status: `0.1.0` — feature-complete and conformant, pre-publish.** The public
> API is settled and encoded output is frozen: shards written by any build stay
> readable. Every claim in this README has an entry in
> [`corpus/LEDGER.md`](corpus/LEDGER.md) with the run that produced it.

## Performance (interleaved, pinned, CPU-time)

Measured against **ISA-L's own dispatched `avx2_gfni` assembly** on the same
i7-14650HX, arms alternating ABBA inside one harness so session drift cancels,
work identity gated by parity checksums. Source-bytes basis, best-of-3. Ratios
above 1.00 mean this crate is faster.

| Cell | rusty_erasure | ISA-L | Ratio |
|---|--:|--:|--:|
| S1 encode (4,2) 4 KiB | 59.35 GB/s | 54.46 | **1.09×** |
| S2 encode (10,4) 64 KiB | 25.27 GB/s | 19.02 | **1.33×** |
| S3 encode (10,4) 1 MiB | 21.65 GB/s | 13.22 | **1.64×** |
| S5a encode (17,3) 64 KiB | 30.34 GB/s | 28.74 | **1.06×** |
| S5b encode (20,8) 64 KiB | 10.40 GB/s | 10.51 | 0.99× |
| RAID xor (10) 64 KiB | 89.48 GB/s | 86.60 | **1.03×** |
| RAID pq (10) 64 KiB | 65.37 GB/s | 64.01 | **1.02×** |
| Recover e=1 (10,4) 64 KiB | 64.86 GB/s | 53.11 | **1.22×** |
| Recover e=2 (10,4) 64 KiB | 45.10 GB/s | 35.92 | **1.26×** |

Multi-stripe: **130 GB/s** aggregate across 24 threads (wall basis). Read the
caveats with the numbers: the recovery rows give ISA-L the advantage, amortizing
its matrix work *outside* the timed loop while ours stays *inside*; and ISA-L's
own harness swings 33→11 GB/s across invocations on a single cell, which is why
every number here is best-of-N interleaved rather than a single run.

[`corpus/LEDGER.md`](corpus/LEDGER.md) records what every milestone measured —
including the bricks reverted for being flat or slower, with the reason.

## Correctness evidence

Conformance is **byte identity** with ISA-L's `erasure_code` module, gated
against golden vectors generated from real ISA-L v2.32.1:

- **902/902 full-grid cases** — every Cauchy k=1..32 × p=1..8 at three lengths
  plus every safe-region Vandermonde configuration, replayed through the
  *shipping dispatched path*, each with a parity-requiring recovery.
- **65,536/65,536 GF(2^8) products** and 256/256 inverses against ISA-L's own
  tables, through two independent multiply implementations that must agree.
- **1470/1470 exhaustive loss patterns** on (10,4) — every subset of size 1..=4
  over 14 shards — reconstructed and checked against ground truth.
- **3/3 bidirectional compatibility** with the real `reed-solomon-erasure`
  crate: its shards decode here and ours decode there, so migrating is a
  drop-in with no wire-format break.
- **Every SIMD kernel is byte-identical to the `forbid(unsafe_code)` scalar
  oracle** on x86-64 (SSSE3/AVX2/AVX2-GFNI), aarch64 NEON and wasm SIMD128 —
  the last two executed in CI under qemu and wasmtime, not merely compiled.
- **Miri** (31 tests, zero UB), **ASan fuzzing** across six targets with zero
  open crashers, and **ten Kani proof harnesses** over the arithmetic the
  `// SAFETY:` comments assert.
- **End-to-end on a real vault:** a 730 MB Deputy snapshot sharded 4+2, two
  *data* shards deleted to force both parity shards through decode, restored to
  **4,740/4,740 files SHA-256 identical**.

## Security

Audited against the `use-protection-please` 41-gate hardening standard —
**14 of 16 applicable v1.0.0 gates met**. The two open gates are a calendar and
a tag: H-27 is the 30-day continuous-fuzz soak (the nightly mechanism is live,
the corpus is committed as a floor, zero open crashers today), and H-12 attaches
the SBOM to a release that has not been cut. The gate-by-gate table is at the
bottom of this README.

- **No attack surface of its own.** No sockets, no files, no processes, no
  secrets — the library transforms caller buffers in memory and returns.
- **`unsafe` is confined to one crate.** `cargo geiger` confirms it: the core
  and the facade are 0/0, and every unsafe block in `rusty_erasure-accel`
  carries a stated invariant, mechanically enforced by
  `clippy::undocumented_unsafe_blocks = "deny"` restated in that crate's own
  manifest.
- **Fuzzing that means something on every architecture.** libFuzzer is x86-only,
  so the corpus is embedded at build time and replayed on wasm and aarch64, and
  a seeded generator gives those architectures their own randomized coverage.
- **Erasure parity is not a MAC.** It protects against loss, not tampering. Sign
  or authenticate shards separately if your adversary is malicious rather than
  unlucky.

`unsafe` inventory: [crates/rusty_erasure-accel/UNSAFE.md](crates/rusty_erasure-accel/UNSAFE.md) ·
threat model and reports: [SECURITY.md](SECURITY.md) (72-hour acknowledgement,
90-day coordinated disclosure).

## What is this?

A reimplementation, not a binding. There is no ISA-L in the dependency tree —
the C library appears only as a development-time differential oracle that
generates golden vectors, and is never built into anything shipped.

The published library has **zero third-party dependencies**. Erasure coding is
the primitive under SpaceDB/disco shard placement: k+p shards spread across
peers survive p losses at (k+p)/k overhead instead of 3× replication.

## Features

**Coding** — matrix-flexible Reed-Solomon over GF(2^8)/0x11d: Vandermonde with
ISA-L's safe-region refusal, Cauchy, and the klauspost construction for
byte-compatible migration off `reed-solomon-erasure`. Encode, incremental
single-source update, `verify`, and a `DecodePlan` that amortizes survivor
selection and matrix inversion across stripes.

**RAID-6** — `xor_gen`/`pq_gen` and their checkers, ISA-L semantics but for any
length, including the sub-word tails their base code silently drops.

**Acceleration** — runtime-dispatched SSSE3, AVX2 and AVX2-GFNI on x86-64; NEON
on aarch64; SIMD128 on wasm32. Dispatch resolves once at the surface, never
inside a loop, and an always-on census reports what fraction of production bytes
actually reached the SIMD path.

**Portability** — x86-64, aarch64 and wasm32, across Linux, macOS and Windows.
`no_std + alloc` core; `--no-default-features` is a pure-safe build with no
`unsafe` anywhere in the tree.

## Install

```toml
[dependencies]
rusty_erasure = "0.1"
```

| crate | docs | what |
|---|---|---|
| [`rusty_erasure`](https://crates.io/crates/rusty_erasure) | [docs.rs](https://docs.rs/rusty_erasure) | the typed API and dispatch — start here |
| [`rusty_erasure-core`](https://crates.io/crates/rusty_erasure-core) | [docs.rs](https://docs.rs/rusty_erasure-core) | GF math, matrices, scalar kernels (the oracle) |
| [`rusty_erasure-accel`](https://crates.io/crates/rusty_erasure-accel) | [docs.rs](https://docs.rs/rusty_erasure-accel) | the SIMD kernels — the only unsafe crate |
| [`rusty_erasure-cli`](https://crates.io/crates/rusty_erasure-cli) | [docs.rs](https://docs.rs/rusty_erasure-cli) | `rerasure`: encode / recover / verify / bench |

## Quick start

```rust
use rusty_erasure::{Matrix, coder};

// 4 data shards + 2 parity: survives losing any 2 of the 6.
let c = coder(Matrix::cauchy(4, 2)?)?;

let data: [&[u8]; 4] = [b"shard-one...", b"shard-two...", b"shard-three.", b"shard-four.."];
let len = data[0].len();

let mut parity = vec![vec![0u8; len]; 2];
{
    let mut out: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
    c.encode(&data, &mut out)?;
}

// Lose shard 0 (data) and shard 4 (parity), then rebuild both.
let shards = [
    None, Some(data[1]), Some(data[2]), Some(data[3]),
    None, Some(parity[1].as_slice()),
];
let mut rebuilt = vec![vec![0u8; len]; 2];
{
    let mut out: Vec<&mut [u8]> = rebuilt.iter_mut().map(|b| b.as_mut_slice()).collect();
    c.recover(&shards, &[0, 4], &mut out)?;
}
assert_eq!(rebuilt[0], data[0]);
# Ok::<(), Box<dyn core::error::Error>>(())
```

## Architecture

Four layers, each with one job:

- **`rusty_erasure-core`** — GF(2^8) arithmetic, matrices, and the scalar
  kernels. `no_std + alloc`, zero dependencies, `forbid(unsafe_code)`. These
  kernels are the permanent conformance oracle: every SIMD kernel must equal
  them byte for byte, forever.
- **`rusty_erasure-accel`** — the hand-written SIMD twins, and the workspace's
  only `unsafe`. The GF multiply here is a *different algorithm* from the scalar
  lookup — PSHUFB/TBL/swizzle nibble tables, or a `GF2P8AFFINEQB` affine
  transform — which is why auto-vectorization cannot produce it and why the
  crate exists at all.
- **`rusty_erasure`** — the facade: the typed API, parameter validation, the
  ISA-L-named compat layer, and kernel dispatch resolved once at the surface.
- **`rusty_erasure-cli`** — `rerasure`, consumer #1 of the API: every capability
  is callable by the binary, a test and an agent before it is anything else.

## Benchmarking

```bash
cargo build --release -p rusty_erasure-cli
target/release/rerasure bench --k 10 --p 4 --len 65536 --reps 40000 --kernels auto
target/release/rerasure census   # what fraction of bytes reached the SIMD path
```

No number enters this README or the ledger without a method line: pinned to a
core, process CPU time, arms interleaved ABBA, work identity asserted by a
parity checksum, best-of-N. A keep/revert decided on sequential same-arm runs is
inadmissible here — that rule exists because it caught a "+14%" brick that an
interleaved A/B then measured at 0.92×.

## The Remade With Rust ecosystem

<!-- ORG BOILERPLATE — keep identical across repos -->

**Remade With Rust** is an initiative by **[Mata Network](https://www.mata.network/)**
to rebuild essential C and C++ tools in Rust — for the memory safety, the
predictable performance, and the freedom of a permissive license. Each project
is a reimplementation, not a fork: same wire protocols and file formats, new
code you can actually depend on.

We build the core to production grade and open-source it so the community can
extend it. No copyleft. No surprises. Just the tools we rely on, made faster and
safer.

| Project | What it is |
|---|---|
| 🎬 **[remade_ffmpeg_rs](https://github.com/Remade-With-Rust/remade_ffmpeg_rs)** | **Our FFmpeg alternative.** Drop-in `ffmpeg` and `ffprobe` binaries — demux → decode → filter → encode → mux, rebuilt as composable Rust crates with **zero GPL/LGPL**. Apache-2.0. `rusty_h264` is its H.264 codec. |
| 🧠 **[FFAI](https://github.com/Remade-With-Rust/FFAI)** | **Our sister project: media *for* AI.** "The AI media toolkit, remade with rust." Embedded ASR + TTS (**Mercury**), OCR (**Carmenta**) and vision-language captioning (**Argus**) behind an ffmpeg-style, swap-by-name architecture — no Python, no CUDA. MIT OR Apache-2.0. |
| 🌐 **[Mata Network](https://www.mata.network/)** | **The home page.** *"Stop sacrificing your privacy for convenience."* Sovereign, self-hostable privacy infrastructure — wallet & identity, password manager, contact manager, and a browser extension that stops information leaking as you browse. Remade With Rust is its open-source arm. |

→ All projects: **[github.com/Remade-With-Rust](https://github.com/Remade-With-Rust)**

<!-- /ORG BOILERPLATE -->

## License

MIT OR Apache-2.0, at your option — see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE). No GPL or LGPL anywhere in the tree. The
ISA-L oracle and its golden vectors are development-only, keep their own
licence, and never ship.

## About Mata Network

rusty_erasure is part of the [remade-with-rust](https://github.com/remade-with-rust)
portfolio from [Mata Network](https://www.mata.network/): foundational software
rebuilt in Rust, memory-safe by construction, measured rather than asserted.

---

<!-- HARDENING-TABLE:BEGIN generated by use-protection-please — edit docs/plans/use-protection-please.md, not this block -->
## Hardening status

**Tier** critical-path · **Audited** 2026-08-28 (deep) · **v1.0.0 gates** 14/16 · [Full checklist](docs/plans/use-protection-please.md)

`█████████████████░░░` **89%** &nbsp;·&nbsp; 32 Completed · 1 Scheduled · 3 Incomplete · 5 N/A

| Phase | ✅ Completed | 🗓 Scheduled | ⬜ Incomplete | · N/A |
|---|--:|--:|--:|--:|
| 0 — Threat modeling | 2 | 0 | 0 | 0 |
| 1 — Toolchain | 3 | 0 | 1 | 0 |
| 2 — Supply chain | 7 | 0 | 1 | 0 |
| 3 — Code level | 6 | 0 | 0 | 1 |
| 4 — Static analysis | 1 | 0 | 0 | 0 |
| 5 — Dynamic analysis | 3 | 0 | 0 | 0 |
| 6 — Fuzzing and properties | 3 | 1 | 0 | 0 |
| 7 — Formal verification | 1 | 0 | 0 | 0 |
| 8 — Build and binary | 2 | 0 | 0 | 0 |
| 9 — Runtime privilege | 0 | 0 | 0 | 1 |
| 10 — Cryptography | 0 | 0 | 0 | 3 |
| 11 — CI/CD, release, and operations | 4 | 0 | 1 | 0 |
| **Total** | **32** | **1** | **3** | **5** |

**Next up** — H-27 Continuous fuzzing with no open crashes (Tim Almond — 2026-09-28)

**Architect** — Tim Almond
<!-- HARDENING-TABLE:END -->

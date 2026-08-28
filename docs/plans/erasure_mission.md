# rusty_erasure — Mission Plan

> **Mission:** `rusty_erasure` is Intel ISA-L's erasure coding, remade with Rust. A pure-Rust,
> memory-safe, matrix-flexible GF(2^8) Reed-Solomon engine that matches or beats ISA-L's
> hand-written assembly on encode and recovery throughput — with a safe core, validated
> parameters (ISA-L validates nothing), and one platform ISA-L can never reach: **wasm**, so
> every SpaceDB peer, browser client, and disco edge function can erasure-code shards locally.
>
> Reference target: [intel/isa-l](https://github.com/intel/isa-l) (C + NASM, BSD-3-Clause),
> `erasure_code/` module, v2.32.1.
> Part of the [Remade-With-Rust](https://github.com/remade-with-rust) family.

Status: **M0 in progress** (all §9 decisions ratified 2026-08-28) · Owner: tim.almond@thehouseinc.xyz · Created: 2026-08-28

---

## 1. Why this exists

- **ISA-L is the best-in-class C implementation** — the erasure engine behind Ceph, Hadoop
  HDFS-EC, SPDK, and liberasurecode (OpenStack Swift). It is also C plus a large body of
  hand-written NASM (`gf_{1..6}vect_{dot_prod,mad,mul}_{sse,avx,avx2,avx2_gfni,avx512,avx512_gfni}.asm`),
  whose own README states **"parameters passed to ISA-L functions are not fully validated —
  callers are responsible for ensuring all arguments are valid."** An unvalidated C surface fed
  by untrusted shard metadata is exactly the profile this org exists to delete.
- **The mesh needs erasure coding — and the house is already paying a third party for it.**
  The disco Placement seam is literally specified as *"erasure shards, anti-affinity,
  self-repair"* (building-the-new-internet §3). k+p shards across peers survive p losses at
  (k+p)/k overhead instead of 3× replication — the economics of a data-center-free SpaceDB.
  And it is not hypothetical: **`spacedb-durability` depends on `reed-solomon-erasure = "6"`
  (third-party, maintenance-lagged) today**, and Deputy's vault snapshots
  (`deputy snapshot`/`restore`) ride on it via `encode_snapshot`/`reconstruct_snapshot`.
  rusty_erasure deletes a non-house dependency from the supply-chain tool's own trust path —
  the first consumer swap is already named (M7).
- **wasm is the differentiator.** ISA-L cannot follow us into the browser or an edge function.
  A `wasm32` SIMD128 GF kernel (`i8x16.swizzle` is PSHUFB's twin) means a browser peer can
  encode/verify/repair shards locally — a capability no C erasure library has.
- **The gate is the strongest we have ever had.** GF arithmetic is exact integer math: for the
  same matrix and inputs, output must match ISA-L **byte-for-byte**. No tolerance, no SNR, no
  perceptual metric — every brick in this campaign gates on `assert_eq!`.

### Prior art — stated honestly

| Project | What it is | Our position |
|---|---|---|
| **ISA-L** (C+NASM) | The reference. PSHUFB nibble-table + GFNI kernels, 1–6-row fused dot products, runtime CPUID dispatch. | The baseline every corpus number is measured against, and the conformance oracle. |
| **reed-solomon-erasure** (Rust) | Port of klauspost's Go library; same PSHUFB idea; widely used; maintenance has lagged. | Secondary corpus baseline — and **the incumbent inside our own stack** (`spacedb-durability`). If it ever beats us, that is a stop-and-explain finding — it is the "why not the existing crate" receipt. |
| **reed-solomon-simd** / **novelpoly** (Rust) | Leopard-class O(n log n) FFT codes over GF(2^16). | A **different algorithm family**: superb at very large shard counts, but not matrix-flexible, not ISA-L-shaped, no incremental update. Documented as complementary, benchmarked honestly at the crossover, never claimed beaten on its home turf. |
| **klauspost/reedsolomon** (Go) | The Go engine MinIO uses. | Design input for API ergonomics only; not a corpus arm. |

We must beat or match ISA-L on the corpus, not merely beat the Rust crates — otherwise the
honest answer to "why not wrap ISA-L?" is "no reason," and this repo has no mission.

---

## 2. The one-line test, applied

> Could this ship as-is to a user who assumes their data is theirs alone, onto a machine
> you do not own, with no C toolchain anywhere in the build?

| Concern | Answer |
|---|---|
| C toolchain | **None, anywhere.** No NASM, no `cc`, no `*-sys`. The SIMD twins are `core::arch` intrinsics behind runtime dispatch. ISA-L itself is built ONLY on the Linux CI/bench rig as the conformance oracle and perf baseline; checked-in **golden vectors are data**, so every dev machine gates conformance with zero C. |
| Data | The library holds no state — bytes in, bytes out. Shard persistence belongs to the caller (SpaceDB per-entry law applies **there**, not here). The CLI writes only what it is told to. |
| Identity | N/A for a library. Any service wrapping it authorizes via mID at that service's boundary. |
| Machine you don't own | `rusty_erasure-core` is `no_std + alloc`, `forbid(unsafe_code)`, zero dependencies — it runs on a mesh node, an embedded target, or a browser tab identically. |
| Test rigs are exempt | ISA-L, `reed-solomon-erasure` benches, and perf harnesses live on the rig only. Nothing C ships in any artifact. |

---

## 3. Scope — the ISA-L parity map

ISA-L conventions used throughout: the matrix `a` is `m×k` where `k` = source shards and
`m` = total rows (k + p parity). `gftbls` = 32 bytes per coefficient (`k · p · 32`).

### v1.0 MUST (the erasure_code module, complete)

- **GF(2^8) primitives:** `gf_mul`, `gf_inv`, exhaustively proven against ISA-L (all 65,536
  pairs / 255 inverses — this also pins the field polynomial by construction, no trust required)
- **Matrix generation:** `gf_gen_rs_matrix` (Vandermonde) with ISA-L's documented safe limits
  **enforced as `Result`, not a footnote** (their header: k ≤ 3 any m; k=4 m ≤ 25; k=5 m ≤ 10;
  k ≤ 21 when m−k=4; any k when m−k ≤ 3); `gf_gen_cauchy1_matrix` (any submatrix invertible)
  as the recommended general path; `gf_invert_matrix` with singularity reported, never UB
- **Table expansion:** `ec_init_tables` (32 B/coefficient, bit-identical layout to ISA-L so
  golden vectors can compare tables, not just outputs)
- **Encode:** `ec_encode_data` equivalent — k sources → p parity in one pass, the N-row fused
  dot-product structure (one walk of the data feeds up to 6 parity rows — ISA-L's cache-bandwidth
  play, ours from day one in the scalar design so SIMD twins drop in later)
- **Incremental update:** `ec_encode_data_update` equivalent (`gf_vect_mad` path) — feed one
  source shard at a time; a completed update sequence is **byte-identical** to one-shot encode
- **Recovery:** the decode flow ISA-L leaves as an exercise (invert the surviving-rows
  submatrix, re-encode) shipped as a **first-class validated API** — every loss pattern up to p
- **Kernel-level API:** `gf_vect_dot_prod`, `gf_vect_mad`, `gf_vect_mul` equivalents, public,
  each with its scalar twin as documented oracle
- **Parameter validation everywhere** — the headline safety delta vs ISA-L: dimension checks,
  shard-length agreement, matrix-limit enforcement, no panic on any caller-reachable path
  (parse-constructors returning `Result`; fuzzers prove it from M1)
- **x86-64 SIMD:** SSSE3/AVX2 PSHUFB nibble-table kernels + runtime dispatch + scalar fallback,
  every kernel with a `*_matches_scalar` oracle test and a **reach census** (codec-analyzer
  instrument #7) proving production hits it — the rusty_zstd unwired-kernel defect is a standing
  gate here, not a lesson
- **Arbitrary lengths:** ISA-L's SIMD requires len ≥ 32 (dot_prod) / ≥ 64 (mad); our dispatch
  handles any length ≥ 0 with a scalar tail — strictly wider contract, gated against `_base`

### v1.x SHOULD

- **GFNI/AVX-512 kernels** (`GF2P8AFFINEQB` — one instruction per GF multiply; stable
  `core::arch` support to be verified at M4, kernels land only behind runtime detection)
- **aarch64 NEON** (`vqtbl1q_u8` nibble tables) — per-arch parity or a written note, per the
  codec-vectorize-kernel non-negotiable
- **wasm32 SIMD128** (`i8x16.swizzle`) — the differentiator kernel
- **RAID module:** `xor_gen` / `xor_check` / `pq_gen` / `pq_check` (same shard-walk shape;
  ISA-L's alignment requirements — 32 B pointers, len multiples — relaxed where we can gate it)
- **Multi-stripe parallelism** (rayon, feature-gated, off in core) — the "threads dwarf
  micro-SIMD when units are independent" law; stripes are embarrassingly parallel
- **Streaming/chunked API** for shards larger than memory
- **C-ABI compat crate** (`cdylib` exporting ISA-L-compatible symbols) so C consumers can swap

### NEVER

- igzip/DEFLATE (rusty_zstd's territory), CRC (its own future crate if ever), isa-l_crypto
  (RustCrypto owns that ground)
- GF(2^16) FFT codes — a possible **sibling** crate; bolting a second algorithm family onto
  this API would blur both
- Threading inside the core encode path — parallelism is the caller's (or the feature's) choice,
  never a hidden default

---

## 4. Architecture — the scaffold

Per the house scaffold (building-the-new-internet §2). Names final unless the org objects:

```
rusty_erasure/
├── Cargo.toml                      # workspace; pins live here once; unsafe_code = "deny"
├── docs/plans/erasure_mission.md   # this file
├── crates/
│   ├── rusty_erasure-core/     # LIBRARY. GF(2^8) math, matrices, table init, scalar kernels
│   │                           # (the oracles). no_std + alloc, forbid(unsafe_code), ZERO deps.
│   │                           # A developer who has never heard of MATA can use this crate.
│   ├── rusty_erasure-accel/    # LIBRARY. The unsafe boundary: x86-64 SSSE3/AVX2(/GFNI),
│   │                           # aarch64 NEON, wasm32 SIMD128 twins. Every kernel: scalar
│   │                           # oracle test, SAFETY invariants, runtime feature detection.
│   ├── rusty_erasure/          # LIBRARY. The facade: typed public API, validation, dispatch
│   │                           # (accel if detected + census-proven, scalar otherwise),
│   │                           # ISA-L-named compat layer for porting their tests verbatim.
│   ├── rusty_erasure-cli/      # DELIVERABLE `rerasure`. Allocator declared HERE (seam).
│   └── rusty_erasure-alloc/    # The rusty_alloc seam — one crate, one pin.
├── corpus/                     # §7: golden vectors, configs, runner, LEDGER.md
├── tools/oracle/               # rig-only: ISA-L build script, vector generator, perf arm.
│                               # Nothing here ships; the vectors it emits are checked in.
└── fuzz/                       # cargo-fuzz targets: matrix gen, invert, encode, recover, update
```

Rules the layout enforces: `#[global_allocator]` only in the deliverable; the core knows bytes
and matrices, never products; **every capability is an op** callable by CLI, test, and agent;
`unsafe` exists in exactly one crate, behind safe dispatch that proves bounds before the call.

Design notes the sibling codecs paid for:

- **The scalar core is written in the N-row fused shape from day one** (process one walk of the
  k sources into up to 6 parity accumulators), so the SIMD twin is a widening, not a redesign —
  and so the scalar path itself auto-vectorizes where the baseline ISA allows.
- **Bounds checks are expected to cost ~0** (measured flat on h264, rav1e, dds). The safe core
  is not the performance risk; **layout and call structure are**. We do not lift
  `forbid(unsafe)` in core for speed — the bounds-check-ceiling probe (codec-analyzer #4) will
  price it once, on evidence, expecting to confirm ~0.
- **Dispatch sits at the surface, not in the loop** — the rusty_dds ±86-point lesson: resolve
  the arm once per encode call, `#[target_feature]` functions own the whole loop.
- **`gftbls` layout is bit-identical to ISA-L's** so the oracle can diff intermediate state
  (tables), not just final output — the codec-bringup "compare full state, not output" law.

---

## 5. Ops before buttons — the API surface

Every capability is a typed op before it is a CLI verb. Public API (names indicative):

- `Matrix::reed_solomon(k, p) -> Result<Matrix, MatrixError>` (enforces the Vandermonde limits)
- `Matrix::cauchy(k, p) -> Result<Matrix, MatrixError>`
- `Matrix::invert_for_recovery(&self, surviving: &[usize]) -> Result<Matrix, SingularError>`
- `Coder::new(matrix) -> Coder` (expands `gftbls` once)
- `coder.encode(data: &[&[u8]], parity: &mut [&mut [u8]]) -> Result<(), CodeError>`
- `coder.update(shard_index, data: &[u8], parity: &mut [&mut [u8]]) -> Result<(), CodeError>`
- `coder.recover(present: &[Option<&[u8]>], out: &mut [...]) -> Result<(), RecoverError>`
- `verify(...)` — parity consistency check (the `*_check` shape from the RAID module)
- Kernel layer: `gf::mul`, `gf::inv`, `gf::vect_dot_prod`, `gf::vect_mad` — public, documented,
  each naming its scalar oracle
- Compat layer: `isal::ec_encode_data(...)` etc., signature-shaped so ISA-L's own test vectors
  and any C-ported caller drop in

CLI (`rerasure`) is consumer #1, never the only consumer:

`rerasure encode --k 10 --p 4 [--cauchy] <file>` · `rerasure recover` · `rerasure verify` ·
`rerasure bench [--cell k,p,len]` (prints the §7 method line with every number) ·
`rerasure census` (the kernel-reach census, a first-class user-visible op)

---

## 6. The build-function inventory — every seam, every platform

### 6.0 Portable core (all targets, including wasm — `rusty_erasure-core`)

| Function | Notes |
|---|---|
| `gf::mul / inv` | table-backed; tables `const`-generated at build (no runtime init), proven vs ISA-L exhaustively |
| `matrix::gen_rs / gen_cauchy` | limits enforced; property test: every k-of-m submatrix of Cauchy inverts |
| `matrix::invert` | Gauss-Jordan in GF(2^8); singular → typed error; never reads uninit |
| `tables::init` | ISA-L 32 B/coeff layout, bit-identical |
| `kernel::dot_prod_scalar / mad_scalar / mul_scalar` | THE oracles; stay in-tree forever; branchless inner loops (auto-vec friendly) |
| `encode::encode / update / recover` | N-row fused walk; zero allocations on the hot path (scratch owned by `Coder`) |
| `validate::*` | every public entry; fuzz-proven no-panic |

### 6.1 x86-64 (`rusty_erasure-accel`)

| Kernel | Implementation |
|---|---|
| `dot_prod_ssse3 / avx2` | PSHUFB nibble-split: two 16 B tables per coeff, shuffle+shuffle+XOR; 1..6-row fused variants |
| `mad_ssse3 / avx2` | single-source multiply-accumulate (the update path) |
| `dot_prod_gfni_*` (v1.x) | `GF2P8AFFINEQB` per multiply; land only if stable intrinsics + measured win |
| dispatch | `is_x86_feature_detected!` cached once; alignment-agnostic (`loadu`); scalar tail for len % lane |

### 6.2 aarch64 (v1.x)

| Kernel | Implementation |
|---|---|
| `dot_prod_neon / mad_neon` | `vqtbl1q_u8` nibble tables — same structure, 128-bit lanes; cross-tested in CI |

### 6.3 wasm32 (v1.x)

| Kernel | Implementation |
|---|---|
| `dot_prod_simd128` | `i8x16_swizzle` nibble tables behind the `simd128` target feature; scalar core is the guaranteed fallback |

### 6.4 Cross-cutting build gates (every PR, per workflow doctrine)

```sh
cargo check --target x86_64-unknown-linux-gnu \
            --target x86_64-unknown-linux-musl \
            --target aarch64-unknown-linux-musl \
            --target x86_64-pc-windows-msvc \
            --target aarch64-pc-windows-msvc \
            --target x86_64-apple-darwin \
            --target aarch64-apple-darwin \
            --target wasm32-unknown-unknown
```

- `unsafe_code = "deny"` workspace-wide; lifted ONLY in `rusty_erasure-accel`, each block with
  a `// SAFETY:` invariant and an adversarially-reviewed bounds argument
- `cargo test --no-default-features` in CI — the fallback path must compile and pass everywhere
  (the codec-vectorize-kernel cfg-broke-the-fallback lesson)
- Miri on `rusty_erasure-core`'s test suite; cargo-fuzz targets from M1; Deputy owns the
  lockfile from commit one; `python check-versions.py` before any dependency decision (core
  target: zero deps, so this list should stay embarrassingly short)
- `use-protection-please` audit before v1.0

---

## 7. The corpus — performance vs ISA-L

**ERASCORP v1.** The corpus is the mission's referee: ISA-L is the primary baseline,
`reed-solomon-erasure` secondary. No performance claim leaves this repo — README included —
unless it is in `corpus/LEDGER.md` with the run that produced it (the rusty_zstd refusal
discipline).

### 7.1 Method

- **Conformance before any speed number, and counts before times** (codec-measurement).
  The gate is byte-identity — exact, deterministic, one run, immune to every timing artifact.
  Golden vectors (inputs, matrices, expanded tables, outputs) generated on the rig from real
  ISA-L, checked in as data, diffed on every dev machine and in CI with no C anywhere.
- **The baseline arm uses ISA-L's own harness** (`erasure_code_perf`, their build, their
  defaults) — the tooling its authors trust, the rusty_time/clknetsim analog. Our arm is
  `rerasure bench`. Both arms: same buffers, same (k, p, len) cell, work-count parity asserted
  (bytes in = bytes out on both sides).
- **The measurement bar applies in full:** pinned CPU-time not wall, ABBA-interleaved arms,
  N ≥ 20 pairs with win-rate + z-score, a **null arm** (ISA-L-vs-ISA-L) establishing the noise
  floor every claim must clear, arm durations matched (≥ ~15 s per arm), and every published
  number carries its method line. Sub-1% bricks are judged by deterministic counters
  (kernel calls, bytes down each dispatch path), the clock as confirmation only.
- **The reach census is a standing corpus column:** every run prints % of bytes through each
  kernel per arch. Anything under 100% on the shipped path is a defect, not a statistic.
- **Ceiling probes before optimization bricks** (codec-measurement): stub the GF multiply /
  the table lookup and bound the prize before building any kernel.

### 7.2 Scenarios

| # | Scenario | Config | What it stresses |
|---|---|---|---|
| S1 | Small hot stripe | 4+2, 4 KiB shards | dispatch overhead, L1-resident floor |
| S2 | **Headline cell** | 10+4, 64 KiB | the HDFS-shaped config everyone quotes |
| S3 | Large shard | 10+4, 1 MiB | streaming bandwidth |
| S4 | Cache sweep | 10+4, 4 KiB → 16 MiB | L2/L3 crossing — the cache-boundedness verdict (gates any codec-cache-tiles work) |
| S5 | Wide stripe | 17+3 and 20+8 (Cauchy) | table pressure, 6-row fusion limits |
| S6 | Recovery | S2 with 1, 2, 4 losses | invert + rebuild path, per-loss-pattern |
| S7 | Incremental update | S2, shard-at-a-time | `mad` path vs one-shot re-encode |
| S8 | Tails & alignment | len = 32q+{0..31}, offset pointers | the contract ISA-L doesn't offer |
| S9 | RAID (v1.x) | xor / pq, 8 sources, 64 KiB | the sibling module |
| S10 | Multi-stripe (v1.x) | S2 × N stripes, rayon | scaling; the threads-beat-SIMD law |

### 7.3 Metrics (per cell, per arm)

- **Throughput:** GB/s of source data per core (CPU-time based), and cycles/byte
- **Recovery:** time-to-first-rebuilt-shard incl. matrix inversion; inversion cost separately
- **Determinism:** kernel-reach census %, allocations per call (hot-path target: 0), work counts
- **Footprint:** binary size of `rerasure`, core crate dep count (target 0), RSS during S3

### 7.4 Release gates (v1.0 ships when ERASCORP says)

| Gate | Bar | Status |
|---|---|---|
| G1 conformance | byte-identical vs ISA-L on every legal cell of the config matrix (k ≤ 32, p ≤ 8, Vandermonde within its limits + Cauchy everywhere), all golden vectors, recovery for every loss pattern ≤ p on sampled configs, update-sequence ≡ one-shot | ✅ **MET** (LEDGER full-benchmark): **902/902** full-grid cases through the shipping dispatched path (every Cauchy k=1..32 × p=1..8 × 3 lens + every safe Vandermonde config, per-case recovery spot-checks) on top of the standing 77-vector, 1470-pattern, and exhaustive-GF gates |
| G2 speed vs ISA-L | on the rig, matched ISA arm (AVX2 vs AVX2): ≥ parity on S2; no scenario worse than 1.15×; beat on ≥ half the cells. Honest reporting per cell — parity is a fine v1 story (rusty_alloc precedent), a false "beats" claim is not | ⚠️ **PARTIAL** (LEDGER full-benchmark): beat on S1 1.12× / S2 1.07× / S3 1.34× / large-shard sweep to **1.50×**; S5b 0.92× within bound; **S5a (17,3) 0.80× BREACHES the 1.15× bound** — wide-k pays per-chunk affine reloads the k≤4 register lane doesn't cover; ISA-L's 5/6-dest fusion wins there. Named next brick: k-tiered preload / wider fusion, dispatched by k |
| G3 vs Rust prior art | beat `reed-solomon-erasure` on every cell (the "why not the crate" receipt); publish the honest crossover table vs `reed-solomon-simd`'s FFT family | not started |
| G4 safety | fuzzers clean (matrix/encode/recover/update), Miri clean on core, no panic from any public API on any input, `forbid(unsafe)` in core intact, every accel block SAFETY-reviewed | not started |
| G5 portability | all 8 targets green; NEON + wasm kernels present with per-arch census, or a written note why not | not started |
| G6 reachability | census = 100% of shipped-path bytes through the intended kernel on every arch shipped | not started |
| G7 claims hygiene | README claims ⊆ LEDGER.md, each with its method line | not started |

`corpus/LEDGER.md` records every run: date, commit, cell set, seeds, numbers, method line,
verdict. Wins may be cited; anything not in the ledger does not exist.

---

## 8. Milestones — one brick at a time

| M | Deliverable | Exit test |
|---|---|---|
| M0 ✅ 2026-08-28 | Workspace scaffold, alloc seam, Deputy, CI check-matrix (§6.4), corpus/LEDGER.md stub, this plan merged | **met** — check matrix green on all 8 targets, `--no-default-features` per target included; `rusty_alloc-api 1.1.4` via the seam (version + provenance from check-versions.py); `deputy discover` enumerates 14 pinned deps, all from the alloc seam + binding-only platform crates — core/accel/facade contribute **zero** external deps; git repo initialized, scaffold staged |
| M1 ✅ 2026-08-28 | `rusty_erasure-core` GF math: `gf_mul`/`gf_inv`, both matrix generators with enforced limits, `gf_invert_matrix`, `ec_init_tables`; fuzzers for matrix paths | **met** — see LEDGER.md 2026-08-28: all 65,536 products + 256 inverses byte-identical to ISA-L v2.32.1 golden tables via TWO independent constructions (const log/exp tables and shift-XOR), so the 0x11d polynomial is proven; RS/Cauchy byte-match reference transcriptions (12 configs); safe region enforced + tested both sides; Cauchy submatrix property 7×200 green; `table_mul` proven ∀(c,x); fuzz clean under ASan (4.6M combined execs — Windows needs the VS BuildTools `clang_rt.asan_dynamic-x86_64.dll` dir on PATH, see fuzz/README.md); check matrix still green on all 8 targets. The sweep caught a real defect: `k+p` usize overflow panic, fixed with `checked_add` |
| M2 ✅ 2026-08-28 | Oracle harness: ISA-L built on the rig (`tools/oracle/`), golden-vector generator, vectors checked in; scalar `dot_prod`/`mad` + `encode`/`update`/`recover`; **first ERASCORP conformance run** | **met** — see LEDGER.md 2026-08-28 (M2): rig = WSL Ubuntu gcc, two-file build of unmodified `ec_base.c` + `gen_vectors.c`; 77/77 vector cases byte-identical (gftbls, encode, update≡one-shot — asserted in C against the reference and in Rust with random orders); recovery gated on ground truth incl. **all 1470 loss patterns** on S6's (10,4); `Coder` API validated end-to-end (typed errors, no panics, len-0 valid); roundtrip fuzz target added; check matrix green on all 8 targets. Vectors are `_base`-generated (ISA-L's own suite guarantees SIMD==base; full dispatched build cross-check lands with the M4 perf rig) |
| M3 ✅ 2026-08-28 | Validation + no-panic hardening; the typed public API (§5) + ISA-L compat layer; ISA-L's own test suite ported through the compat layer | **met** (a00d47c; see LEDGER.md M3): compat `isal` module with ISA-L semantics + typed validation; their erasure_code_test.c ported line-for-line incl. the singular-retry decode flow, 7/7 green; Miri clean (31 tests, zero UB/leaks); four fuzz targets cover every public surface, all clean under ASan; scalar baseline in the ledger with full method line — (10,4,64 KiB) 0.477 GB/s best / 0.463 median, flat to 1 MiB (early not-cache-bound signal) |
| M4 ✅ 2026-08-28 | x86-64 SIMD: SSSE3 + AVX2 nibble kernels, 1..6-row fused, runtime dispatch, scalar tails; the reach census instrument; **first ERASCORP perf run vs ISA-L** | **met — bar crushed** (see LEDGER.md M4): pluggable `Kernels` seam (dispatch once at the surface), SSSE3+AVX2 PSHUFB kernels with 4-row fusion, always-on census. Oracle 5/5 both levels; census **100.00%** on the shipping path; **S2 = 0.96× of ISA-L's own AVX2 NASM (parity; exit bar was 1.5×)**, 31.4× over scalar; byte-identical to the dispatched avx2_gfni full build on all measured cells. Remaining gap: GFNI only (0.78× vs dispatched) → M5. Two instrument findings ledgered (mixed table formats caught by checksums; reference-perf drift) |
| M5 ✅ 2026-08-28 | Perf campaign to G2: ceiling probes, redundancy elimination first (codec-optimize order), GFNI/AVX-512 evaluated behind detection | **met — G2 MET** (LEDGER M5 + M5-close): GFNI kernels (affine matrices proven vs ISA-L's own table; exhaustive oracle) then the ceiling-probe-guided register-lane brick (4 KiB **2.05×**, 28.9→59.4 GB/s). Standing interleaved scoreboard vs Intel's dispatched GFNI asm: **S2 1.09× · 1 MiB 1.26× · 4 KiB 1.02× — beat or parity on every cell**. Census 100% on `x86_64/avx2_gfni`. One brick reverted with kind recorded (matrix-preload, measured-worse interleaved; sequential runs declared inadmissible) |
| M6 ✅ 2026-08-28 | aarch64 NEON mirror + wasm32 SIMD128 kernel; `--no-default-features` and cross-target CI | **met** (LEDGER M6): NEON executed under qemu-aarch64 (full suites incl. golden replay + ported ISA-L tests, reach proven), SIMD128 executed under wasmtime (same, reach proven), per-arch census asserted in the portable gate, browser demo Node-verified (4/4 recoveries; 4.2 GB/s in-sandbox; demo checksum == native parity checksum — cross-arch byte-identity witnessed by the deliverable). CI arm64 + wasmtime jobs re-confirm on real silicon at first push (arm runners free on public repos only) |
| M7 ✅ 2026-08-28 | RAID module (`xor`/`pq` + checks), rayon multi-stripe feature, streaming API; **the consumer swap**: `spacedb-durability` drops `reed-solomon-erasure = "6"` for `rusty_erasure` (its seam is two functions — `encode_snapshot`/`reconstruct_snapshot`) | **met** (LEDGER M7a + M7b): RAID **36/36 byte-identical** vs ISA-L (dual-authority vectors); streaming = segment-slicing, proven identical; multi-stripe via std `thread::scope` (zero new deps — deliberate rayon deviation), **130 GB/s aggregate at 24 threads**; `compat::reed_solomon_erasure_matrix` proven byte-compatible both ways vs the real old crate (3/3); spacedb-durability swapped, its 29 tests green; **Deputy e2e on the real 730 MB vault: snapshot → delete 2 DATA shards → restore → 4,740/4,740 files hash-identical**. Old snapshots decode by construction (encode byte-equality). Pending: spacedb/deputy commits in their repos + crates.io publish to replace the path/patch deps |
| v1.0 | Gates G1–G7 green; `use-protection-please` audit; README written FROM the ledger | ship; crates.io publish (core, accel, facade, cli) |

---

## 9. Decisions — all ratified by the owner, 2026-08-28

1. **Naming — RESOLVED:** crate `rusty_erasure`, CLI `rerasure`, underscores (matching
   `rusty_zstd`/`rusty_xml`).
2. **License — RESOLVED:** MIT OR Apache-2.0 dual. Clean-room from the published math
   (Plank/Greenan/Miller PSHUFB technique); ISA-L (BSD-3-Clause) used only as a black-box
   oracle; its generated test vectors are data. No ISA-L source is ever pasted into this repo.
3. **RAID module — RESOLVED:** in-crate module (same shard-walk shape), lands at M7.
4. **C-ABI compat — RESOLVED:** we want the compatibility story. The in-Rust ISA-L-named
   compat layer ships in the facade (M3); the `cdylib` symbol-export crate is post-v1.0,
   revisited at M7.
5. **GFNI/AVX-512 — RESOLVED:** verify stable `core::arch` support at M4; if unstable or the
   rig lacks the silicon, ship AVX2 and record the gap in the ledger (an unmeasured kernel is
   not shipped, per the reach-census law).
6. **Windows dev-loop oracle — RESOLVED:** golden vectors in-repo (conformance gates run with
   zero C anywhere); the perf arm is rig-only (Linux/WSL), and the ledger names the rig per run.

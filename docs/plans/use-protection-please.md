# rusty_erasure — hardening audit

**Standard**: Remade-With-Rust recursive hardening process — see the skill's `STANDARD.md`
**Registry**: 41 gates / 12 phases (`use-protection-please` v1)
**Unit**: `rusty_erasure` (workspace root, audited as one unit — the six crates share one
manifest policy, one CI workflow, one test battery, and are published together)
**Tier**: critical-path — the library consumes shard bytes, lengths, matrix dimensions and
loss patterns supplied by callers that may themselves be handling hostile data (a corrupted
shard store, a malicious peer's stripe). A unit that eats bytes from outside the process is
critical-path regardless of size.
**Mirrors**: none yet — no standalone landing repo, not yet published to crates.io. Both
become mirrors at v1.0 publish and must be rendered from THIS file (SKILL.md §3.1).
**Compliance**: none — the library holds no personal, health, or card data, opens no
sockets, writes no files, and keeps no secrets. Every C-gate is `N/A` for that reason.
**Architect**: Tim Almond
**Audit depth**: deep for supply chain, code level, dynamic analysis and lints (tools run,
outputs below); survey for release/ops gates that need artifacts a v0.1.0 pre-release has
not produced yet.
**Audited**: 2026-08-28 by Claude Fable 5 · **Next review**: 2027-02-28

> Source of truth for this unit's hardening status. The README's status table is
> **generated from this file** — edit here, then run:
> `python <skills>/use-protection-please/scripts/render_readme_table.py --plan docs/plans/use-protection-please.md --readme README.md`

**Status tokens**: `Completed` (evidenced pass) · `Scheduled` (owner + date in Target) ·
`Incomplete` (not done, or not evidenced) · `N/A` (out of tier — reason required in
Evidence; excluded from the totals).

---

## Threat sketch

*Assets* — the integrity of erasure-coded data: parity that byte-matches GF(2^8)/0x11d
Reed-Solomon, and recovery that reconstructs exactly the bytes encoded. SpaceDB durability
and Deputy vault snapshots stake data durability on both.

*Adversaries* — no network adversary reaches this code directly; it opens no sockets, files
or processes and holds no secrets. The adversary is whoever controls the **arguments**:
shard slices, lengths, dimensions, loss patterns — supplied by a caller that may be
processing a corrupted shard store or a malicious peer's stripe.

*Highest-value attack path* — feed dimensions or slice lengths that drive an integer
overflow or an out-of-bounds index inside a SIMD kernel, turning a durability library into
memory corruption in the consumer's process. This is why the whole design puts validation
in front of the kernels, keeps every kernel byte-gated against a `forbid(unsafe_code)`
oracle, and treats a panic on caller input as a defect rather than a diagnostic.

*Full model* — [SECURITY.md](../../SECURITY.md) (STRIDE pass, asset and adversary
statement, residual-risk pointer).

---

## Checklist

`★` = v1.0.0-blocking. Full probe and pass criteria per gate: the skill's `CHECKLIST.md`.

### Phase 0 — Threat modeling

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-01 | ★ Threat model documented and linked from README | Completed | `SECURITY.md` §Threat model — assets, adversaries, STRIDE pass over all six categories, residual-risk pointer; README links it | |
| H-02 | Threat model revisited after last major change | Completed | Written 2026-08-28, same day as the deploy-everywhere and pq/quad-chunk kernel work — the model postdates every source change in the repo | |

### Phase 1 — Toolchain

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-03 | Toolchain pinned (`rust-toolchain.toml`) | Completed | `rust-toolchain.toml`: channel `1.98.0`, components clippy + rustfmt. MSRV floor `rust-version = 1.95` is tracked separately in `[workspace.package]` | |
| H-04 | Committed `.cargo/config.toml` hardening defaults | Incomplete | **Declined on measured evidence, not skipped.** `force-frame-pointers=yes` was implemented and A/B'd: S2 GFNI encode 0.941x with frame pointers vs 0.985x without (interleaved, pinned CPU-time) — the flag alone costs ~4.4% on the crate's headline path, and because `.cargo/config.toml` binds only builds *in this repo* it would make our own published benchmark numbers pessimistic relative to what consumers actually get. Revisit if a profiler need outweighs it | |
| H-05 | ★ Release profile hardened (overflow-checks, LTO, panic policy) | Completed | `[profile.release]`: `overflow-checks = true`, `lto = "thin"`, `codegen-units = 1`; unwind kept deliberately so the typed-error contract holds. Cost measured, not assumed: 0.985x on S2 GFNI (25.81 vs 26.21 GB/s) | |
| H-06 | Security toolchain available to CI and developers | Completed | `.github/workflows/ci.yml` `supply-chain` job installs `cargo-deny@0.19.9` + `cargo-audit@0.22.2` (version-pinned via `taiki-e/install-action`) and runs both on every push and PR; the `miri` job installs the nightly miri component. Locally: cargo-audit, cargo-deny, cargo-geiger, cargo-vet, cargo-careful all present and exercised in this audit | |

### Phase 2 — Supply chain

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-07 | ★ `Cargo.lock` committed | Completed | `git ls-files Cargo.lock` → tracked (and `fuzz/Cargo.lock`) | |
| H-08 | ★ `deny.toml` policy present and enforced | Completed | `deny.toml` covers advisories + licenses + bans + sources; `[bans].deny` mechanizes the no-`*-sys` doctrine (openssl/libz/zstd/lz4/bzip2-sys); `[sources]` denies unknown registries and git. `cargo deny check` → `advisories ok, bans ok, licenses ok, sources ok`, exit 0 | |
| H-09 | ★ Vulnerability scan clean | Completed | `cargo audit --deny warnings` → exit 0 (42 crate deps scanned). Two advisories carry dated written justifications in both `deny.toml` and `.cargo/audit.toml`: RUSTSEC-2024-0384 (`instant` unmaintained) and RUSTSEC-2026-0253 (`lru` unsound) reach the graph ONLY via the test-only dev-dependency `reed-solomon-erasure`; `cargo tree -e normal` shows the shipped tree is five workspace crates plus rusty_alloc. Review 2027-02-28 | |
| H-10 | ★ `cargo vet` coverage complete | Incomplete | cargo-vet installed but no `supply-chain/` directory and no certifications imported. Cheap here — the normal tree is two external crates (`rusty_alloc`, `rusty_alloc-api`), both house-owned | |
| H-11 | Unsafe inventory measured and trending down | Completed | `cargo geiger --all-features` (run from `crates/rusty_erasure`): facade 0/0, `rusty_erasure-core` 0/0, `rusty_erasure-accel` 19 functions / 2048 expressions — the entire unsafe surface is the one crate the architecture designates. Archived in `crates/rusty_erasure-accel/UNSAFE.md` as the first datum of the trend | |
| H-12 | ★ SBOM generated and published with releases | Incomplete | No release artifacts exist yet (v0.1.0, unpublished). Blocked on the first release, not on tooling | |
| H-13 | Git dependencies pinned; no unknown registries or sources | Completed | No `git =` dependencies anywhere; the only `path =` deps are intra-workspace members with matching `version` fields. `deny.toml [sources]` sets `unknown-registry = "deny"` and `unknown-git = "deny"`; `cargo deny check sources` → ok | |
| H-14 | Dependency freshness reviewed, human-in-the-loop updates | Incomplete | No Renovate/Dependabot config. Two external normal deps, both house-owned and pinned; triage is trivial but nothing automates it | |

### Phase 3 — Code level

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-15 | ★ Workspace lint policy set and clean | Completed | `[workspace.lints.rust] unsafe_code = "deny"` + `[workspace.lints.clippy] undocumented_unsafe_blocks = "deny"`, inherited by every crate except the two deliberate unsafe overrides (accel restates the clippy lint itself — see H-16). `cargo clippy --workspace --all-targets -- -D warnings` → **exit 0**. **Audit finding, fixed in this pass**: CI already ran that exact command, so the lint job was RED — 21 warnings across 8 files. Fixed properly (type aliases `EncodeFn`/`UpdateFn`/`RaidKernels`/`Stripe`, `is_multiple_of`, let-chains, doc/literal formatting), never by blanket allow; the four surviving `#[allow]`s are per-function with a written reason (proven-hot kernels whose index shape addresses tables as well as accumulators, and ISA-L-ported test loops kept diffable against the reference). Pedantic/nursery still not configured — the remaining honest gap | |
| H-16 | ★ `unsafe` isolated, SAFETY-commented, inventoried | Completed | `crates/rusty_erasure-accel/UNSAFE.md`: counts per file, the three classes of unsafe and what discharges each, and the oracle battery that backs them. Comment coverage is now **mechanically enforced** — x86 59/59, wasm 13/13, aarch64 7/7. **Audit finding, fixed in this pass**: a `[lints]` override drops the WHOLE workspace table, so accel — the only crate with unsafe — inherited no lint policy at all; restating `undocumented_unsafe_blocks = "deny"` in its own manifest exposed 5 undocumented blocks (4 paired table loads + 1 transmute), all now documented | |
| H-17 | Arithmetic safety explicit | Completed | `checked_add` on the `k + p` dimension path (a real overflow panic found by the no-panic sweep at M1 and fixed); GF arithmetic is `u8` table lookup and XOR — no overflow-capable path; kernel index arithmetic is bounded by asserted slice lengths; `overflow-checks = true` in release makes any missed case a panic, not silent wraparound | |
| H-18 | ★ No `unwrap`/`expect`/panic on untrusted paths; typed errors | Completed | Every public entry point returns `Result` with a typed error (`CodeError`, `MatrixError`, `RecoverError`). Gated by tests, not by grep: `dimension_sweep_never_panics_and_errors_are_typed`, `misuse_is_typed_errors_never_panics`, `misuse_is_an_error_never_a_panic`, `raid_misuse_is_typed_errors` — all green. Remaining `expect(` calls are inside kernels behind already-asserted invariants ("in range", "checked") | |
| H-19 | Input validation — external bytes treated as hostile | Completed | The library has no parser and no deserialization: its inputs are byte slices plus dimensions. Dimensions validated before use (`check_dims`, `checked_add`); every slice length re-asserted at the kernel boundary (`check_encode` and per-kernel `assert_eq!`) before any unchecked access; loss patterns validated in `decode_plan` with `TooManyMissing`. 1470-pattern exhaustive loss sweep plus ~1M-exec ASan fuzzing over arbitrary byte slices | |
| H-20 | ★ Secrets zeroized; never logged | N/A | The library handles no key material or secrets — it is pure GF(2^8) arithmetic over caller buffers, with no crypto, no keys, and no logging of any kind. Consumers encrypt before sharding (compress → encrypt → shard); ciphertext bytes are the caller's to zeroize | |
| H-21 | Concurrency discipline | Completed | No `static mut` anywhere. Shared state is exactly two `AtomicU64` census counters (relaxed adds, one per call) and `OnceLock` dispatch caches. `Coder: Send + Sync` is asserted by test, not assumed, and derived — no manual `unsafe impl Send/Sync` exists in the tree. Multi-thread S10 workload (24 threads, 47 stripes) produces the identical checksum at every thread count | |

### Phase 4 — Static analysis

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-22 | Static analysis beyond the default linter on every PR | Incomplete | CI runs clippy and fmt only — no Semgrep, MIRAI, or Rudra-class pattern rules | |

### Phase 5 — Dynamic analysis

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-23 | ★ Tests pass under Miri | Completed | `cargo +nightly miri test` — **31 tests** executed, zero UB, zero leaks (M3 ledger entry; non-vacuous, count recorded per the gate's own rule) | |
| H-24 | Critical paths pass the sanitizers | Completed | ASan under cargo-fuzz on all three targets: roundtrip 540,959 + 694,740 execs, compat 391,868 + 403,501 execs, matrix_gen 4.13M, matrix_invert 0.50M — all clean (M1–M3 + full-benchmark ledger entries) | |
| H-25 | `cargo careful test` green | Incomplete | cargo-careful installed but not run in this pass | |

### Phase 6 — Fuzzing and properties

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-26 | ★ A fuzz target per public parser, decoder, or message handler | Completed | `fuzz/fuzz_targets/`: roundtrip (encode → lose ≤p shards → recover → assert ground truth), compat (the ISA-L-named API surface), matrix_gen, matrix_invert — covering every untrusted-input entry point the library exposes | |
| H-27 | ★ Continuous fuzzing with no open crashes | Incomplete | Zero open crashers and every past finding fixed (the harness-side infinite loop found at M2 is a bounded Fisher-Yates now), but the coverage is campaign runs totalling ~5.7M execs, **not** ≥30 days of continuous coverage-guided fuzzing and not OSS-Fuzz-enrolled | |
| H-28 | Property tests cover the documented invariants | Completed | Round-trip property (encode → any ≤p losses → recover → ground truth) exhaustively over 1470 loss patterns on (10,4) plus sampled across the (2,2)–(32,8) grid; "never panics on any byte slice" as the no-panic sweeps + fuzz targets; streaming-slicing ≡ one-shot proven for encode and recover; update-sequence ≡ one-shot asserted both in Rust and in C against the reference | |
| H-29 | Mutation and/or differential testing on critical modules | Completed | This crate IS a differential harness against its reference: 902-case full-grid conformance (every Cauchy k=1..32 × p=1..8 and every safe-region Vandermonde config) generated by real ISA-L v2.32.1 and replayed through the shipping dispatched path, plus 77 encode + 36 RAID golden vectors, 65,536 exhaustive GF products, and 3/3 bidirectional conformance vs the real `reed-solomon-erasure` crate. Kernel-vs-scalar-oracle differential runs on every architecture in CI. `cargo mutants` not run | |

### Phase 7 — Formal verification

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-30 | Proof of panic-freedom / UB-freedom per `unsafe` module | Incomplete | No Kani/Creusot/Verus harnesses. The unsafe module's safety argument is currently test-and-oracle-based (per-ISA byte-identity gates + Miri + ASan), which is strong evidence but not a proof | |

### Phase 8 — Build and binary

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-31 | ★ Binary hardening applied and verified | Incomplete | `rerasure` is a developer/bench CLI, not a shipped product binary, and no checksec/mitigation-policy verification has been run on it. Blocks 1.0.0 only for the binary artifact; the library crates are unaffected | |
| H-32 | Build is reproducible or fully auditable | Incomplete | No `cargo auditable` build and no documented reproducible-build procedure. `Cargo.lock` + pinned toolchain make it auditable in principle | |

### Phase 9 — Runtime privilege

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-33 | Least privilege documented and tested | N/A | No daemon, service, or deployment surface — a library plus a local developer CLI. No privileges to drop and no container to constrain | |

### Phase 10 — Cryptography

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-34 | Vetted crypto only; no bespoke primitives | N/A | Contains no cryptography. Erasure coding is an availability primitive, not a confidentiality or authenticity one — GF(2^8) Reed-Solomon parity is a public, deterministic function of public data. `SECURITY.md` states plainly that parity is not a MAC and consumers must sign or MAC shards themselves | |
| H-35 | Side-channel discipline | N/A | No secret-dependent code paths exist because no value in the library is secret. Notably the kernels are *already* data-independent in timing (table lookups and SIMD ops of fixed cost per byte, no data-dependent branching), so a consumer sharding ciphertext gets constant-time behaviour by construction | |
| H-36 | Post-quantum migration plan for long-lived keys | N/A | No keys of any lifetime | |

### Phase 11 — CI/CD, release, and operations

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-37 | CI runs the hardening gate on every PR | Incomplete | `.github/workflows/ci.yml` now runs on push and PR: 8-target check matrix, tests on 4 OSes (including arm64 hardware), wasm under wasmtime, fmt, `clippy -D warnings`, **plus the new `supply-chain` (deny + audit) and `miri` jobs**. Remaining gap: actions are pinned to major tags (`actions/checkout@v4`), not commit SHAs, and there is no scheduled fuzz-regression job | |
| H-38 | Releases signed, attested, and changelogged for security | Incomplete | No releases, no tags, no CHANGELOG yet | |
| H-39 | ★ `SECURITY.md` with a coordinated disclosure process | Completed | `SECURITY.md` at the repo root: contact, 72-hour acknowledgement, 14-day status cadence, 90-day coordinated disclosure, supported-versions statement | |
| H-40 | Advisory monitoring and scheduled re-audit | Incomplete | `cargo audit` is run manually (this pass) against a local advisory-db; no subscription or scheduled job. Next review date recorded in this file's header, but nothing enforces it | |
| H-41 | ★ Residual risks listed and accepted; waivers time-bounded | Completed | Register below: every open risk has an owner, an acceptance, and a review date; both advisory waivers carry a 2027-02-28 expiry | |

### Phase 12 — Compliance controls

Every C-gate (C-01 … C-14) is **`N/A`**: no framework is in scope. The unit processes no
personal, health, or cardholder data, makes no network egress, persists nothing, and holds
no keys — it transforms caller-owned byte buffers in memory and returns.

---

## Residual risk register

| Risk | Impact | Acceptance | Owner | Review |
|---|---|---|---|---|
| Two RUSTSEC advisories in the **dev-only** oracle tree (`instant` unmaintained, `lru` unsound) via `reed-solomon-erasure` | None on shipped artifacts — dev-dependency only, verified by `cargo tree -e normal` | Accepted; waived in `deny.toml` and `.cargo/audit.toml` with dated reasons. Expires 2027-02-28 or immediately on any shipped-tree path | Tim Almond | 2027-02-28 |
| Unsafe safety argument is test-and-oracle-based, not formally proven (H-30) | A memory-safety bug that changes no output byte would evade the byte-identity oracles | Accepted for 0.x: ASan fuzzing and Miri cover exactly that gap, and both are green. Kani harnesses are the named 1.0 follow-up | Tim Almond | 2027-02-28 |
| Fuzzing is campaign-based (~5.7M execs), not continuous ≥30 days (H-27) | Lower probability of finding deep-state bugs than sustained fuzzing | Accepted for 0.x; OSS-Fuzz enrolment or a scheduled CI fuzz job is the 1.0 follow-up | Tim Almond | 2027-02-28 |
| CI actions pinned to major tags, not commit SHAs (H-37) | A compromised or retagged action version could execute in CI | Accepted: all actions are first-party GitHub or widely-used publishers; SHA pinning is the follow-up | Tim Almond | 2027-02-28 |
| Frame pointers declined for measured perf (H-04) | Production stack walking through the kernels is harder for profilers | Accepted: measured 4.4% cost on the headline encode path, and the flag would only bind this repo's own builds, skewing our published numbers | Tim Almond | 2027-02-28 |

---

## v1.0.0 readiness

**17 ★ gates. 12 Completed, 1 N/A, 4 Incomplete** — so 1.0.0 is **blocked** on:

| Gate | What is missing | Shape of the work |
|---|---|---|
| H-10 | `cargo vet` certifications | Hours — the normal tree is two external crates, both house-owned |
| H-12 | SBOM published with a release | Blocked on there being a release, not on tooling |
| H-27 | ≥30 days continuous fuzzing, no open crashers | Calendar time; a scheduled CI fuzz job starts the clock |
| H-31 | Binary hardening verified for the `rerasure` CLI | One checksec/mitigation-policy run per shipped artifact |

None of the four is a design problem: two are calendar or release-gated, two are hours of
mechanical work. The proposed order (owner and dates are the human step, deliberately left
blank): (1) `cargo vet` certifications — closes ★ H-10; (2) a scheduled CI fuzz job to
start the ★ H-27 clock; (3) at first release, `cargo auditable` + SBOM + signed tag +
CHANGELOG + the binary-hardening check — closes ★ H-12, ★ H-31, H-32 and H-38 together;
(4) pedantic/nursery clippy tiers, SHA-pinned actions, Kani harnesses (H-30) as the
deeper follow-ups.

---

## Audit log

- **2026-08-28** — First pass, deep depth on supply chain / code level / dynamic analysis /
  lints. Tools actually run: `cargo deny check` (exit 0 after two justified waivers),
  `cargo audit --deny warnings` (exit 0), `cargo geiger --all-features`, `cargo clippy
  --workspace --all-targets`. Artifacts created: `SECURITY.md`, `deny.toml`,
  `.cargo/audit.toml`, `rust-toolchain.toml`, `crates/rusty_erasure-accel/UNSAFE.md`, this
  file. Manifest changes: `overflow-checks = true` (perf-gated at 0.985x),
  `[workspace.lints.clippy] undocumented_unsafe_blocks = "deny"`. **Findings fixed in the
  pass**: (1) `rusty_erasure-accel` — the only crate containing `unsafe` — inherited NO
  workspace lint policy, because a `[lints]` override replaces the whole table rather than
  merging; restating the lint in its manifest exposed and fixed 5 undocumented unsafe
  blocks. (2) `force-frame-pointers` was implemented, measured at a 4.4% cost on the
  headline encode path, and declined on the evidence rather than shipped unmeasured.
  (3) The CI lint job was **already red**: it runs `clippy --workspace --all-targets -D
  warnings` and 21 warnings had accumulated across 8 files. Fixed at the source (type
  aliases, `is_multiple_of`, let-chains, doc and literal formatting) rather than by
  blanket allow; the four remaining `#[allow]`s are per-function and carry written
  reasons. Gates closed in-pass by this work: ★ H-15, plus H-06 and part of H-37 via the
  new `supply-chain` and `miri` CI jobs.

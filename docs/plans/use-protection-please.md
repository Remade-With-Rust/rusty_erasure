# rusty_erasure — hardening audit

**Standard**: Remade-With-Rust recursive hardening process — see the skill's `STANDARD.md`
**Registry**: 41 gates / 12 phases (`use-protection-please` v1)
**Unit**: `rusty_erasure` (workspace root, audited as one unit — the six crates share one
manifest policy, one CI workflow, one test battery, and are published together)
**Tier**: critical-path — the library consumes shard bytes, lengths, matrix dimensions and
loss patterns supplied by callers that may themselves be handling hostile data (a corrupted
shard store, a malicious peer's stripe). A unit that eats bytes from outside the process is
critical-path regardless of size.
**Mirrors**: the five crates.io pages — `rusty_erasure`, `-core`, `-accel`, `-cli`,
`-alloc` (published 2026-08-28). All five set `readme = "../../README.md"`, so they render
the SAME block from THIS file and cannot drift (SKILL.md §3.1); a re-render plus the next
`cargo publish` updates every one. No standalone landing repo yet.
**Compliance**: none — the library holds no personal, health, or card data, opens no
sockets, writes no files, and keeps no secrets. Every C-gate is `N/A` for that reason.
**Architect**: Tim Almond
**Audit depth**: deep for supply chain, code level, dynamic analysis and lints (tools run,
outputs below); survey for release/ops gates that need artifacts a v0.1.0 pre-release has
not produced yet.
**Audited**: 2026-08-28 by Claude Fable 5 (re-audited same day after the ★-gate pass) ·
**Next review**: 2027-02-28

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
| H-04 | Committed `.cargo/config.toml` hardening defaults | Incomplete | **A measured decision, not an oversight — and the outcome the gate wants is achieved elsewhere.** `force-frame-pointers=yes` was implemented and A/B'd: S2 GFNI encode 0.941× with it vs 0.985× without (interleaved, pinned CPU-time) — ~4.4% on the crate's headline path. Since `.cargo/config.toml` binds only builds *in this repo*, it would also make our published benchmark numbers pessimistic relative to what consumers get. Declined. The remaining hardening flags the gate names are already satisfied without it: PIE, full RELRO and non-executable stack come from the toolchain defaults and are **verified on the real binary** by H-31, and Windows Control Flow Guard is applied to the shipped artifact in `release.yml` (deliberately not repo-wide, for the same measurement reason). `.cargo/audit.toml` is committed. Recorded Incomplete rather than Completed because the literal artifact the gate asks for is absent — evidence over vibes | |
| H-05 | ★ Release profile hardened (overflow-checks, LTO, panic policy) | Completed | `[profile.release]`: `overflow-checks = true`, `lto = "thin"`, `codegen-units = 1`; unwind kept deliberately so the typed-error contract holds. Cost measured, not assumed: 0.985x on S2 GFNI (25.81 vs 26.21 GB/s) | |
| H-06 | Security toolchain available to CI and developers | Completed | `.github/workflows/ci.yml` `supply-chain` job installs `cargo-deny@0.19.9` + `cargo-audit@0.22.2` (version-pinned via `taiki-e/install-action`) and runs both on every push and PR; the `miri` job installs the nightly miri component. Locally: cargo-audit, cargo-deny, cargo-geiger, cargo-vet, cargo-careful all present and exercised in this audit | |

### Phase 2 — Supply chain

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-07 | ★ `Cargo.lock` committed | Completed | `git ls-files Cargo.lock` → tracked (and `fuzz/Cargo.lock`) | |
| H-08 | ★ `deny.toml` policy present and enforced | Completed | `deny.toml` covers advisories + licenses + bans + sources; `[bans].deny` mechanizes the no-`*-sys` doctrine (openssl/libz/zstd/lz4/bzip2-sys); `[sources]` denies unknown registries and git. `cargo deny check` → `advisories ok, bans ok, licenses ok, sources ok`, exit 0 | |
| H-09 | ★ Vulnerability scan clean | Completed | `cargo audit --deny warnings` → exit 0 (42 crate deps scanned). Two advisories carry dated written justifications in both `deny.toml` and `.cargo/audit.toml`: RUSTSEC-2024-0384 (`instant` unmaintained) and RUSTSEC-2026-0253 (`lru` unsound) reach the graph ONLY via the test-only dev-dependency `reed-solomon-erasure`; `cargo tree -e normal` shows the shipped tree is five workspace crates plus rusty_alloc. Review 2027-02-28 | |
| H-10 | ★ `cargo vet` coverage complete | Completed | `cargo vet check` → **Vetting Succeeded (34 fully audited, 2 exempted)**, enforced per-push by the CI `vet` job. Built by importing the mozilla / google / isrg / zcash registries and then establishing trust in publishers those registries already trust (kennykerr for the `windows_*` family, rust-lang-owner for `libc`/`libm`, Amanieu, alexcrichton, newpavlov, …), plus our own org's publisher for `rusty_alloc`/`rusty_alloc-api`. **Every `safe-to-deploy` crate — the entire shipped tree — is certified.** The 2 exemptions are `winapi-{i686,x86_64}-pc-windows-gnu`, which predate crates.io publisher tracking (publisher UNKNOWN, so no trust relation is expressible); both are dev-only AND are the `*-windows-gnu` import stubs, never compiled on any target we ship. Documented and dated in `supply-chain/config.toml`. Structural note recorded during the audit: `cargo tree -p rusty_erasure -e normal` shows **the published library has zero third-party dependencies** — `libc`/`windows-*` enter only through the CLI's allocator seam | |
| H-11 | Unsafe inventory measured and trending down | Completed | `cargo geiger --all-features` (run from `crates/rusty_erasure`): facade 0/0, `rusty_erasure-core` 0/0, `rusty_erasure-accel` 19 functions / 2048 expressions — the entire unsafe surface is the one crate the architecture designates. Archived in `crates/rusty_erasure-accel/UNSAFE.md` as the first datum of the trend | |
| H-12 | ★ SBOM generated and published with releases | Incomplete | No release artifacts exist yet (v0.1.0, unpublished). Blocked on the first release, not on tooling | |
| H-13 | Git dependencies pinned; no unknown registries or sources | Completed | No `git =` dependencies anywhere; the only `path =` deps are intra-workspace members with matching `version` fields. `deny.toml [sources]` sets `unknown-registry = "deny"` and `unknown-git = "deny"`; `cargo deny check sources` → ok | |
| H-14 | Dependency freshness reviewed, human-in-the-loop updates | Completed | `.github/dependabot.yml` watches three ecosystems — cargo (weekly), the separate `/fuzz` workspace (monthly), and **github-actions** (weekly, the higher-value half, since a compromised action runs in CI with our token). Nothing auto-merges: every bump arrives as a PR and must pass deny/audit/vet/clippy/Miri/careful/semgrep/the 8-target matrix/the cross-arch replay. Triage performed this pass with `cargo outdated`: exactly one update available (`rusty_alloc-api` 1.1.4 → 1.1.6), **taken** — the org's own guidance is that earlier `rusty_alloc` releases carried use-after-frees, so newer is the safer direction — and verified against the full gate set (vet, deny, audit, 26/26 suites) before keeping | |

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
| H-22 | Static analysis beyond the default linter on every PR | Completed | `tools/semgrep-rules.yml` + the CI `static-analysis` job. Generic Rust rulesets mostly restate clippy, so these encode **this codebase's structural laws** — the invariants a reviewer could break with no lint noticing: `unsafe` outside the one sanctioned crate; `#[global_allocator]` in a library; kernel dispatch inside a loop; nibble tables fed to a GFNI kernel (the M4 wrong-parity-at-plausible-speed defect); parity documented as authentication. **The job runs the ruleset twice**: clean over the real tree (19 files, 0 findings) and against `tools/semgrep-selftest/`, where all 5 rules must still fire. That second pass is load-bearing — it immediately caught 4 of 5 rules silently not matching (path globs excluded the fixture, and `pattern: "#[global_allocator]"` bound to every expression, giving 2160 findings on a clean tree). A rule that cannot fire reports safety it is not checking | |

### Phase 5 — Dynamic analysis

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-23 | ★ Tests pass under Miri | Completed | `cargo +nightly miri test` — **31 tests** executed, zero UB, zero leaks (M3 ledger entry; non-vacuous, count recorded per the gate's own rule) | |
| H-24 | Critical paths pass the sanitizers | Completed | ASan under cargo-fuzz across all six targets, all clean. Historic: roundtrip 540,959 + 694,740, compat 391,868 + 403,501, matrix_gen 4.13M, matrix_invert 0.50M. This pass added the two kernel-facing targets — **raid 3,214,440 execs, zero crashes**, coverage saturated (cov 279 / ft 893, only 136 new units over 3.2M runs) — plus a fresh campaign on the older four. The `kernels` target runs every SIMD level against the scalar oracle under ASan, which is the configuration that can catch an over-read that changes no output byte | |
| H-25 | `cargo careful test` green | Completed | `cargo +nightly careful test --workspace` → **exit 0, 16 suites**, over a std built with extra debug assertions and uninitialised-memory checks — a fault class the byte-identity oracles structurally cannot see. Wired into CI as the `careful` job so it runs per-PR | |

### Phase 6 — Fuzzing and properties

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-26 | ★ A fuzz target per public parser, decoder, or message handler | Completed | **Six** targets in `fuzz/fuzz_targets/`, covering every untrusted-input entry point: roundtrip (encode → lose ≤p shards → recover → assert ground truth, now also asserting dispatched == scalar), compat (the ISA-L-named API surface), matrix_gen, matrix_invert, and two added in this pass — **`raid`** (differential xor/pq vs the scalar core, plus checker-detects-a-flipped-bit) and **`kernels`** (every SIMD level this CPU exposes, differentially vs the scalar oracle, over encode/mad/update). The two new ones close a real gap: the RAID and quad-chunk kernels were the newest unsafe code in the tree and had no fuzz coverage at all. Bodies live in `crates/rusty_erasure-fuzzlib` so the libFuzzer targets and the cross-architecture replay run identical code | |
| H-27 | ★ Continuous fuzzing with no open crashes | Scheduled | **Zero open crashers** (`fuzz/artifacts/` empty for all six targets), and every past finding is a regression test — `past_findings_replay_without_hanging` replays the M2 timeout's whole seed space. Continuous fuzzing is now actually running rather than proposed: `.github/workflows/fuzz.yml` runs nightly, one job per target, **persisting the corpus across runs via the actions cache** so each night resumes from the accumulated coverage instead of cold-starting; a crash fails the job and uploads the reproducer. The ≥30-day criterion is calendar time from the workflow's first scheduled run. **Cross-architecture coverage is the part that is already complete**, in two layers: (1) `corpus_replay` re-executes all 1,361 corpus inputs plus the historical reproducer on wasm32 (wasmtime, +simd128) and aarch64 (qemu), so x86 findings become regression cases for the SIMD128 and NEON kernels libFuzzer cannot reach; (2) `seeded_random_sweep_never_panics` gives those architectures their OWN randomized coverage — corpus replay alone under-samples them, because libFuzzer's corpus is shaped by coverage feedback from the AVX2/GFNI kernels. Measured this pass: **240,000 generated inputs per architecture (40,000 × 6 targets), clean on wasm32 and aarch64**, scaled to 200,000/target in the nightly job | Tim Almond — 2026-09-28 |
| H-28 | Property tests cover the documented invariants | Completed | Plus, added this pass: `short_and_degenerate_inputs_never_panic` sweeps all six harness bodies over every length 0..40 × five byte patterns + a counting pattern — the "never panics on any byte slice" invariant as a deterministic test rather than only a fuzzing artifact. Prior evidence: | Round-trip property (encode → any ≤p losses → recover → ground truth) exhaustively over 1470 loss patterns on (10,4) plus sampled across the (2,2)–(32,8) grid; "never panics on any byte slice" as the no-panic sweeps + fuzz targets; streaming-slicing ≡ one-shot proven for encode and recover; update-sequence ≡ one-shot asserted both in Rust and in C against the reference | |
| H-29 | Mutation and/or differential testing on critical modules | Completed | This crate IS a differential harness against its reference: 902-case full-grid conformance (every Cauchy k=1..32 × p=1..8 and every safe-region Vandermonde config) generated by real ISA-L v2.32.1 and replayed through the shipping dispatched path, plus 77 encode + 36 RAID golden vectors, 65,536 exhaustive GF products, and 3/3 bidirectional conformance vs the real `reed-solomon-erasure` crate. Kernel-vs-scalar-oracle differential runs on every architecture in CI. `cargo mutants` not run | |

### Phase 7 — Formal verification

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-30 | Proof of panic-freedom / UB-freedom per `unsafe` module | Completed | Ten Kani harnesses in `crates/rusty_erasure-core/src/proofs.rs`, run by the CI `kani` job. **Scope stated honestly**: Kani has no model for `core::arch` intrinsics, so the kernel bodies cannot be symbolically executed — what is proven is the arithmetic the `// SAFETY:` comments actually assert, which is where the bounds argument lives. Proven: GF multiplication commutes; 1 is the identity and 0 annihilates; every nonzero element's inverse really inverts; the table and shift-XOR multiplies agree on all 65,536 pairs; the dimension check cannot overflow over the whole `usize` domain (the proof form of the M1 defect); the **nibble and affine table offsets `(r*k + j) * BYTES` plus their full read width stay inside the `rows * k * BYTES` region the safe wrappers assert**, over the shipped config grid (k ≤ 32, rows ≤ 8 — bounded because `r * k` is nonlinear, not for range reasons); and the chunked-loop guard admits no iteration that reads past the end, for every unroll tier (16/32/64/128). The remaining layers cover what proof cannot: byte-identity oracles for output equality on every architecture, ASan and Miri for runtime memory faults | |

### Phase 8 — Build and binary

| ID | Gate | Status | Evidence | Target |
|---|---|---|---|---|
| H-31 | ★ Binary hardening applied and verified | Completed | `tools/verify_hardening.sh` checks the real ELF and **all five pass** on `rerasure`: PIE (type DYN), GNU_RELRO, BIND_NOW (full RELRO), non-executable stack, and the `.dep-v0` auditable dependency section. Windows verified with `dumpbin /headers`: Dynamic base, High Entropy Virtual Addresses, NX compatible — plus Control Flow Guard, applied to the shipped binary in `release.yml` only (it instruments indirect calls, and the kernel-dispatch seam is exactly that, so it is deliberately kept out of library and benchmark builds). Enforced by the CI `binary-hardening` job on every push, not just at release, so a linker-flag change cannot silently un-harden it | |
| H-32 | Build is reproducible or fully auditable | Completed | Release artifacts are built with `cargo auditable` (`release.yml`), embedding the full dependency list in the binary — verified present as the `.dep-v0` ELF section by `tools/verify_hardening.sh`, so a shipped binary can be advisory-scanned without its source tree. Combined with the committed `Cargo.lock` and the pinned `rust-toolchain.toml`, the build is fully auditable | |

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
| H-37 | CI runs the hardening gate on every PR | Completed | Per-PR: fmt, `clippy -D warnings`, tests on 4 OSes (including real arm64), the 8-target check matrix, wasm under wasmtime, `supply-chain` (deny + audit), `vet`, `miri`, `careful`, `static-analysis`, `kani`, and `binary-hardening`. Fuzz regression is per-PR too — `corpus_replay` runs inside `cargo test --workspace`, replaying the whole corpus plus every past reproducer. Continuous fuzzing itself is the documented nightly schedule (`fuzz.yml`). **Every action is now SHA-pinned** with the tag in a trailing comment, so a retagged or compromised release cannot execute in CI. Tokens are least-privilege: `ci.yml` declares `permissions: contents: read` at the workflow level, and the two workflows needing more (release attestations, advisory issues) request it locally | |
| H-38 | Releases signed, attested, and changelogged for security | Incomplete | `CHANGELOG.md` now exists with an explicit **Security** section per release, and `release.yml` attaches build provenance (`actions/attest-build-provenance`) plus SHA256SUMS to every artifact. Still missing, and the reason this stays Incomplete: no release has been cut, and signed tags are the owner's key — the workflow cannot sign on their behalf | |
| H-39 | ★ `SECURITY.md` with a coordinated disclosure process | Completed | `SECURITY.md` at the repo root: contact, 72-hour acknowledgement, 14-day status cadence, 90-day coordinated disclosure, supported-versions statement | |
| H-40 | Advisory monitoring and scheduled re-audit | Completed | `.github/workflows/advisories.yml` runs `cargo audit --deny warnings` and `cargo deny check advisories` **daily**, and **opens a labelled issue** on a finding rather than only failing a run nobody reads. The per-PR `supply-chain` job cannot cover this: advisories are published against code that has not changed, so a PR-triggered workflow would never notice. A second cron fires quarterly with the re-audit checklist, so this plan's "Next review" date is enforced by a job rather than by memory | |
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
| Kani cannot model `core::arch` SIMD intrinsics, so the kernel bodies themselves are not symbolically executed (H-30) | A fault inside an intrinsic sequence is not reached by proof | Accepted, and deliberately layered: the proofs cover the *arithmetic the `// SAFETY:` comments assert* (table offsets, loop bounds) over the whole symbolic domain; byte-identity oracles cover output equality on every architecture; ASan and Miri cover runtime memory faults. Each layer covers what the others structurally cannot | Tim Almond | 2027-02-28 |
| Continuous fuzzing has started but has not yet run 30 days (H-27) | Less deep-state coverage than sustained fuzzing gives | Accepted: the nightly workflow is live and corpus-persistent, zero open crashers, and every discovered input already replays on all three architectures. OSS-Fuzz enrolment remains the stronger option if the crate's exposure grows | Tim Almond | 2026-09-28 |
| CI actions pinned to major tags, not commit SHAs (H-37) | A compromised or retagged action version could execute in CI | Accepted: all actions are first-party GitHub or widely-used publishers; SHA pinning is the follow-up | Tim Almond | 2027-02-28 |
| Frame pointers declined for measured perf (H-04) | Production stack walking through the kernels is harder for profilers | Accepted: measured 4.4% cost on the headline encode path, and the flag would only bind this repo's own builds, skewing our published numbers | Tim Almond | 2027-02-28 |

---

## v1.0.0 readiness

**17 ★ gates. 14 Completed, 1 Scheduled, 1 N/A, 1 Incomplete** (the renderer shows 14/16, excluding the N/A). Nothing is left that
requires a decision or a design change — the two open items are a calendar and a tag:

| Gate | Status | What remains |
|---|---|---|
| H-27 | Scheduled (2026-09-28) | Calendar only. The nightly fuzz workflow is committed and accumulates corpus across runs; 30 days of it is the criterion. Zero open crashers today |
| H-12 | Incomplete | One `git tag` away. `release.yml` builds auditable binaries, generates the CycloneDX SBOM, verifies hardening and attaches provenance — it just needs a release to attach them to, and cutting a 1.0.0 tag is the owner's call |

Everything else in the registry that can be closed without a release or a calendar now is
closed. The three rows left open outside the ★ set are each a stated decision rather than
an omission: **H-04** (frame pointers declined on a 4.4% measurement, with the hardening
outcome verified instead by H-31), **H-38** (release signing needs the owner's key), and
**H-12** (needs a tag to attach the SBOM to). Remaining genuine follow-ups: pedantic and
nursery clippy tiers (H-15's other half), and extending the Kani harnesses as Kani's
support for `core::arch` improves — today the intrinsic bodies are out of proof's reach
and are covered by the oracle, ASan and Miri layers instead.

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
- **2026-08-28 (second pass, the ★-gate sweep)** — Closed ★ H-10, ★ H-31 and H-32;
  advanced ★ H-27 from Incomplete to Scheduled with the clock running; left ★ H-12
  release-gated. Tools run: `cargo vet check` (34 audited / 2 exempted),
  `tools/verify_hardening.sh` (5/5 on the real ELF), `dumpbin /headers` (Windows
  mitigations), `cargo auditable build`, `cargo cyclonedx`, and fuzz campaigns totalling
  **7.5M+ executions across six targets with zero crashes**. Added: two fuzz targets
  (`raid`, `kernels`) for the newest unsafe code, `crates/rusty_erasure-fuzzlib` holding
  the shared harness bodies, the cross-architecture corpus replay (1,300+ inputs on
  native + wasm + aarch64), `.github/workflows/fuzz.yml` (nightly, corpus-persistent),
  `.github/workflows/release.yml`, `tools/verify_hardening.sh`, and CI jobs for vet and
  binary hardening. **Findings in this pass**: (1) `cargo vet init` writes blanket
  *exemptions*, which satisfy `cargo vet check` while certifying nothing — the initial
  "Vetting Succeeded (36 exempted)" was vacuous and had to be rebuilt from real trust
  relationships. (2) The corpus replay's first design used `include_bytes!` against live
  corpus paths and broke a build mid-campaign, because libFuzzer's REDUCE pass deletes
  inputs while it runs; fixed by snapshotting into `OUT_DIR`. (3) Pinning the toolchain
  in the previous pass silently broke the cross-target dev loop — the aarch64 and wasm
  stds had to be installed for 1.98.0, and a stale aarch64 std produced "invalid metadata
  for crate core" until removed by hand.
- **2026-08-28 (third pass, closing the workable remainder)** — Closed H-14, H-22, H-25,
  H-37, H-40 and ★ H-30. Tools run: `cargo careful test --workspace` (exit 0, 16 suites),
  `cargo outdated` (one update, taken and gated), `semgrep` (19 files clean; 5/5 rules
  verified live), `cargo kani` (10 harnesses, all verified). Added: `.github/dependabot.yml`,
  `.github/workflows/advisories.yml`, `tools/semgrep-rules.yml` +
  `tools/semgrep-selftest/` + `tools/run_semgrep.sh`,
  `crates/rusty_erasure-core/src/proofs.rs`, CI jobs for static analysis, careful and
  kani, and SHA pins for every action. **Findings in this pass**: (1) the semgrep
  self-test caught **4 of 5 rules silently not matching** on its first run — path globs
  excluded the fixture and an attribute pattern bound to every expression (2160 findings
  on a clean tree). This is why the ruleset is run twice; a rule that cannot fire reports
  safety it is not checking. (2) **Kani found an overflow inside a proof harness** —
  `assume(i + step <= n)` with unconstrained `i` overflows in the assumption itself. A
  specification can carry the exact bug it exists to rule out. (3) Registering
  `cfg(kani)` had to go in `[workspace.lints.rust]`, not the crate's own table: the same
  override trap that had left the accel crate unlinted in pass one.

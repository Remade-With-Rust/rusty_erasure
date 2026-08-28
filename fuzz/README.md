# Fuzz targets

Six targets, all asserting full invariants rather than merely "no crash":

| Target | What it proves |
|---|---|
| `roundtrip` | encode → lose ≤p shards → recover reconstructs ground truth exactly; dispatched encode == scalar oracle |
| `compat` | every `isal::*` entry point returns a typed error or succeeds on arbitrary parameters and undersized buffers — never panics |
| `matrix_gen` | matrix generation never panics for any `(k, p)` |
| `matrix_invert` | inversion never panics, and a reported inverse really is one |
| `raid` | dispatched `xor_gen`/`pq_gen` are byte-identical to the scalar core, the checkers accept their own output, and `pq_check` detects a single flipped bit |
| `kernels` | every SIMD level this CPU exposes agrees with the scalar oracle on encode, mad and the fused update |

```powershell
cargo +nightly fuzz run raid -- -max_total_time=60
```

## The bodies live in the workspace, not here

Each file in `fuzz_targets/` is a three-line delegate to
`crates/rusty_erasure-fuzzlib`. That crate holds the actual harness bodies so
two consumers can run **identical** code:

1. these libFuzzer targets, with ASan, on x86-64;
2. `corpus_replay`, which re-executes the whole corpus on **wasm32 under
   wasmtime** and **aarch64 under qemu**.

The second exists because libFuzzer only runs on x86-64. Without it, a fuzz
finding would say nothing about the SIMD128 and NEON kernels — different
unsafe code, different unroll boundaries, reached by the same inputs. With it,
every input the fuzzer keeps becomes a regression case on every architecture
we ship, automatically.

## The corpus is committed on purpose

`corpus/` and `artifacts/` are **not** gitignored. The corpus is the seed set
the nightly workflow resumes from and the input to the replay gate — ignoring
it would make that gate vacuous on a fresh clone, CI included. `artifacts/`
holds past reproducers, so replaying them keeps "every past crasher is a
regression test" true without anyone having to remember.

Keep them small: `cargo +nightly fuzz cmin <target>` reduces a corpus to the
minimal set that preserves coverage. Run it before committing a campaign's
output.

## Continuous fuzzing

`.github/workflows/fuzz.yml` runs every target nightly and persists each
corpus through the actions cache, so runs accumulate coverage instead of cold
-starting. A crash fails the job and uploads the reproducer.

## Windows note

cargo-fuzz links the ASan runtime but the DLL is not on PATH by default
(`STATUS_DLL_NOT_FOUND` at startup). Prepend the MSVC host tools directory:

```powershell
$env:PATH = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64;$env:PATH"
```

(`--sanitizer none` does NOT work on the MSVC target — libfuzzer-sys then
fails to link against the sancov section symbols.)

The deterministic twins of these targets — the seeded no-panic sweeps in
`crates/rusty_erasure-core/tests/properties.rs` and
`short_and_degenerate_inputs_never_panic` in the fuzzlib — run on every
`cargo test`, on every platform, with no nightly required.

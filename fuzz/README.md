# Fuzz targets

`cargo +nightly fuzz run matrix_gen` / `matrix_invert` (add `-- -max_total_time=60` for a
bounded pass). Both targets assert full invariants, not just "no crash": an inversion that
succeeds must actually produce the inverse.

**Windows note:** cargo-fuzz links the ASan runtime but the DLL is not on PATH by default
(`STATUS_DLL_NOT_FOUND` at startup). Prepend the MSVC host tools directory first:

```powershell
$env:PATH = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64;$env:PATH"
```

(`--sanitizer none` does NOT work on the MSVC target — libfuzzer-sys then fails to link
against the sancov section symbols.)

The deterministic twins of these targets — the seeded no-panic sweeps in
`crates/rusty_erasure-core/tests/properties.rs` — run on every `cargo test`, on every
platform, with no nightly required.

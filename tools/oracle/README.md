# tools/oracle — the ISA-L oracle rig (nothing here ships)

Rig-only tooling that turns Intel ISA-L into our conformance oracle and perf baseline
(mission plan §7.1). Runs under WSL/Linux; no dev machine ever needs a C toolchain — the
outputs are checked into `corpus/golden/` as **data** and diffed everywhere with zero C.

## What exists (M1–M2)

- **`extract_tables.py`** — parses ISA-L `erasure_code/ec_base.h` (tag **v2.32.1**,
  downloaded to a scratch dir, never committed) and emits the four GF(2^8) base tables as
  binary golden data, with element-count and cross-consistency verification. Provenance
  (source sha256, output sha256s) lands in `corpus/golden/PROVENANCE.md`.
- **`gen_vectors.c`** — compiled against unmodified upstream `ec_base.c` (a two-file gcc
  build: no autotools, no nasm) and run to produce `corpus/golden/encode_vectors.bin`:
  deterministic inputs → `gf_gen_rs_matrix` / `gf_gen_cauchy1_matrix` →
  `ec_init_tables_base` → `ec_encode_data_base`, 77 cases across 11 configs × 7 lengths
  (tails included). The generator itself asserts update-sequence ≡ one-shot against the
  reference before writing anything. File format documented in the source header.

  ```sh
  gcc -O2 -I <isal_headers> gen_vectors.c <isal_src>/ec_base.c -o genvec
  ./genvec corpus/golden/encode_vectors.bin
  ```

  Headers needed beside the sources: `erasure_code.h`, `isal_api.h`, `gf_vect_mul.h`,
  `ec_base.h` (all from the pinned tag).

## Honesty note

These vectors are produced by ISA-L's portable **`_base`** implementations. ISA-L's own
test suite guarantees its SIMD kernels match `_base`; our M4+ perf work will additionally
cross-check against the full dispatched library build on the perf rig.

## Still to come (M4+)

- `perf_arm.sh` — ISA-L's own `erasure_code_perf` as the baseline arm for ERASCORP,
  their full build (autotools + nasm), their defaults, work-count parity asserted.

License note (plan §9.2): ISA-L is BSD-3-Clause and is used strictly as a black-box
oracle. No ISA-L source is copied into this repository; generated vectors are data.

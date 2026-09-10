# rusty_erasure on an ESP32-S3 — the on-board proof

"Builds for a bare-metal target" and "runs on the part" are different claims. CI
makes the first one every push (`cargo check --target thumbv7em-none-eabihf` and
`--target riscv32imac-unknown-none-elf`). This directory is the second one, and
it is hand-run: it needs Espressif's Rust fork, which CI does not have.

**Result, 2026-09-09, ESP32-S3 revision v0.2, 8 MB flash, 240 MHz, 192 KiB heap,
`no_std + alloc`, scalar kernels:**

```
=== rusty_erasure on ESP32-S3 (xtensa, no_std + alloc, scalar kernels) ===
census::CENSUS_LIVE = false  (false is expected here: no 64-bit atomics)
cpu_clock           = 240 MHz
timer resolution    = 1 us (measured, not quoted)
kernel set          = scalar
encode+verify       OK   (10,4) x 1024 B shards
recover 2 lost      OK   shards [2, 7] rebuilt byte-identical

-- throughput (source-byte basis, best of 5, scalar kernels) --
  null arm (no codec work)          185 us for 2608 reps  (harness floor)
  encode (4,2) 1KiB               5.962 MB/s     40.26 cyc/B
  encode (10,4) 1KiB              2.991 MB/s     80.24 cyc/B
  encode (10,4) 4KiB              2.994 MB/s     80.15 cyc/B
  recover (10,4) 2 lost 1KiB      5.972 MB/s     40.18 cyc/B
  raid xor P (10 src, 4KiB)      22.079 MB/s     10.87 cyc/B
  raid pq P+Q (10 src, 4KiB)     19.509 MB/s     12.30 cyc/B

RESULT: PASS -- encoded, verified, recovered and timed on the board
```

## Why this part is the right test

Xtensa LX7 is 32-bit, so `core::sync::atomic::AtomicU64` does not exist on it —
the same reason Cortex-M4F and RV32 could not build the crate at all until
`census64` landed in 0.4.1. Every reach-census counter here is the zero-sized
stub, which is why `CENSUS_LIVE` prints `false`. **A zero from a counter on this
part means "not measurable on this target", never "measured zero".**

The board therefore exercises the exact configuration the fix created, and the
`PASS` line is the evidence that stubbing the instrument did not disturb the
codec.

## The correctness gate comes before the numbers

A wrong codec's throughput is not a result, so the firmware refuses to print a
timing table unless the gate passes first: RS(10,4) encode, `verify` against the
encoded parity, then destroy data shards 2 and 7 and rebuild them from the
survivors, compared **byte for byte** against the originals. On failure it prints
`RESULT: FAIL` and stops without timing anything.

## What the throughput numbers mean, and how they were taken

Method line, so the numbers can be audited rather than trusted:

- **Basis: source bytes** — the bytes fed in per rep, the same basis every number
  in `corpus/LEDGER.md` uses, so the board is comparable with the host.
- **Clock:** `esp_hal::time::Instant`, whose resolution the firmware **measures**
  (smallest non-zero gap between distinct readings) rather than quoting: 1 µs.
  Every cell is calibrated to run ~60 ms, so one tick is ~0.002% — far below any
  effect reported.
- **Best of 5**, after an untimed calibration pass that doubles as the warm-up,
  so no reported round pays for cold caches.
- **A null arm** — the same loop shape with no codec work — establishes the
  harness floor at 185 µs / 2608 reps ≈ 0.071 µs per rep. The codec cells run
  0.7–14 ms per rep, so the harness is ≤0.01% of every cell.
- **Buffers and the coder are built once, outside the timed region**, so these
  measure the kernels and not the allocator.
- **Reproducibility: three runs, agreeing to at most one timer tick.** Across
  three consecutive flashes (the last on exactly the committed source) every
  cell reproduced, with two cells differing by a single microsecond in ~57,000
  (57027/57026 µs, and 56580/56579 µs). The null arm calibrates to a different
  rep count each boot but its **per-rep cost is identical** — 185/2608 and
  224/3157 are both 0.0709 µs. Bare metal has no scheduler, no co-tenants and no
  thermal governor in play here, so the pinning/ABBA/z-score machinery a host box
  needs is not required: the instrument is deterministic.

### The work model closes

These are absolute throughput figures for one implementation, not an A/B, so the
corroboration is internal: RS(k,p) encode performs `k·p·len` GF(2^8)
multiply-accumulate byte-ops, which on a source basis is exactly **p** MACs per
source byte; rebuilding `r` shards from `k` survivors is **r** per source byte.
Dividing each cell by its own MAC count should therefore give one constant:

| cell | cyc / source byte | MACs / byte | cyc per GF MAC byte |
|---|---:|---:|---:|
| encode (4,2) 1 KiB | 40.26 | 2 | **20.13** |
| encode (10,4) 1 KiB | 80.24 | 4 | **20.06** |
| encode (10,4) 4 KiB | 80.15 | 4 | **20.04** |
| recover 2-of-10 1 KiB | 40.18 | 2 | **20.09** |

Four cells, two different code paths, three parameter sets, two shard sizes —
all within **0.46%** of ~20.1 cycles per GF(2^8) multiply-accumulate byte. Every
throughput figure above is that one constant times the parity count, which is
what a correct, compute-bound scalar kernel should look like on an in-order
core.

The RAID pair is consistent with it: the P path is a plain XOR with no GF
multiply and costs 10.87 cyc/source byte, roughly half a GF MAC; P+Q adds only
1.43 cyc/byte over P because the fused kernel reads each source **once** and
computes both lanes from the loaded word.

For scale, the host scalar arm in `corpus/LEDGER.md` is ~445 MB/s for (10,4) on
an i7-14650HX ≈ 2.81 cyc per MAC byte, so this in-order 240 MHz Xtensa spends
~7.1× the cycles per MAC byte that an out-of-order x86 does. That is a plausible
IPC gap for table-lookup-heavy integer code, not an anomaly.

## Running it

```sh
espup install                      # once: the `esp` Rust fork for Xtensa
cargo install espflash             # once
cd bare-metal/esp32s3
cargo run --release -- --port COM4 # your port; omit on Linux/macOS to autodetect
```

The linker is the esp toolchain's, and only espup's export script puts it on
`PATH`. In bash:

```sh
export PATH="$HOME/.rustup/toolchains/esp/xtensa-esp-elf/bin:$PATH"
```

To capture the output from a non-interactive shell (the monitor never exits, and
a pipe loses everything when the timeout fires — redirect to a FILE):

```sh
timeout 170 espflash flash --monitor --port COM4 --non-interactive \
  target/xtensa-esp32s3-none-elf/release/s3erasure > run.log 2>&1
```

Passes when the last line reads `RESULT: PASS`. If a re-run reports
`Access is denied` on the port, a previous monitor still holds it — kill the
lingering `espflash` process first.

It is not in the workspace (`exclude` in the root manifest), so a normal
`cargo build` at the repo root never sees it and never needs the Xtensa
toolchain.

## What this does not claim

- **Scalar only, and that is the whole story on this part.** The build is
  `--no-default-features`. `rusty_erasure-accel` has no Xtensa kernel set, so
  the scalar kernels *are* the shipping path here — there is no SIMD being left
  on the table. The firmware prints `kernel set = scalar` so the log says which
  path ran; on this part the reach census cannot answer that question, because
  it is stubbed.
- **One part, and not a Kairos target.** The S3 is Xtensa. The two targets CI
  compiles are ARM (Cortex-M4F) and RISC-V, and neither has been run on silicon
  here. This makes the stub configuration real on hardware; it does not close
  anyone's Cortex-M row.
- **No comparison verdict.** Nothing here is measured against another
  implementation, so no "faster than X" claim is made or implied.
- **Heap not measured to a floor.** 192 KiB was ample for these shard sizes;
  finding the minimum heap per configuration is a separate exercise.

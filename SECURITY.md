# Security policy

## Reporting a vulnerability

Email **tim.almond@thehouseinc.xyz** with the details. You will get an
acknowledgement within **72 hours** and a status update at least every **14
days**. Please give us **90 days** of coordinated disclosure before publishing;
we will credit you in the advisory unless you ask otherwise.

Do not open public issues for suspected vulnerabilities.

## Supported versions

Only the latest published release receives security fixes.

## Threat model

**Assets.** The integrity of erasure-coded data: parity that byte-matches the
GF(2^8)/0x11d Reed-Solomon math, and recovery that reconstructs exactly the
bytes that were encoded. Consumers (SpaceDB durability, Deputy vault snapshots)
stake data durability on both.

**Adversaries and untrusted input.** This library performs pure computation:
it opens no sockets, files, or processes and holds no secrets or key material.
The attack surface is its arguments — shard slices, lengths, matrix
dimensions, and loss patterns supplied by a caller that may itself be
processing hostile data (a corrupted shard store, a malicious peer's stripe).

**STRIDE pass.**
- *Spoofing / Repudiation / Information disclosure*: out of scope — no
  identity, no logging of payloads, no secret state. Shard *authenticity* is
  the consumer's job (sign or MAC shards); erasure parity is not a MAC and
  `verify()` detects corruption, not forgery.
- *Tampering*: corrupted shard bytes yield wrong reconstruction only if the
  caller feeds a wrong-but-well-formed stripe; `verify()`/`pq_check` detect
  inconsistency. Within the library, conformance is gated by byte-identity
  against ISA-L golden vectors (902-case full grid) so the math itself cannot
  drift silently.
- *Denial of service*: hostile dimensions or lengths must never panic or
  overflow. Every public entry point validates and returns typed errors
  (`CodeError`, `MatrixError`, `RecoverError`); the no-panic dimension sweep,
  three fuzz targets (ASan), and Miri gate this. Release builds carry
  `overflow-checks = true`.
- *Elevation of privilege*: the only `unsafe` code is the SIMD kernels in
  `rusty_erasure-accel`, every block SAFETY-commented, every kernel gated
  byte-identical against the `forbid(unsafe_code)` scalar core on every
  architecture in CI.

**Residual risks** are listed in
[docs/plans/use-protection-please.md](docs/plans/use-protection-please.md).

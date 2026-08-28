//! Fuzz harness bodies, shared by two consumers so they can never drift:
//!
//! 1. `fuzz/fuzz_targets/*.rs` — coverage-guided libFuzzer runs with ASan, on
//!    x86-64 (libFuzzer's only target here).
//! 2. `tests/corpus_replay.rs` — the SAME bodies over the SAME corpus, run on
//!    every architecture we ship: native, `wasm32-wasip1` under wasmtime, and
//!    `aarch64` under qemu.
//!
//! That second consumer is the point. libFuzzer cannot run on wasm, so a
//! fuzzer finding on x86 would otherwise say nothing about the SIMD128 or
//! NEON kernels — different unsafe code, different unroll boundaries, same
//! inputs. Replaying the corpus everywhere closes that gap: every input the
//! fuzzer discovered is executed against every kernel set we ship.
//!
//! Every body is total: it may return early on shapes it cannot use, but it
//! must never panic on any byte string. A panic IS the finding.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use rusty_erasure::isal;
use rusty_erasure_core::{Coder, Matrix};

/// A small deterministic PRNG for harness choices (never for data).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Matrix generation must never panic, for ANY `(k, p)` — errors are typed.
pub fn matrix_gen(data: &[u8]) {
    if data.len() < 8 {
        return;
    }
    let k = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let p = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let _ = Matrix::reed_solomon(k, p);
    let _ = Matrix::cauchy(k, p);
}

/// Inversion of arbitrary matrices must never panic — and when it succeeds,
/// the result must actually be the inverse.
pub fn matrix_invert(data: &[u8]) {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let n = (first % 32) as usize + 1;
    if rest.len() < n * n {
        return;
    }
    let Ok(m) = Matrix::from_bytes(n, n, rest[..n * n].to_vec()) else {
        return;
    };
    if let Ok(inv) = m.invert() {
        let prod = m.multiply(&inv).expect("square dimensions always compose");
        assert!(prod.is_identity(), "inverse that is not an inverse, n={n}");
    }
    let _ = m.select_rows(&[0, n - 1]);
}

/// End-to-end: encode arbitrary data, lose an arbitrary subset of shards,
/// recover, and assert ground truth — the whole pipeline must never panic and
/// never reconstruct wrong bytes.
///
/// Runs through the DISPATCHED coder, so on each architecture this exercises
/// that architecture's SIMD kernels.
pub fn roundtrip(input: &[u8]) {
    if input.len() < 4 {
        return;
    }
    let k = (input[0] % 16) as usize + 1;
    let p = (input[1] % 8) as usize + 1;
    let len = input[2] as usize; // 0..=255
    let loss_seed = input[3];
    let payload = &input[4..];
    if payload.len() < k * len {
        return;
    }

    let matrix = Matrix::cauchy(k, p).expect("dims in range");
    let coder = rusty_erasure::coder(matrix).expect("has parity");
    let data: Vec<&[u8]> = (0..k).map(|j| &payload[j * len..(j + 1) * len]).collect();
    let mut parity = vec![vec![0u8; len]; p];
    {
        let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
        coder
            .encode(&data, &mut refs)
            .expect("validated inputs encode");
    }

    // The dispatched encode must equal the scalar oracle, byte for byte —
    // this is what turns a corpus replay on wasm/arm into a kernel proof.
    {
        let scalar = Coder::new(Matrix::cauchy(k, p).expect("dims in range")).expect("has parity");
        let mut want = vec![vec![0u8; len]; p];
        let mut refs: Vec<&mut [u8]> = want.iter_mut().map(|b| b.as_mut_slice()).collect();
        scalar.encode(&data, &mut refs).expect("scalar encodes");
        assert_eq!(
            want, parity,
            "dispatched encode != scalar oracle, k={k} p={p} len={len}"
        );
    }

    // Lose up to p shards, chosen by the fuzzer. Bounded by construction:
    // a partial Fisher-Yates over 0..n (the previous ad-hoc walk had a fixed
    // point — e.g. (2*31+17) % 7 == 2 — and could spin forever; the fuzzer
    // itself caught it as a timeout. Selection loops must be provably finite.)
    let n = k + p;
    let mut rng = Rng(loss_seed as u64);
    let nloss = 1 + (rng.next() as usize) % p;
    let mut all: Vec<usize> = (0..n).collect();
    for i in 0..nloss {
        let j = i + (rng.next() as usize) % (n - i);
        all.swap(i, j);
    }
    let mut missing = all[..nloss].to_vec();
    missing.sort_unstable();

    let shards: Vec<Option<&[u8]>> = (0..n)
        .map(|i| {
            if missing.contains(&i) {
                None
            } else if i < k {
                Some(data[i])
            } else {
                Some(parity[i - k].as_slice())
            }
        })
        .collect();
    let mut out = vec![vec![0u8; len]; missing.len()];
    let mut refs: Vec<&mut [u8]> = out.iter_mut().map(|b| b.as_mut_slice()).collect();
    coder
        .recover(&shards, &missing, &mut refs)
        .expect("<= p losses always recover");
    for (&x, got) in missing.iter().zip(&out) {
        let expect: &[u8] = if x < k { data[x] } else { &parity[x - k] };
        assert_eq!(got.as_slice(), expect, "shard {x}");
    }
}

/// The compat layer's no-panic contract: every `isal::*` entry point, fed
/// arbitrary parameters and undersized buffers, must return a typed error or
/// succeed — never panic, never read out of bounds (ASan watches the latter).
pub fn compat(input: &[u8]) {
    if input.len() < 8 {
        return;
    }
    let k = input[0] as usize;
    let m = input[1] as usize;
    let rows = input[2] as usize % 16;
    let len = input[3] as usize;
    let vec_i = input[4] as usize;
    let n = input[5] as usize % 24;
    let payload = &input[8..];

    // Matrix generation into a buffer the fuzzer sizes (possibly too small).
    let mut a = vec![0u8; payload.len().min(4096)];
    let _ = isal::gf_gen_rs_matrix(&mut a, m, k);
    let _ = isal::gf_gen_cauchy1_matrix(&mut a, m, k);

    // Inversion of arbitrary bytes at an arbitrary claimed dimension.
    let mut inv = vec![0u8; a.len()];
    let mut scratch = a.clone();
    let _ = isal::gf_invert_matrix(&mut scratch, &mut inv, n);

    // Table expansion with whatever coefficients and (k, rows) claim.
    let mut gftbls = vec![0u8; payload.len().min(8192)];
    let _ = isal::ec_init_tables(k, rows, payload, &mut gftbls);

    // Encode / update / kernels over fuzzer-shaped buffers.
    let shard = payload.len().min(64);
    let nsrc = (k % 8).max(1);
    let srcs: Vec<Vec<u8>> = (0..nsrc)
        .map(|i| {
            let start = (i * shard).min(payload.len());
            let end = ((i + 1) * shard).min(payload.len());
            payload[start..end].to_vec()
        })
        .collect();
    let src_refs: Vec<&[u8]> = srcs.iter().map(|s| s.as_slice()).collect();
    let mut coding = vec![vec![0u8; shard]; rows.max(1)];
    let mut coding_refs: Vec<&mut [u8]> = coding.iter_mut().map(|b| b.as_mut_slice()).collect();

    let _ = isal::ec_encode_data(len, nsrc, rows, &gftbls, &src_refs, &mut coding_refs);
    let _ = isal::ec_encode_data_update(len, nsrc, rows, vec_i, &gftbls, payload, &mut coding_refs);

    let mut dest = vec![0u8; shard];
    let _ = isal::gf_vect_dot_prod(len, nsrc, &gftbls, &src_refs, &mut dest);
    let _ = isal::gf_vect_mad(len, nsrc, vec_i, &gftbls, payload, &mut dest);
    let _ = isal::gf_vect_mul(len, &gftbls, payload, &mut dest);
}

/// RAID parity, differentially: the DISPATCHED `xor_gen`/`pq_gen` must be
/// byte-identical to the scalar core for every shape, and the checkers must
/// agree with what the generators produced.
///
/// This is the target for the newest unsafe code in the tree — five RAID
/// kernel paths (SSE2, AVX2, GFNI, NEON, SIMD128), each with 128/64/32/16-byte
/// unroll tiers whose boundaries are exactly where an off-by-one lives.
pub fn raid(input: &[u8]) {
    use rusty_erasure_core::raid as core_raid;

    if input.len() < 3 {
        return;
    }
    // 2..=17 sources, lengths that straddle every unroll tier.
    let nsrc = (input[0] % 16) as usize + 2;
    let len = input[1] as usize * 2; // 0..=510, hits 0/16/32/64/128 crossings
    let payload = &input[2..];
    if payload.is_empty() || len == 0 {
        return;
    }

    // Build sources by tiling the payload — arbitrary bytes, exact lengths.
    let sources: Vec<Vec<u8>> = (0..nsrc)
        .map(|i| {
            (0..len)
                .map(|j| payload[(i * len + j) % payload.len()])
                .collect()
        })
        .collect();
    let refs: Vec<&[u8]> = sources.iter().map(|s| s.as_slice()).collect();

    // xor: dispatched vs core.
    let mut want_x = vec![0u8; len];
    core_raid::xor_gen(&refs, &mut want_x).expect("validated shape");
    let mut got_x = vec![0xAAu8; len];
    rusty_erasure::raid::xor_gen(&refs, &mut got_x).expect("validated shape");
    assert_eq!(
        want_x, got_x,
        "dispatched xor_gen != core, nsrc={nsrc} len={len}"
    );

    // pq: dispatched vs core.
    let mut want_p = vec![0u8; len];
    let mut want_q = vec![0u8; len];
    core_raid::pq_gen(&refs, &mut want_p, &mut want_q).expect("validated shape");
    let mut got_p = vec![0xAAu8; len];
    let mut got_q = vec![0x55u8; len];
    rusty_erasure::raid::pq_gen(&refs, &mut got_p, &mut got_q).expect("validated shape");
    assert_eq!(
        want_p, got_p,
        "dispatched pq_gen P != core, nsrc={nsrc} len={len}"
    );
    assert_eq!(
        want_q, got_q,
        "dispatched pq_gen Q != core, nsrc={nsrc} len={len}"
    );

    // The checkers must accept what the generators produced.
    let mut vects: Vec<&[u8]> = refs.clone();
    vects.push(&got_x);
    assert!(
        rusty_erasure::raid::xor_check(&vects).expect("validated shape"),
        "xor_check rejected its own xor_gen output, nsrc={nsrc} len={len}"
    );
    assert!(
        rusty_erasure::raid::pq_check(&refs, &got_p, &got_q)
            .expect("validated shape")
            .is_none(),
        "pq_check rejected its own pq_gen output, nsrc={nsrc} len={len}"
    );

    // And must REJECT a single flipped bit (detection, not just agreement).
    if len > 0 {
        let flip = (input[0] as usize) % len;
        let mut bad_q = got_q.clone();
        bad_q[flip] ^= 0x80;
        if bad_q != got_q {
            assert!(
                rusty_erasure::raid::pq_check(&refs, &got_p, &bad_q)
                    .expect("validated shape")
                    .is_some(),
                "pq_check missed a flipped Q byte at {flip}, nsrc={nsrc} len={len}"
            );
        }
    }
}

/// Every SIMD kernel set this build exposes, differentially against the
/// scalar oracle: encode, mad, and the fused update, over fuzzer-chosen
/// dimensions and lengths.
///
/// The scalar set in core is `forbid(unsafe_code)`; every other set is
/// hand-written intrinsics. Any disagreement is a kernel bug, and any
/// out-of-bounds access is an ASan finding on the native run.
pub fn kernels(input: &[u8]) {
    if input.len() < 4 {
        return;
    }
    let k = (input[0] % 12) as usize + 1;
    let rows = (input[1] % 8) as usize + 1;
    let len = input[2] as usize * 2; // straddles 16/32/64/128 boundaries
    let payload = &input[3..];
    if payload.is_empty() {
        return;
    }

    let coeffs: Vec<u8> = (0..rows * k).map(|i| payload[i % payload.len()]).collect();
    let data: Vec<Vec<u8>> = (0..k)
        .map(|i| {
            (0..len)
                .map(|j| payload[(i * len + j + 1) % payload.len()])
                .collect()
        })
        .collect();
    let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();

    let scalar = rusty_erasure::kernel::Kernels::scalar();
    let mut want = vec![vec![0u8; len]; rows];
    {
        let g = (scalar.init)(&coeffs);
        let mut refs: Vec<&mut [u8]> = want.iter_mut().map(|b| b.as_mut_slice()).collect();
        (scalar.encode)(&g, &data_refs, &mut refs);
    }

    for name in ["auto", "ssse3", "avx2", "gfni", "neon", "simd128"] {
        let Some(set) = rusty_erasure::kernels_named(name) else {
            continue; // not supported on this CPU/build — nothing to compare
        };
        let g = (set.init)(&coeffs);

        // encode
        let mut got = vec![vec![0xAAu8; len]; rows];
        {
            let mut refs: Vec<&mut [u8]> = got.iter_mut().map(|b| b.as_mut_slice()).collect();
            (set.encode)(&g, &data_refs, &mut refs);
        }
        assert_eq!(
            want, got,
            "{name} encode != scalar, k={k} rows={rows} len={len}"
        );

        // mad on the first coefficient, from an arbitrary starting state
        let base: Vec<u8> = (0..len).map(|j| payload[(j + 7) % payload.len()]).collect();
        let sg = (scalar.init)(&coeffs[..1]);
        let vg = (set.init)(&coeffs[..1]);
        let mut ma = base.clone();
        let mut mb = base;
        (scalar.mad)(&sg, &data[0], &mut ma);
        (set.mad)(&vg, &data[0], &mut mb);
        assert_eq!(ma, mb, "{name} mad != scalar, len={len}");

        // fused update: fold source 0 into every row, from the encoded state
        let mut ua = want.clone();
        let mut ub = want.clone();
        {
            let sgf = (scalar.init)(&coeffs);
            let mut refs: Vec<&mut [u8]> = ua.iter_mut().map(|b| b.as_mut_slice()).collect();
            (scalar.update)(&sgf, k, 0, &data[0], &mut refs);
        }
        {
            let mut refs: Vec<&mut [u8]> = ub.iter_mut().map(|b| b.as_mut_slice()).collect();
            (set.update)(&g, k, 0, &data[0], &mut refs);
        }
        assert_eq!(
            ua, ub,
            "{name} update != scalar, k={k} rows={rows} len={len}"
        );
    }
}

/// The embedded fuzz corpus, generated by `build.rs`.
pub mod corpus {
    include!(concat!(env!("OUT_DIR"), "/corpus.rs"));
}

/// Dispatch one corpus case to the body that produced it.
pub fn run_case(target: &str, input: &[u8]) {
    match target {
        "matrix_gen" => matrix_gen(input),
        "matrix_invert" => matrix_invert(input),
        "roundtrip" => roundtrip(input),
        "compat" => compat(input),
        "raid" => raid(input),
        "kernels" => kernels(input),
        other => panic!("corpus directory '{other}' has no harness body — add one or remove it"),
    }
}

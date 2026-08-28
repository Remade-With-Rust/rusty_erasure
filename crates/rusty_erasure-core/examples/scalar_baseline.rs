//! Scalar encode baseline (M3, ledger). Deterministic work: encodes one
//! stripe `reps` times and prints the work counts, per-rep wall stats, and a
//! parity checksum (defeats dead-code elimination; doubles as a determinism
//! check across runs). The pinned harness (`tools/bench/scalar_baseline.ps1`)
//! derives GB/s from the PROCESS CPU time per codec-measurement §1–§2 —
//! wall time printed here is informational only.
//!
//! Usage: scalar_baseline <k> <p> <shard_len> <reps>

use std::hint::black_box;
use std::time::Instant;

use rusty_erasure_core::{Coder, Matrix};

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let &[k, p, len, reps] = args.as_slice() else {
        eprintln!("usage: scalar_baseline <k> <p> <shard_len> <reps>");
        std::process::exit(2);
    };

    let coder = match Matrix::cauchy(k, p).and_then(Coder::new) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bad config: {e}");
            std::process::exit(2);
        }
    };

    // Deterministic data (splitmix64) so every run encodes identical bytes.
    let mut state: u64 = (k as u64) << 32 | (p as u64) << 16 | len as u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let data: Vec<Vec<u8>> = (0..k)
        .map(|_| (0..len).map(|_| next() as u8).collect())
        .collect();
    let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
    let mut parity = vec![vec![0u8; len]; p];

    // Warmup (untimed), then the timed reps.
    for _ in 0..3 {
        let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
        coder.encode(&data_refs, &mut refs).expect("encode");
    }
    let mut per_rep_ns: Vec<u128> = Vec::with_capacity(reps);
    let total = Instant::now();
    for _ in 0..reps {
        let t = Instant::now();
        let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
        coder
            .encode(&data_refs, black_box(&mut refs))
            .expect("encode");
        per_rep_ns.push(t.elapsed().as_nanos());
        black_box(&parity);
    }
    let wall = total.elapsed();

    let checksum = parity.iter().flatten().fold(0u8, |a, &b| a ^ b);
    per_rep_ns.sort_unstable();
    let src_bytes = (k * len) as u128 * reps as u128;
    let mul_count = (k * p * len) as u128 * reps as u128;
    println!("cell k={k} p={p} len={len} reps={reps}");
    println!("work: source_bytes={src_bytes} table_muls={mul_count} checksum={checksum:#04x}");
    println!(
        "wall: total_ms={} rep_min_us={} rep_median_us={}",
        wall.as_millis(),
        per_rep_ns.first().unwrap_or(&0) / 1000,
        per_rep_ns.get(reps / 2).unwrap_or(&0) / 1000,
    );
}

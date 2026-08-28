//! End-to-end: encode arbitrary data, lose an arbitrary subset of shards,
//! recover, and assert ground truth — the whole pipeline must never panic and
//! never reconstruct wrong bytes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rusty_erasure_core::{Coder, Matrix};

fuzz_target!(|input: &[u8]| {
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

    let coder = Coder::new(Matrix::cauchy(k, p).expect("dims in range")).expect("has parity");
    let data: Vec<&[u8]> = (0..k).map(|j| &payload[j * len..(j + 1) * len]).collect();
    let mut parity = vec![vec![0u8; len]; p];
    {
        let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
        coder.encode(&data, &mut refs).expect("validated inputs encode");
    }

    // Lose up to p shards, chosen by the fuzzer. Bounded by construction:
    // a partial Fisher-Yates over 0..n (the previous ad-hoc walk had a fixed
    // point — e.g. (2*31+17) % 7 == 2 — and could spin forever; the fuzzer
    // itself caught it as a timeout. Selection loops must be provably finite.)
    let n = k + p;
    let mut state = loss_seed as u64;
    let mut rand = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let nloss = 1 + (rand() as usize) % p;
    let mut all: Vec<usize> = (0..n).collect();
    for i in 0..nloss {
        let j = i + (rand() as usize) % (n - i);
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
    coder.recover(&shards, &missing, &mut refs).expect("<= p losses always recover");
    for (&x, got) in missing.iter().zip(&out) {
        let expect: &[u8] = if x < k { data[x] } else { &parity[x - k] };
        assert_eq!(got.as_slice(), expect, "shard {x}");
    }
});

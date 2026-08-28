//! Coder property tests: roundtrip, exhaustive loss patterns, update-order
//! independence, and the validated-misuse contract. Seeded and deterministic.

use rusty_erasure_core::{CodeError, Coder, Matrix, RecoverError};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

struct Stripe {
    coder: Coder,
    data: Vec<Vec<u8>>,
    parity: Vec<Vec<u8>>,
}

fn stripe(coder: Coder, len: usize, rng: &mut Rng) -> Stripe {
    let (k, p) = (coder.k(), coder.p());
    let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();
    let mut parity = vec![vec![0u8; len]; p];
    let refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
    let mut prefs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
    coder.encode(&refs, &mut prefs).expect("encode");
    Stripe { coder, data, parity }
}

/// Recover every shard in `missing` from the rest and assert ground truth.
fn check_recovery(s: &Stripe, missing: &[usize]) {
    let (k, p) = (s.coder.k(), s.coder.p());
    let len = s.data[0].len();
    let shards: Vec<Option<&[u8]>> = (0..k + p)
        .map(|i| {
            if missing.contains(&i) {
                None
            } else if i < k {
                Some(s.data[i].as_slice())
            } else {
                Some(s.parity[i - k].as_slice())
            }
        })
        .collect();
    let mut out = vec![vec![0u8; len]; missing.len()];
    let mut refs: Vec<&mut [u8]> = out.iter_mut().map(|b| b.as_mut_slice()).collect();
    s.coder
        .recover(&shards, missing, &mut refs)
        .unwrap_or_else(|e| panic!("recover k={k} p={p} missing={missing:?}: {e}"));
    for (&x, got) in missing.iter().zip(&out) {
        let expect = if x < k { &s.data[x] } else { &s.parity[x - k] };
        assert_eq!(got, expect, "shard {x} of missing={missing:?}");
    }
}

#[test]
fn roundtrip_verify_and_sampled_recovery_across_the_grid() {
    let mut rng = Rng(0x0002_0001);
    let configs: &[(usize, usize)] = if cfg!(miri) {
        &[(2, 2), (4, 2)]
    } else {
        &[(2, 2), (4, 2), (8, 2), (10, 4), (16, 4), (20, 8), (32, 8)]
    };
    let lens: &[usize] = if cfg!(miri) { &[1, 33] } else { &[1, 31, 32, 33, 96, 1024] };
    let patterns = if cfg!(miri) { 3 } else { 20 };
    for &(k, p) in configs {
        for &len in lens {
            let s = stripe(Coder::new(Matrix::cauchy(k, p).unwrap()).unwrap(), len, &mut rng);
            let drefs: Vec<&[u8]> = s.data.iter().map(|d| d.as_slice()).collect();
            let prefs: Vec<&[u8]> = s.parity.iter().map(|b| b.as_slice()).collect();
            assert_eq!(s.coder.verify(&drefs, &prefs), Ok(true), "verify k={k} p={p} len={len}");

            // Random loss patterns of size 1..=p across the whole stripe.
            for _ in 0..patterns {
                let nloss = 1 + rng.below(p);
                let mut missing: Vec<usize> = Vec::new();
                while missing.len() < nloss {
                    let x = rng.below(k + p);
                    if !missing.contains(&x) {
                        missing.push(x);
                    }
                }
                missing.sort_unstable();
                check_recovery(&s, &missing);
            }
        }
    }
}

#[test]
#[cfg_attr(miri, ignore = "1470-pattern sweep is deterministic and UB-free; too slow interpreted")]
fn exhaustive_loss_patterns_on_the_s6_config() {
    // ERASCORP S6: (10, 4). EVERY loss pattern of size 1..=4 over the 14
    // shards — 1470 patterns — rebuilt in full and compared to ground truth.
    let mut rng = Rng(0x0002_0002);
    let s = stripe(Coder::new(Matrix::cauchy(10, 4).unwrap()).unwrap(), 257, &mut rng);
    let n = 14;
    let mut patterns = 0u32;
    let mut missing = Vec::new();
    for a in 0..n {
        missing.clear();
        missing.push(a);
        check_recovery(&s, &missing);
        patterns += 1;
        for b in a + 1..n {
            missing.truncate(1);
            missing.push(b);
            check_recovery(&s, &missing);
            patterns += 1;
            for c in b + 1..n {
                missing.truncate(2);
                missing.push(c);
                check_recovery(&s, &missing);
                patterns += 1;
                for d in c + 1..n {
                    missing.truncate(3);
                    missing.push(d);
                    check_recovery(&s, &missing);
                    patterns += 1;
                }
            }
        }
    }
    assert_eq!(patterns, 14 + 91 + 364 + 1001, "pattern census");
}

#[test]
fn update_in_any_order_equals_one_shot_encode() {
    let mut rng = Rng(0x0002_0003);
    let configs: &[(usize, usize)] =
        if cfg!(miri) { &[(4, 2)] } else { &[(4, 2), (10, 4), (16, 4)] };
    for &(k, p) in configs {
        let len = if cfg!(miri) { 65 } else { 513 };
        let s = stripe(Coder::new(Matrix::cauchy(k, p).unwrap()).unwrap(), len, &mut rng);
        for _ in 0..5 {
            // Random permutation of the update order.
            let mut order: Vec<usize> = (0..k).collect();
            for i in 0..k {
                let j = i + rng.below(k - i);
                order.swap(i, j);
            }
            let mut parity = vec![vec![0u8; len]; p];
            let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
            for &j in &order {
                s.coder.update(j, &s.data[j], &mut refs).expect("update");
            }
            assert_eq!(parity, s.parity, "k={k} p={p} order={order:?}");
        }
    }
}

#[test]
fn decode_plan_recovery_equals_one_shot() {
    let mut rng = Rng(0x0002_0006);
    for &(k, p) in &[(4usize, 2usize), (10, 4), (16, 8)] {
        let len = 777;
        let s = stripe(Coder::new(Matrix::cauchy(k, p).unwrap()).unwrap(), len, &mut rng);
        for _ in 0..10 {
            let nloss = 1 + rng.below(p);
            let mut missing: Vec<usize> = Vec::new();
            while missing.len() < nloss {
                let x = rng.below(k + p);
                if !missing.contains(&x) {
                    missing.push(x);
                }
            }
            missing.sort_unstable();
            let present: Vec<bool> = (0..k + p).map(|i| !missing.contains(&i)).collect();
            let plan = s.coder.decode_plan(&present, &missing).expect("plan");
            let shards: Vec<Option<&[u8]>> = (0..k + p)
                .map(|i| {
                    if missing.contains(&i) {
                        None
                    } else if i < k {
                        Some(s.data[i].as_slice())
                    } else {
                        Some(s.parity[i - k].as_slice())
                    }
                })
                .collect();
            // One plan, several stripes' worth of calls (reuse is the point).
            for _ in 0..2 {
                let mut out = vec![vec![0u8; len]; missing.len()];
                let mut refs: Vec<&mut [u8]> = out.iter_mut().map(|b| b.as_mut_slice()).collect();
                s.coder.recover_with(&plan, &shards, &mut refs).expect("recover_with");
                for (&x, got) in missing.iter().zip(&out) {
                    let expect = if x < k { &s.data[x] } else { &s.parity[x - k] };
                    assert_eq!(got, expect, "k={k} p={p} plan recovery shard {x}");
                }
            }
        }
    }
}

#[test]
fn verify_detects_corruption() {
    let mut rng = Rng(0x0002_0004);
    let s = stripe(Coder::new(Matrix::cauchy(6, 3).unwrap()).unwrap(), 300, &mut rng);
    let drefs: Vec<&[u8]> = s.data.iter().map(|d| d.as_slice()).collect();
    let mut bad = s.parity.clone();
    bad[1][17] ^= 0x40;
    let prefs: Vec<&[u8]> = bad.iter().map(|b| b.as_slice()).collect();
    assert_eq!(s.coder.verify(&drefs, &prefs), Ok(false));
}

#[test]
fn zero_length_shards_are_a_valid_stripe() {
    let coder = Coder::new(Matrix::cauchy(4, 2).unwrap()).unwrap();
    let data: Vec<&[u8]> = vec![&[]; 4];
    let mut parity = vec![vec![0u8; 0]; 2];
    let mut prefs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
    coder.encode(&data, &mut prefs).expect("len-0 encode");
}

#[test]
fn misuse_is_typed_errors_never_panics() {
    let mut rng = Rng(0x0002_0005);
    let s = stripe(Coder::new(Matrix::cauchy(4, 2).unwrap()).unwrap(), 64, &mut rng);
    let coder = &s.coder;
    let drefs: Vec<&[u8]> = s.data.iter().map(|d| d.as_slice()).collect();

    // Wrong data-shard count.
    let mut parity = vec![vec![0u8; 64]; 2];
    let mut prefs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
    assert!(matches!(
        coder.encode(&drefs[..3], &mut prefs),
        Err(CodeError::ShardCount { expected: 4, got: 3 })
    ));

    // Mismatched shard length, with the offending index reported.
    let short = vec![0u8; 63];
    let mixed: Vec<&[u8]> = vec![&s.data[0], &short, &s.data[2], &s.data[3]];
    assert!(matches!(
        coder.encode(&mixed, &mut prefs),
        Err(CodeError::ShardLength { index: 1, expected: 64, got: 63 })
    ));

    // Update: out-of-range source index.
    assert!(matches!(
        coder.update(4, &s.data[0], &mut prefs),
        Err(CodeError::ShardIndex { index: 4, k: 4 })
    ));

    // Recover: too many missing.
    let shards: Vec<Option<&[u8]>> =
        vec![None, None, None, Some(&s.data[3]), Some(&s.parity[0]), Some(&s.parity[1])];
    let mut out = vec![vec![0u8; 64]; 3];
    let mut orefs: Vec<&mut [u8]> = out.iter_mut().map(|b| b.as_mut_slice()).collect();
    assert!(matches!(
        coder.recover(&shards, &[0, 1, 2], &mut orefs),
        Err(RecoverError::TooManyMissing { missing: 3, p: 2 })
    ));

    // Recover: wrong stripe width.
    assert!(coder.recover(&shards[..5], &[0], &mut orefs[..1]).is_err());

    // Recover: rebuild index out of range.
    let full: Vec<Option<&[u8]>> = (0..6)
        .map(|i| {
            if i < 4 {
                Some(s.data[i].as_slice())
            } else {
                Some(s.parity[i - 4].as_slice())
            }
        })
        .collect();
    assert!(coder.recover(&full, &[6], &mut orefs[..1]).is_err());

    // Rebuilding a PRESENT shard is allowed and must reproduce it (useful as
    // a scrub): also covers parity targets.
    let mut one = vec![vec![0u8; 64]; 1];
    let mut oref: Vec<&mut [u8]> = one.iter_mut().map(|b| b.as_mut_slice()).collect();
    coder.recover(&full, &[5], &mut oref).expect("scrub parity");
    assert_eq!(one[0], s.parity[1]);

    // Coder::new with no parity rows.
    assert!(Coder::new(Matrix::from_bytes(3, 3, vec![0; 9]).unwrap()).is_err());
}

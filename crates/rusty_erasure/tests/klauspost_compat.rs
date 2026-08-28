//! The consumer-swap conformance gate (M7b): a `Coder` built on
//! `compat::reed_solomon_erasure_matrix` must be BYTE-COMPATIBLE with the
//! `reed-solomon-erasure` crate (the engine spacedb-durability is migrating
//! off) in both directions:
//!   - our encode produces byte-identical parity to theirs;
//!   - we reconstruct stripes they encoded, through loss patterns that
//!     REQUIRE parity (data shards missing);
//!   - they reconstruct stripes we encoded.
//!
//! With this proven, the swap is a drop-in with zero wire-format break — old
//! snapshots decode, new snapshots read back on old code.

use reed_solomon_erasure::galois_8::ReedSolomon;
use rusty_erasure::{Coder, coder, compat};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

fn compat_coder(k: usize, p: usize) -> Coder {
    coder(compat::reed_solomon_erasure_matrix(k, p).expect("matrix")).expect("coder")
}

const CONFIGS: &[(usize, usize)] = &[(2, 1), (2, 2), (4, 2), (10, 4), (16, 8), (32, 8)];
const LENS: &[usize] = &[1, 31, 64, 1024, 4113];

#[test]
fn our_encode_is_byte_identical_to_reed_solomon_erasure() {
    let mut rng = Rng(0xC047_0001);
    for &(k, p) in CONFIGS {
        for &len in LENS {
            let rs = ReedSolomon::new(k, p).expect("rs");
            let c = compat_coder(k, p);
            let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();

            // Their encode: data + zeroed parity in one Vec.
            let mut theirs: Vec<Vec<u8>> = data.clone();
            theirs.extend((0..p).map(|_| vec![0u8; len]));
            rs.encode(&mut theirs).expect("their encode");

            // Our encode on the compat matrix.
            let refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
            let mut parity = vec![vec![0u8; len]; p];
            {
                let mut prefs: Vec<&mut [u8]> =
                    parity.iter_mut().map(|b| b.as_mut_slice()).collect();
                c.encode(&refs, &mut prefs).expect("our encode");
            }
            for l in 0..p {
                assert_eq!(parity[l], theirs[k + l], "k={k} p={p} len={len} parity {l}");
            }
        }
    }
}

#[test]
fn we_reconstruct_their_stripes_through_parity() {
    let mut rng = Rng(0xC047_0002);
    for &(k, p) in CONFIGS {
        let len = 1027;
        let rs = ReedSolomon::new(k, p).expect("rs");
        let c = compat_coder(k, p);
        let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();
        let mut stripe: Vec<Vec<u8>> = data.clone();
        stripe.extend((0..p).map(|_| vec![0u8; len]));
        rs.encode(&mut stripe).expect("their encode");

        // Drop the FIRST p shards — all data when p <= k, forcing full parity
        // use — and a mixed pattern.
        for missing in [
            (0..p).collect::<Vec<usize>>(),
            (0..p).map(|i| i * (k + p - 1) / p.max(1)).collect(),
        ] {
            let mut missing = missing;
            missing.sort_unstable();
            missing.dedup();
            let shards: Vec<Option<&[u8]>> = (0..k + p)
                .map(|i| {
                    if missing.contains(&i) {
                        None
                    } else {
                        Some(stripe[i].as_slice())
                    }
                })
                .collect();
            let mut out = vec![vec![0u8; len]; missing.len()];
            {
                let mut orefs: Vec<&mut [u8]> = out.iter_mut().map(|b| b.as_mut_slice()).collect();
                c.recover(&shards, &missing, &mut orefs)
                    .unwrap_or_else(|e| panic!("k={k} p={p} missing={missing:?}: {e}"));
            }
            for (&x, got) in missing.iter().zip(&out) {
                assert_eq!(got, &stripe[x], "k={k} p={p} shard {x}");
            }
        }
    }
}

#[test]
fn they_reconstruct_our_stripes() {
    let mut rng = Rng(0xC047_0003);
    for &(k, p) in CONFIGS {
        let len = 513;
        let rs = ReedSolomon::new(k, p).expect("rs");
        let c = compat_coder(k, p);
        let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();
        let refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
        let mut parity = vec![vec![0u8; len]; p];
        {
            let mut prefs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
            c.encode(&refs, &mut prefs).expect("our encode");
        }
        // Hand them our stripe with the first min(p, k) DATA shards missing.
        let drop = p.min(k);
        let mut slots: Vec<Option<Vec<u8>>> = (0..k + p)
            .map(|i| {
                if i < drop {
                    None
                } else if i < k {
                    Some(data[i].clone())
                } else {
                    Some(parity[i - k].clone())
                }
            })
            .collect();
        rs.reconstruct(&mut slots)
            .expect("their reconstruct of our stripe");
        for i in 0..drop {
            assert_eq!(
                slots[i].as_ref().expect("rebuilt"),
                &data[i],
                "k={k} p={p} shard {i}"
            );
        }
    }
}

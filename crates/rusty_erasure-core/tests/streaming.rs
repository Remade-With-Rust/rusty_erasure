//! The streaming contract: erasure coding is bytewise, so a stripe processed
//! in arbitrary length-segments is byte-identical to one-shot processing —
//! encode, update, and recover alike. This test IS the streaming API's
//! specification: stream by slicing all shards at the same offsets; no
//! wrapper type needed, no state carried between segments.

use rusty_erasure_core::{Coder, Matrix};

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

#[test]
fn segmented_processing_is_byte_identical_to_one_shot() {
    let mut rng = Rng(0x57AE_A201);
    let (k, p, len) = (10usize, 4usize, 100_003usize); // deliberately odd
    let coder = Coder::new(Matrix::cauchy(k, p).unwrap()).unwrap();
    let data: Vec<Vec<u8>> = (0..k).map(|_| rng.bytes(len)).collect();

    // One-shot reference.
    let mut whole = vec![vec![0u8; len]; p];
    {
        let refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
        let mut prefs: Vec<&mut [u8]> = whole.iter_mut().map(|b| b.as_mut_slice()).collect();
        coder.encode(&refs, &mut prefs).expect("encode");
    }

    // Segmented: uneven cuts, including tiny and vector-width-straddling ones.
    let cuts = [0usize, 1, 31, 32, 4096, 4097, 65_536, 99_990, len];
    let mut seg = vec![vec![0u8; len]; p];
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let refs: Vec<&[u8]> = data.iter().map(|d| &d[a..b]).collect();
        let mut prefs: Vec<&mut [u8]> = seg.iter_mut().map(|s| &mut s[a..b]).collect();
        coder.encode(&refs, &mut prefs).expect("segment encode");
    }
    assert_eq!(seg, whole, "segmented encode != one-shot");

    // Segmented recovery of two lost shards, same cuts.
    let missing = [1usize, k + 2];
    let mut rec = vec![vec![0u8; len]; 2];
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let shards: Vec<Option<&[u8]>> = (0..k + p)
            .map(|i| {
                if missing.contains(&i) {
                    None
                } else if i < k {
                    Some(&data[i][a..b])
                } else {
                    Some(&whole[i - k][a..b])
                }
            })
            .collect();
        let mut orefs: Vec<&mut [u8]> = rec.iter_mut().map(|s| &mut s[a..b]).collect();
        coder.recover(&shards, &missing, &mut orefs).expect("segment recover");
    }
    assert_eq!(rec[0], data[1], "segmented recovery of a source");
    assert_eq!(rec[1], whole[2], "segmented recovery of a parity");
}

#[test]
fn coder_is_send_and_sync_for_multi_stripe_parallelism() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Coder>();
}

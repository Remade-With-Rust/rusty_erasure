//! The dispatched RAID path must be byte-identical to the scalar core (which
//! is itself golden-gated against ISA-L) across edge lengths and source
//! counts — the raid twin of the matches-scalar oracle gates.

use rusty_erasure::raid as facade_raid;
use rusty_erasure_core::raid as core_raid;

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
fn dispatched_raid_matches_core() {
    let mut rng = Rng(0x8A1D_0001);
    for &len in &[0usize, 1, 7, 31, 32, 33, 63, 64, 65, 100, 1017, 65536] {
        for &nsrc in &[2usize, 3, 8, 15] {
            let sources: Vec<Vec<u8>> = (0..nsrc).map(|_| rng.bytes(len)).collect();
            let refs: Vec<&[u8]> = sources.iter().map(|s| s.as_slice()).collect();

            let mut xa = vec![0u8; len];
            let mut xb = vec![0u8; len];
            core_raid::xor_gen(&refs, &mut xa).expect("core xor");
            facade_raid::xor_gen(&refs, &mut xb).expect("dispatched xor");
            assert_eq!(xa, xb, "xor nsrc={nsrc} len={len}");

            let mut pa = vec![0u8; len];
            let mut qa = vec![0u8; len];
            let mut pb = vec![0u8; len];
            let mut qb = vec![0u8; len];
            core_raid::pq_gen(&refs, &mut pa, &mut qa).expect("core pq");
            facade_raid::pq_gen(&refs, &mut pb, &mut qb).expect("dispatched pq");
            assert_eq!(pa, pb, "P nsrc={nsrc} len={len}");
            assert_eq!(qa, qb, "Q nsrc={nsrc} len={len}");

            // And the checkers accept the dispatched outputs.
            assert_eq!(
                core_raid::pq_check(&refs, &pb, &qb),
                Ok(None),
                "nsrc={nsrc} len={len}"
            );
        }
    }
}

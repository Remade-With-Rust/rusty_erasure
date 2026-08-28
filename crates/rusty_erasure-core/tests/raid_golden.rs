//! RAID conformance: replay `corpus/golden/raid_vectors.bin` (kind 0 = real
//! dispatched ISA-L xor_gen/pq_gen on aligned lengths; kind 1 = ISA-L's own
//! byte-wise reference math on odd lengths) byte-for-byte, plus the check
//! functions' contract both ways.

use rusty_erasure_core::raid::{PqParity, pq_check, pq_gen, xor_check, xor_gen};

static VECTORS: &[u8] = include_bytes!("../../../corpus/golden/raid_vectors.bin");

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        s
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().expect("2 bytes"))
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("4 bytes"))
    }
}

#[test]
#[cfg_attr(miri, ignore = "byte-identity replay is deterministic and UB-free; slow interpreted")]
fn raid_matches_isal_vectors() {
    let mut c = Cursor { buf: VECTORS, pos: 0 };
    assert_eq!(c.take(4), b"RRV1", "vector file magic");
    let count = c.u32();
    assert!(count > 0);

    for case in 0..count {
        let kind = c.u8();
        let nsrc = c.u16() as usize;
        let len = c.u32() as usize;
        let sources: Vec<&[u8]> = (0..nsrc).map(|_| c.take(len)).collect();
        let xor_expect = c.take(len);
        let p_expect = c.take(len);
        let q_expect = c.take(len);
        let tag = format!("case {case}: kind={kind} nsrc={nsrc} len={len}");

        let mut xor_out = vec![0u8; len];
        xor_gen(&sources, &mut xor_out).unwrap_or_else(|e| panic!("{tag}: xor_gen {e}"));
        assert_eq!(xor_out.as_slice(), xor_expect, "{tag}: xor parity");

        let mut p = vec![0u8; len];
        let mut q = vec![0u8; len];
        pq_gen(&sources, &mut p, &mut q).unwrap_or_else(|e| panic!("{tag}: pq_gen {e}"));
        assert_eq!(p.as_slice(), p_expect, "{tag}: P");
        assert_eq!(q.as_slice(), q_expect, "{tag}: Q");

        // The checkers agree with the golden state...
        let mut all: Vec<&[u8]> = sources.clone();
        all.push(&xor_out);
        assert_eq!(xor_check(&all), Ok(true), "{tag}: xor_check");
        assert_eq!(pq_check(&sources, &p, &q), Ok(None), "{tag}: pq_check");

        // ...and detect corruption, naming the right parity and offset.
        if len > 2 {
            let hit = len / 2;
            let mut bad_q = q.clone();
            bad_q[hit] ^= 0x20;
            let m = pq_check(&sources, &p, &bad_q).unwrap().expect("must detect");
            assert_eq!((m.index, m.parity), (hit, PqParity::Q), "{tag}");
            let mut bad_p = p.clone();
            bad_p[hit] ^= 0x01;
            let m = pq_check(&sources, &bad_p, &q).unwrap().expect("must detect");
            assert_eq!((m.index, m.parity), (hit, PqParity::P), "{tag}");
            let mut bad_all = all.clone();
            let bad_first: Vec<u8> = sources[0].iter().map(|&b| b ^ 0x80).collect();
            bad_all[0] = &bad_first;
            assert_eq!(xor_check(&bad_all), Ok(false), "{tag}: xor_check corruption");
        }
    }
    assert_eq!(c.pos, VECTORS.len(), "trailing bytes in vector file");
}

#[test]
fn raid_misuse_is_typed_errors() {
    let a = [1u8; 16];
    let short = [1u8; 15];
    let mut out = vec![0u8; 16];
    let mut q = vec![0u8; 16];
    assert!(xor_gen(&[&a], &mut out).is_err(), "one source refused");
    assert!(xor_gen(&[&a, &short], &mut out).is_err(), "length mismatch refused");
    assert!(pq_gen(&[&a], &mut out, &mut q).is_err(), "one source refused");
    let mut q15 = vec![0u8; 15];
    assert!(pq_gen(&[&a, &a], &mut out, &mut q15).is_err(), "p/q length mismatch");
    assert!(xor_check(&[&a]).is_err(), "one vector refused");
    assert!(pq_check(&[&a, &short], &out, &q).is_err(), "length mismatch refused");
    // len-0 is a valid degenerate stripe.
    let e: [u8; 0] = [];
    let mut z: Vec<u8> = vec![];
    let mut z2: Vec<u8> = vec![];
    assert!(pq_gen(&[&e, &e], &mut z, &mut z2).is_ok());
}

//! RAID parity: XOR (P-only) and P+Q (RAID-6) generation and checking —
//! ISA-L's `raid` module semantics with validated, panic-free APIs.
//!
//! Q is the classic RAID-6 syndrome `Q = Σ 2^j · D_j` over GF(2^8)/0x11d,
//! computed Horner-style exactly as ISA-L's `pq_gen_base` (last source first,
//! `q = s ^ 2·q`), using the same word-parallel multiply-by-2 (eight byte
//! lanes per u64: shift, isolate bit-7 lanes, expand to a lane mask, fold the
//! polynomial). Where ISA-L requires 32-byte-multiple lengths and silently
//! ignores sub-word tails in its base code, this port handles ANY length —
//! word blocks plus an exact byte tail — and is gated both ways against real
//! ISA-L (their outputs on aligned lens; their byte-wise checkers accepting
//! our outputs on odd lens).

use crate::error::CodeError;

const NOTBIT0: u64 = 0xfefe_fefe_fefe_fefe;
const BIT7: u64 = 0x8080_8080_8080_8080;
const GF8POLY: u64 = 0x1d1d_1d1d_1d1d_1d1d;

/// Multiply each of the eight GF(2^8) byte lanes of `q` by 2 (ISA-L's
/// word-parallel trick: `(m << 1) - (m >> 7)` turns each 0x80 lane into 0xff).
#[inline]
const fn gf2_mul2_lanes(q: u64) -> u64 {
    let m = q & BIT7;
    ((q << 1) & NOTBIT0) ^ ((((m << 1).wrapping_sub(m >> 7)) & GF8POLY))
}

#[inline]
const fn gf2_mul2_byte(q: u8) -> u8 {
    (q << 1) ^ (if q & 0x80 != 0 { 0x1d } else { 0 })
}

fn check_lens(sources: &[&[u8]], len: usize, min_sources: usize) -> Result<(), CodeError> {
    if sources.len() < min_sources {
        return Err(CodeError::ShardCount { expected: min_sources, got: sources.len() });
    }
    for (index, s) in sources.iter().enumerate() {
        if s.len() != len {
            return Err(CodeError::ShardLength { index, expected: len, got: s.len() });
        }
    }
    Ok(())
}

/// XOR parity of ≥2 sources into `parity` — ISA-L `xor_gen`.
///
/// Deliberately the multi-pass shape: `copy_from_slice` then one
/// auto-vectorized XOR pass per source. A single-pass u64×4 fold was tried
/// (parity written once instead of N times) and MEASURED WORSE (−8%,
/// LEDGER): the per-source passes are memcpy-class vectorized streams, and
/// sequential parity stores are nearly free — the classic
/// redundant-but-cheaper-than-the-fix case.
pub fn xor_gen(sources: &[&[u8]], parity: &mut [u8]) -> Result<(), CodeError> {
    check_lens(sources, parity.len(), 2)?;
    let (first, rest) = sources.split_first().expect("count checked");
    parity.copy_from_slice(first);
    for src in rest {
        for (d, &s) in parity.iter_mut().zip(*src) {
            *d ^= s;
        }
    }
    Ok(())
}

/// True when the XOR of ALL vectors (parity included) is zero — ISA-L
/// `xor_check`. Word-wide with per-block early exit.
pub fn xor_check(vects: &[&[u8]]) -> Result<bool, CodeError> {
    let len = vects.first().map_or(0, |v| v.len());
    check_lens(vects, len, 2)?;
    let words = len / 8;
    for i in 0..words {
        let o = i * 8;
        let mut acc = 0u64;
        for v in vects {
            acc ^= u64::from_ne_bytes(v[o..o + 8].try_into().expect("in range"));
        }
        if acc != 0 {
            return Ok(false);
        }
    }
    for i in words * 8..len {
        let mut acc = 0u8;
        for v in vects {
            acc ^= v[i];
        }
        if acc != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// RAID-6 P+Q of ≥2 sources — ISA-L `pq_gen`, any length.
pub fn pq_gen(sources: &[&[u8]], p: &mut [u8], q: &mut [u8]) -> Result<(), CodeError> {
    let len = p.len();
    if q.len() != len {
        return Err(CodeError::ShardLength { index: 1, expected: len, got: q.len() });
    }
    check_lens(sources, len, 2)?;

    // 32-byte blocks: four independent u64 lanes per step (brick) — the ×2
    // recurrence chains run in parallel across lanes and the shift/mask/poly
    // trick becomes a 4-wide pattern the compiler can vectorize; per-word
    // closure bounds checks collapse to one slice per block.
    let last = sources.len() - 1;
    let blocks = len / 32;
    for i in 0..blocks {
        let o = i * 32;
        let load = |s: &[u8], w: usize| {
            u64::from_ne_bytes(s[o + w * 8..o + w * 8 + 8].try_into().expect("in range"))
        };
        let mut pw = [0u64; 4];
        let mut qw = [0u64; 4];
        for w in 0..4 {
            pw[w] = load(sources[last], w);
            qw[w] = pw[w];
        }
        for j in (0..last).rev() {
            for w in 0..4 {
                let s = load(sources[j], w);
                pw[w] ^= s;
                qw[w] = s ^ gf2_mul2_lanes(qw[w]);
            }
        }
        for w in 0..4 {
            p[o + w * 8..o + w * 8 + 8].copy_from_slice(&pw[w].to_ne_bytes());
            q[o + w * 8..o + w * 8 + 8].copy_from_slice(&qw[w].to_ne_bytes());
        }
    }
    // Byte tail — the part ISA-L's base quietly ignores; ours is exact.
    for i in blocks * 32..len {
        let last = sources.len() - 1;
        let mut pb = sources[last][i];
        let mut qb = pb;
        for j in (0..last).rev() {
            let s = sources[j][i];
            pb ^= s;
            qb = s ^ gf2_mul2_byte(qb);
        }
        p[i] = pb;
        q[i] = qb;
    }
    Ok(())
}

/// Which parity vector a [`pq_check`] mismatch was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PqParity {
    /// The XOR parity (P) disagreed.
    P,
    /// The RAID-6 syndrome (Q) disagreed.
    Q,
}

/// A P/Q consistency failure: the first offending byte offset and which
/// parity it disagreed with (ISA-L's `i | 1` / `i | 2` return, made typed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PqMismatch {
    /// Byte offset of the first mismatch.
    pub index: usize,
    /// Which parity vector disagreed.
    pub parity: PqParity,
}

/// Check sources against P and Q — ISA-L `pq_check`. `Ok(None)` means
/// consistent; `Ok(Some(_))` names the first mismatch.
pub fn pq_check(
    sources: &[&[u8]],
    p: &[u8],
    q: &[u8],
) -> Result<Option<PqMismatch>, CodeError> {
    let len = p.len();
    if q.len() != len {
        return Err(CodeError::ShardLength { index: 1, expected: len, got: q.len() });
    }
    check_lens(sources, len, 2)?;
    let last = sources.len() - 1;

    // Exact byte-wise check over one range (the reference semantics: first
    // mismatching byte, P examined before Q at each offset).
    let byte_scan = |from: usize, to: usize| -> Option<PqMismatch> {
        for i in from..to {
            let mut pb = sources[last][i];
            let mut qb = pb;
            for j in (0..last).rev() {
                let s = sources[j][i];
                pb ^= s;
                qb = s ^ gf2_mul2_byte(qb);
            }
            if p[i] != pb {
                return Some(PqMismatch { index: i, parity: PqParity::P });
            }
            if q[i] != qb {
                return Some(PqMismatch { index: i, parity: PqParity::Q });
            }
        }
        None
    };

    // Word-wide fast scan (brick: 8 byte lanes per recurrence step); a block
    // that disagrees falls back to the byte scan for the exact index and the
    // exact P-before-Q ordering.
    let words = len / 8;
    for i in 0..words {
        let o = i * 8;
        let load = |s: &[u8]| u64::from_ne_bytes(s[o..o + 8].try_into().expect("in range"));
        let mut pw = load(sources[last]);
        let mut qw = pw;
        for j in (0..last).rev() {
            let s = load(sources[j]);
            pw ^= s;
            qw = s ^ gf2_mul2_lanes(qw);
        }
        if pw != load(p) || qw != load(q) {
            return Ok(byte_scan(o, o + 8));
        }
    }
    Ok(byte_scan(words * 8, len))
}

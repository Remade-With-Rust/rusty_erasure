//! GF(2^8) arithmetic over the field ISA-L uses: the polynomial
//! x^8 + x^4 + x^3 + x^2 + 1 (0x11d), generator 2.
//!
//! Two independent constructions live here on purpose:
//!
//! - [`mul`]/[`inv`] — log/exp table lookups, tables **const-generated at
//!   compile time** from the polynomial (no runtime init, no `OnceLock`).
//! - [`mul_shift`] — carry-less shift-and-XOR multiply reduced by the
//!   polynomial, table-free. The slow in-crate twin.
//!
//! The conformance tests pin both against ISA-L's own `gf_mul_table_base` /
//! `gf_inv_table_base` (checked in as golden data, all 65,536 products and all
//! 256 inverses), so the field — polynomial included — is proven, not trusted.

/// The reduction byte of the field polynomial: overflow of a doubling is
/// folded back with `^ 0x1d` (the low 8 bits of 0x11d).
pub const POLY_REDUCTION: u8 = 0x1d;

/// Double a field element (multiply by the generator, 2).
#[inline]
const fn double(x: u8) -> u8 {
    (x << 1) ^ (if x & 0x80 != 0 { POLY_REDUCTION } else { 0 })
}

/// Exponent table, doubled to 510 entries so `log(a) + log(b)` (max 508)
/// indexes directly with no modulo and no branch.
const fn build_exp() -> [u8; 510] {
    let mut exp = [0u8; 510];
    let mut x: u8 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x;
        exp[i + 255] = x;
        x = double(x);
        i += 1;
    }
    exp
}

const fn build_log(exp: &[u8; 510]) -> [u8; 256] {
    let mut log = [0u8; 256];
    let mut i = 0;
    while i < 255 {
        log[exp[i] as usize] = i as u8;
        i += 1;
    }
    log
}

const EXP: [u8; 510] = build_exp();
const LOG: [u8; 256] = build_log(&EXP);

/// Multiply two field elements. Byte-identical to ISA-L's `gf_mul` for every
/// input pair (proven exhaustively against the golden table).
#[inline]
pub const fn mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    EXP[LOG[a as usize] as usize + LOG[b as usize] as usize]
}

/// Multiplicative inverse of a field element.
///
/// Faithful to ISA-L's `gf_inv`: **`inv(0)` returns 0** rather than being an
/// error — zero has no inverse, and callers that could reach it must guard
/// (the Cauchy construction and the pivot logic in `matrix` never do).
#[inline]
pub const fn inv(a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    EXP[255 - LOG[a as usize] as usize]
}

/// Table-free multiply: carry-less shift-and-XOR reduced by the polynomial.
///
/// The independent slow twin of [`mul`], kept forever as an in-crate oracle
/// (the tests pin `mul_shift == mul` over all 65,536 pairs, so the const
/// tables can never silently drift from the polynomial).
pub const fn mul_shift(mut a: u8, mut b: u8) -> u8 {
    let mut r: u8 = 0;
    while b != 0 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = double(a);
        b >>= 1;
    }
    r
}

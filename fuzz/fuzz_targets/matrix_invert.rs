//! Inversion of arbitrary matrices must never panic — and when it succeeds,
//! the result must actually be the inverse.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rusty_erasure_core::Matrix;

fuzz_target!(|data: &[u8]| {
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
});

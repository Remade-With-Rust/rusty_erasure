//! Matrix generation must never panic, for ANY (k, p) — errors are typed.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rusty_erasure_core::Matrix;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    let k = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let p = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let _ = Matrix::reed_solomon(k, p);
    let _ = Matrix::cauchy(k, p);
});

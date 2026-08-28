//! The compat layer's no-panic contract: every `isal::*` entry point, fed
//! arbitrary parameters and undersized buffers, must return a typed error or
//! succeed — never panic, never read out of bounds (ASan watches the latter).

#![no_main]

use libfuzzer_sys::fuzz_target;
use rusty_erasure::isal;

fuzz_target!(|input: &[u8]| {
    if input.len() < 8 {
        return;
    }
    let k = input[0] as usize;
    let m = input[1] as usize;
    let rows = input[2] as usize % 16;
    let len = input[3] as usize;
    let vec_i = input[4] as usize;
    let n = input[5] as usize % 24;
    let payload = &input[8..];

    // Matrix generation into a buffer the fuzzer sizes (possibly too small).
    let mut a = vec![0u8; payload.len().min(4096)];
    let _ = isal::gf_gen_rs_matrix(&mut a, m, k);
    let _ = isal::gf_gen_cauchy1_matrix(&mut a, m, k);

    // Inversion of arbitrary bytes at an arbitrary claimed dimension.
    let mut inv = vec![0u8; a.len()];
    let mut scratch = a.clone();
    let _ = isal::gf_invert_matrix(&mut scratch, &mut inv, n);

    // Table expansion with whatever coefficients and (k, rows) claim.
    let mut gftbls = vec![0u8; payload.len().min(8192)];
    let _ = isal::ec_init_tables(k, rows, payload, &mut gftbls);

    // Encode / update / kernels over fuzzer-shaped buffers.
    let shard = payload.len().min(64);
    let nsrc = (k % 8).max(1);
    let srcs: Vec<Vec<u8>> = (0..nsrc).map(|i| {
        let start = (i * shard).min(payload.len());
        let end = ((i + 1) * shard).min(payload.len());
        payload[start..end].to_vec()
    }).collect();
    let src_refs: Vec<&[u8]> = srcs.iter().map(|s| s.as_slice()).collect();
    let mut coding = vec![vec![0u8; shard]; rows.max(1)];
    let mut coding_refs: Vec<&mut [u8]> =
        coding.iter_mut().map(|b| b.as_mut_slice()).collect();

    let _ = isal::ec_encode_data(len, nsrc, rows, &gftbls, &src_refs, &mut coding_refs);
    let _ = isal::ec_encode_data_update(len, nsrc, rows, vec_i, &gftbls, payload, &mut coding_refs);

    let mut dest = vec![0u8; shard];
    let _ = isal::gf_vect_dot_prod(len, nsrc, &gftbls, &src_refs, &mut dest);
    let _ = isal::gf_vect_mad(len, nsrc, vec_i, &gftbls, payload, &mut dest);
    let _ = isal::gf_vect_mul(len, &gftbls, payload, &mut dest);
});

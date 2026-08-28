//! The wasm demo deliverable: erasure coding in the browser — encode a
//! stripe, lose shards, recover, all inside the page. No wasm-bindgen, no JS
//! framework: four `extern "C"` exports and ~80 lines of hand-written glue
//! (`web/index.html`), zero `unsafe` (the kernel name crosses the boundary as
//! a pointer+len pair into linear memory; the host only reads).
//!
//! Build: `RUSTFLAGS="-C target-feature=+simd128" cargo build --release
//! -p rusty_erasure-demo --target wasm32-unknown-unknown`
//! Verify headless: `node tools/check_demo.mjs` (also run by the M6 gate).

#[global_allocator]
static ALLOC: rusty_erasure_alloc::HouseAllocator = rusty_erasure_alloc::house_allocator();

use rusty_erasure::{Matrix, best_kernels, coder};

fn stripe(k: usize, len: usize) -> Vec<Vec<u8>> {
    let mut state: u64 = ((k as u64) << 32) | len as u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    (0..k).map(|_| (0..len).map(|_| next() as u8).collect()).collect()
}

/// Encode `k+p` over `len`-byte shards, drop `drop_n` shards (spread across
/// sources and parity), recover them, and compare against ground truth.
/// Returns 0 on success; a negative code names the failing stage.
#[unsafe(no_mangle)]
pub extern "C" fn demo_roundtrip(k: usize, p: usize, len: usize, drop_n: usize) -> i32 {
    let Ok(matrix) = Matrix::cauchy(k, p) else { return -1 };
    let Ok(c) = coder(matrix) else { return -2 };
    if drop_n > p {
        return -3;
    }
    let data = stripe(k, len);
    let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
    let mut parity = vec![vec![0u8; len]; p];
    {
        let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
        if c.encode(&data_refs, &mut refs).is_err() {
            return -4;
        }
    }
    // Drop every other shard index until drop_n are gone.
    let n = k + p;
    let missing: Vec<usize> = (0..n).step_by(2).chain((1..n).step_by(2)).take(drop_n).collect();
    let mut missing = missing;
    missing.sort_unstable();
    let shards: Vec<Option<&[u8]>> = (0..n)
        .map(|i| {
            if missing.contains(&i) {
                None
            } else if i < k {
                Some(data[i].as_slice())
            } else {
                Some(parity[i - k].as_slice())
            }
        })
        .collect();
    let mut out = vec![vec![0u8; len]; missing.len()];
    {
        let mut refs: Vec<&mut [u8]> = out.iter_mut().map(|b| b.as_mut_slice()).collect();
        if c.recover(&shards, &missing, &mut refs).is_err() {
            return -5;
        }
    }
    for (&x, got) in missing.iter().zip(&out) {
        let expect: &[u8] = if x < k { &data[x] } else { &parity[x - k] };
        if got.as_slice() != expect {
            return -6;
        }
    }
    0
}

/// Encode the same stripe `reps` times (the host times the call and derives
/// MB/s). Returns a parity checksum so the work cannot be optimized away.
#[unsafe(no_mangle)]
pub extern "C" fn demo_bench(k: usize, p: usize, len: usize, reps: usize) -> i32 {
    let Ok(matrix) = Matrix::cauchy(k, p) else { return -1 };
    let Ok(c) = coder(matrix) else { return -1 };
    let data = stripe(k, len);
    let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
    let mut parity = vec![vec![0u8; len]; p];
    let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
    for _ in 0..reps {
        if c.encode(&data_refs, &mut refs).is_err() {
            return -1;
        }
    }
    drop(refs);
    i32::from(parity.iter().flatten().fold(0u8, |a, &b| a ^ b))
}

/// Pointer to the active kernel-set name in linear memory (static, read-only).
#[unsafe(no_mangle)]
pub extern "C" fn demo_kernel_name_ptr() -> *const u8 {
    best_kernels().name.as_ptr()
}

/// Length of the kernel-set name.
#[unsafe(no_mangle)]
pub extern "C" fn demo_kernel_name_len() -> usize {
    best_kernels().name.len()
}

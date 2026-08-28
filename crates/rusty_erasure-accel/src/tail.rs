//! Arch-shared scalar tail for nibble-table encode kernels: finish bytes
//! `[i..len)` exactly as the scalar oracle would. Used by the x86, aarch64,
//! and wasm kernels so every architecture's tail is the same proven code.

use rusty_erasure_core::tables::{TABLE_BYTES, table_mul};

pub(crate) fn encode_tail_nibble(gftbls: &[u8], data: &[&[u8]], out: &mut [&mut [u8]], i: usize) {
    let k = data.len();
    for (r, dest) in out.iter_mut().enumerate() {
        for (x, d) in dest[i..].iter_mut().enumerate() {
            let mut acc = 0u8;
            for (j, src) in data.iter().enumerate() {
                let start = (r * k + j) * TABLE_BYTES;
                let tbl: &[u8; TABLE_BYTES] = gftbls[start..start + TABLE_BYTES]
                    .try_into()
                    .expect("checked by wrapper");
                acc ^= table_mul(tbl, src[i + x]);
            }
            *d = acc;
        }
    }
}

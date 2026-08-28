//! Delegates to the shared harness body in `rusty_erasure-fuzzlib` so the
//! libFuzzer run and the cross-architecture corpus replay execute IDENTICAL
//! code. See that crate's docs for why the split exists.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| rusty_erasure_fuzzlib::matrix_gen(data));

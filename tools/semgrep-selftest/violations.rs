//! Deliberate violations of every rule in `tools/semgrep-rules.yml`.
//!
//! This file is NOT part of the workspace and is never compiled. It exists so
//! CI can assert the rules still FIRE. A static-analysis rule that has quietly
//! stopped matching is worse than no rule: it reports safety it is not
//! checking. `tools/run_semgrep.sh` runs the ruleset twice — once over the
//! real tree, which must be clean, and once over this file, which must
//! produce a finding for every rule.

// unsafe-outside-accel
pub fn violation_unsafe_outside_accel(p: *const u8) -> u8 {
    unsafe { *p }
}

// global-allocator-in-library
#[global_allocator]
static VIOLATION_ALLOC: std::alloc::System = std::alloc::System;

// kernel-dispatch-inside-loop
pub fn violation_dispatch_in_loop(n: usize) {
    for _i in 0..n {
        let _k = best_kernels();
    }
}

// nibble-tables-into-gfni-kernel
pub fn violation_mixed_table_formats(coeffs: &[u8], data: &[&[u8]], out: &mut [&mut [u8]]) {
    let g = tables::init_tables(coeffs);
    imp::encode_gfni(g, data, out);
}

/// parity-described-as-authentication: this parity value authenticates the shard.
pub fn violation_parity_doc() {}

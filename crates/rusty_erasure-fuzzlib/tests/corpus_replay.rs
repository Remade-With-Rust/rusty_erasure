//! Cross-architecture corpus replay — the gate that makes "fuzzed" mean
//! something on wasm and aarch64.
//!
//! libFuzzer runs on x86-64 only. Without this, every finding the fuzzer made
//! would be a statement about the AVX2/GFNI kernels and say nothing about
//! SIMD128 or NEON — different unsafe code, different unroll boundaries,
//! reached by the same inputs. This test replays every corpus input the
//! fuzzer ever kept, through the same harness bodies, on whatever
//! architecture it is built for:
//!
//! - native x86-64: `cargo test`
//! - wasm32-wasip1 under wasmtime (`+simd128`)
//! - aarch64-unknown-linux-musl under qemu
//!
//! The corpus is embedded at build time (see `build.rs`), so no filesystem is
//! needed at runtime — which is precisely what lets it run under wasm.
//!
//! A panic anywhere in a body is the finding; the bodies also assert
//! dispatched-vs-scalar byte identity, so a kernel that disagrees on any
//! historical fuzz input fails here.

use rusty_erasure_fuzzlib::{corpus, run_case};

#[test]
// The asserted value IS a build-time constant, and that is the point: it is
// how many inputs `build.rs` embedded. A zero here means the gate is vacuous.
#[allow(clippy::assertions_on_constants)]
fn every_corpus_input_replays_clean() {
    assert!(
        corpus::CORPUS_LEN > 0,
        "the embedded corpus is EMPTY — build.rs found no inputs, so this \
         gate is vacuous. Check fuzz/corpus/ is present and committed."
    );

    let mut per_target = std::collections::BTreeMap::<&str, usize>::new();
    for &(target, case, bytes) in corpus::CORPUS {
        // Any panic here fails the test and names the exact case.
        run_case(target, bytes);
        *per_target.entry(target).or_default() += 1;
        // Keep the failure message useful without printing 700 lines.
        let _ = case;
    }

    let summary: Vec<String> = per_target.iter().map(|(t, n)| format!("{t}={n}")).collect();
    eprintln!(
        "corpus replay: {} inputs across {} targets ({})",
        corpus::CORPUS_LEN,
        per_target.len(),
        summary.join(" ")
    );
}

/// Past crashers stay fixed: the timeout the fuzzer found in the roundtrip
/// harness's own loss-selection walk (a fixed point in `(s*31+17) % n`) must
/// replay instantly. This is the regression test the hardening gate requires
/// for every historical finding.
#[test]
fn past_findings_replay_without_hanging() {
    // The artifact's shape: k=7-ish stripe with the seed that used to spin.
    // Bounded Fisher-Yates makes selection provably finite, so this returns.
    for seed in 0u8..=255 {
        let input = [7u8, 3, 8, seed, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        rusty_erasure_fuzzlib::roundtrip(&input);
    }
}

/// A seeded pseudo-random sweep — wasm and aarch64 get their OWN randomized
/// coverage, not only a replay of what x86 happened to discover.
///
/// Corpus replay is necessary but not sufficient: libFuzzer's corpus is
/// shaped by coverage feedback from the AVX2/GFNI kernels, so it
/// systematically under-samples the input shapes that matter for a 16-byte
/// SIMD128 or NEON unroll. This closes that by generating inputs
/// deterministically on whatever machine is running the test.
///
/// Scale it with `RUSTY_ERASURE_SWEEP` (default 2000 per target); the nightly
/// CI job can afford a much larger number than a laptop `cargo test`.
#[test]
fn seeded_random_sweep_never_panics() {
    let n: u64 = std::env::var("RUSTY_ERASURE_SWEEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);

    let mut state: u64 = 0x5EED_1234_ABCD_0001;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };

    let targets = [
        "roundtrip",
        "compat",
        "raid",
        "kernels",
        "matrix_gen",
        "matrix_invert",
    ];
    for target in targets {
        for _ in 0..n {
            // Lengths biased small (most shapes live there) but reaching past
            // the 128-byte quad-chunk tier the newest kernels unroll to.
            let len = (next() % 300) as usize;
            let input: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            run_case(target, &input);
        }
    }
    eprintln!(
        "seeded sweep: {} inputs per target across {} targets",
        n,
        targets.len()
    );
}

/// The harness bodies must be total: no byte string may panic them. This is a
/// cheap deterministic sweep on top of the corpus, covering short and
/// degenerate inputs that a coverage-guided fuzzer prunes away.
#[test]
fn short_and_degenerate_inputs_never_panic() {
    let targets = [
        "matrix_gen",
        "matrix_invert",
        "roundtrip",
        "compat",
        "raid",
        "kernels",
    ];
    for target in targets {
        for len in 0usize..40 {
            for pattern in [0u8, 1, 0x7f, 0x80, 0xff] {
                let input = vec![pattern; len];
                run_case(target, &input);
            }
            // A counting pattern exercises different dimension choices.
            let input: Vec<u8> = (0..len).map(|i| i as u8).collect();
            run_case(target, &input);
        }
    }
}

//! Embeds a SNAPSHOT of the fuzz corpus into the replay test at build time.
//!
//! Two reasons this is a snapshot copied into `OUT_DIR` rather than
//! `include_bytes!` pointed at `fuzz/corpus` directly:
//!
//! 1. **wasm has no filesystem.** Reading the corpus at runtime would work
//!    natively and fail under wasmtime (cargo runs test binaries from the
//!    package directory, so `../../fuzz/corpus` escapes any preopened dir).
//!    Embedding makes the replay run identically on every target we ship.
//! 2. **libFuzzer mutates the corpus while it runs.** Its REDUCE pass deletes
//!    superseded inputs, so a path captured during the build can be gone by
//!    the time rustc reads it — a build failure that appears only when a fuzz
//!    campaign happens to be running. Copying under `OUT_DIR` makes the
//!    embedded set immutable once the build starts.

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets this");
    let out_dir = env::var("OUT_DIR").expect("cargo sets this");
    let snapshot = Path::new(&out_dir).join("corpus_snapshot");

    // Fresh snapshot every build so removed inputs do not linger.
    let _ = fs::remove_dir_all(&snapshot);
    fs::create_dir_all(&snapshot).expect("OUT_DIR is writable");

    let mut out = String::from(
        "/// Every fuzz-corpus input, embedded: (target, case name, bytes).\n\
         pub static CORPUS: &[(&str, &str, &[u8])] = &[\n",
    );
    let mut count = 0usize;

    // `artifacts` holds every reproducer the fuzzer ever produced. Replaying
    // those is what makes "every past crasher is a regression test" true by
    // construction rather than by someone remembering to write one.
    let mut targets: Vec<_> = ["../../fuzz/corpus", "../../fuzz/artifacts"]
        .iter()
        .flat_map(|root| {
            let root = Path::new(&manifest).join(root);
            println!("cargo:rerun-if-changed={}", root.display());
            fs::read_dir(root).into_iter().flatten().flatten()
        })
        .filter(|e| e.path().is_dir())
        .collect();
    targets.sort_by_key(std::fs::DirEntry::path);

    for target in targets {
        let name = target.file_name().to_string_lossy().into_owned();
        println!("cargo:rerun-if-changed={}", target.path().display());
        let target_snapshot = snapshot.join(&name);
        fs::create_dir_all(&target_snapshot).expect("OUT_DIR is writable");

        let mut cases: Vec<_> = fs::read_dir(target.path())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_file())
            .collect();
        cases.sort_by_key(std::fs::DirEntry::path);

        for case in cases {
            let case_name = case.file_name().to_string_lossy().into_owned();
            // A concurrent fuzz run may delete an input between the listing
            // and the read; skipping it is correct — it is by definition an
            // input the fuzzer just decided was redundant.
            let Ok(bytes) = fs::read(case.path()) else {
                continue;
            };
            let dest = target_snapshot.join(&case_name);
            fs::write(&dest, &bytes).expect("OUT_DIR is writable");
            writeln!(
                out,
                "    ({:?}, {:?}, include_bytes!({:?})),",
                name,
                case_name,
                dest.to_string_lossy()
            )
            .expect("writing to a String cannot fail");
            count += 1;
        }
    }
    out.push_str("];\n");
    writeln!(
        out,
        "/// Number of embedded corpus inputs.\npub const CORPUS_LEN: usize = {count};"
    )
    .expect("writing to a String cannot fail");

    fs::write(Path::new(&out_dir).join("corpus.rs"), out).expect("OUT_DIR is writable");
}

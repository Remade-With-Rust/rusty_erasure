//! `rerasure` — the rusty_erasure CLI. Consumer #1 of the library API: every
//! capability is an op callable by this binary, a test, and an agent before it
//! is anything else (mission plan §5).

#[global_allocator]
static ALLOC: rusty_erasure_alloc::HouseAllocator = rusty_erasure_alloc::house_allocator();

use std::process::ExitCode;

const USAGE: &str = "\
rerasure — erasure coding, remade with Rust (pre-release scaffold)

USAGE:
    rerasure <VERB> [OPTIONS]

VERBS:
    encode      encode k source shards into p parity shards        (lands M2/M3)
    recover     rebuild missing shards from survivors              (lands M2/M3)
    verify      check parity consistency                           (lands M3)
    bench       ERASCORP cells with the full method line           (lands M4)
    census      kernel-reach census: % of bytes per dispatch path  (lands M4)
    help        print this message

Mission plan: docs/plans/erasure_mission.md. No claim without a ledger entry.";

/// Verbs the scaffold knows about but whose implementation lands at a named
/// milestone. Exit code 3 distinguishes "not built yet" from usage errors (2).
const PENDING: &[(&str, &str)] = &[
    ("encode", "M2/M3"),
    ("recover", "M2/M3"),
    ("verify", "M3"),
    ("bench", "M4"),
    ("census", "M4"),
];

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(verb) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match verb.as_str() {
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        v => match PENDING.iter().find(|(name, _)| *name == v) {
            Some((name, milestone)) => {
                eprintln!(
                    "rerasure {name}: not built yet — lands at {milestone} \
                     (docs/plans/erasure_mission.md §8)"
                );
                ExitCode::from(3)
            }
            None => {
                eprintln!("rerasure: unknown verb '{v}'\n\n{USAGE}");
                ExitCode::from(2)
            }
        },
    }
}

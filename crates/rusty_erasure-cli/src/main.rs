//! `rerasure` — the rusty_erasure CLI. Consumer #1 of the library API: every
//! capability is an op callable by this binary, a test, and an agent before it
//! is anything else (mission plan §5).

#[global_allocator]
static ALLOC: rusty_erasure_alloc::HouseAllocator = rusty_erasure_alloc::house_allocator();

use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

use rusty_erasure::{Coder, Matrix, census, coder, kernels_named};

const USAGE: &str = "\
rerasure — erasure coding, remade with Rust

USAGE:
    rerasure <VERB> [OPTIONS]

VERBS:
    bench       encode benchmark on one cell; prints deterministic work counts,
                per-rep wall stats, a checksum, and the kernel census.
                  --k N --p N --len N --reps N   (default 10 4 65536 6000)
                  --kernels auto|scalar|ssse3|avx2|gfni   (default auto)
    census      run a fixed workload through the shipping path and print the
                kernel-reach census; exits 1 if a SIMD set was selected but
                did not carry 100% of the bytes
    encode      encode k source shards into p parity shards       (lands M7, streaming API)
    recover     rebuild missing shards from survivors             (lands M7, streaming API)
    verify      check parity consistency                          (lands M7, streaming API)
    help        print this message

Mission plan: docs/plans/erasure_mission.md. No claim without a ledger entry.";

fn parse_flags(args: &[String]) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let Some(name) = a.strip_prefix("--") else {
            return Err(format!("unexpected argument '{a}'"));
        };
        let Some(v) = it.next() else {
            return Err(format!("--{name} needs a value"));
        };
        map.insert(name.to_string(), v.clone());
    }
    Ok(map)
}

fn get_usize(
    flags: &std::collections::HashMap<String, String>,
    name: &str,
    default: usize,
) -> Result<usize, String> {
    match flags.get(name) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| format!("--{name}: '{v}' is not a number")),
    }
}

fn build_stripe(k: usize, len: usize) -> Vec<Vec<u8>> {
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

fn bench(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;
    let k = get_usize(&flags, "k", 10)?;
    let p = get_usize(&flags, "p", 4)?;
    let len = get_usize(&flags, "len", 65536)?;
    let reps = get_usize(&flags, "reps", 6000)?;
    let stripes = get_usize(&flags, "stripes", 1)?;
    let threads = get_usize(&flags, "threads", 1)?;
    let kernels_arg = flags.get("kernels").map(String::as_str).unwrap_or("auto");
    if stripes > 1 || threads > 1 {
        return bench_stripes(k, p, len, reps, stripes, threads.max(1), kernels_arg);
    }

    let matrix = Matrix::cauchy(k, p).map_err(|e| e.to_string())?;
    let Some(kern) = kernels_named(kernels_arg) else {
        return Err(format!(
            "--kernels: '{kernels_arg}' unknown or unsupported on this CPU \
             (want auto|scalar|ssse3|avx2|gfni)"
        ));
    };
    let c = Coder::with_kernels(matrix, kern).map_err(|e| e.to_string())?;

    let data = build_stripe(k, len);
    let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
    let mut parity = vec![vec![0u8; len]; p];

    // The ref vectors are built ONCE, outside the timed loop, so the bench
    // measures the encode path rather than per-rep Vec construction.
    // (Measured: ~2% on the smallest cell — kept because a probe should not
    // allocate per iteration on principle, not because it was the tax.)
    let mut per_rep_ns: Vec<u128> = Vec::with_capacity(reps);
    let wall;
    {
        let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
        for _ in 0..3 {
            c.encode(&data_refs, &mut refs).map_err(|e| e.to_string())?;
        }
        let total = Instant::now();
        for _ in 0..reps {
            let t = Instant::now();
            c.encode(&data_refs, black_box(&mut refs)).map_err(|e| e.to_string())?;
            per_rep_ns.push(t.elapsed().as_nanos());
        }
        wall = total.elapsed();
    }
    black_box(&parity);

    let checksum = parity.iter().flatten().fold(0u8, |a, &b| a ^ b);
    per_rep_ns.sort_unstable();
    let src_bytes = (k * len) as u128 * reps as u128;
    println!("cell k={k} p={p} len={len} reps={reps} kernels={}", c.kernels().name);
    println!(
        "work: source_bytes={src_bytes} table_muls={} checksum={checksum:#04x}",
        (k * p * len) as u128 * reps as u128
    );
    println!(
        "wall: total_ms={} rep_min_us={} rep_median_us={} (informational; the harness derives GB/s from process CPU time)",
        wall.as_millis(),
        per_rep_ns.first().unwrap_or(&0) / 1000,
        per_rep_ns.get(reps / 2).unwrap_or(&0) / 1000,
    );
    let cen = census::read();
    println!(
        "census: scalar_bytes={} accel_bytes={} accel_pct={}",
        cen.scalar_bytes,
        cen.accel_bytes,
        cen.accel_percent().map_or_else(|| "n/a".into(), |v| format!("{v:.2}%")),
    );
    Ok(())
}

/// Multi-stripe throughput (ERASCORP S10): independent stripes split across
/// `threads` OS threads via `std::thread::scope` — one shared `&Coder` (it is
/// `Sync`), each thread owning its stripes' buffers. No dependency needed for
/// embarrassing parallelism; callers wanting a pool can wrap the same shape.
fn bench_stripes(
    k: usize,
    p: usize,
    len: usize,
    reps: usize,
    stripes: usize,
    threads: usize,
    kernels_arg: &str,
) -> Result<(), String> {
    let matrix = Matrix::cauchy(k, p).map_err(|e| e.to_string())?;
    let Some(kern) = kernels_named(kernels_arg) else {
        return Err(format!("--kernels: '{kernels_arg}' unknown or unsupported"));
    };
    let c = Coder::with_kernels(matrix, kern).map_err(|e| e.to_string())?;

    let mut all: Vec<(Vec<Vec<u8>>, Vec<Vec<u8>>)> = (0..stripes)
        .map(|_| (build_stripe(k, len), vec![vec![0u8; len]; p]))
        .collect();

    let t0 = Instant::now();
    std::thread::scope(|s| {
        let per = stripes.div_ceil(threads);
        for chunk in all.chunks_mut(per) {
            let c = &c;
            s.spawn(move || {
                for (data, parity) in chunk.iter_mut() {
                    let refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
                    let mut prefs: Vec<&mut [u8]> =
                        parity.iter_mut().map(|b| b.as_mut_slice()).collect();
                    for _ in 0..reps {
                        c.encode(&refs, black_box(&mut prefs)).expect("validated");
                    }
                }
            });
        }
    });
    let wall = t0.elapsed().as_secs_f64();
    let src_bytes = (k * len) as f64 * reps as f64 * stripes as f64;
    let checksum = all
        .iter()
        .flat_map(|(_, par)| par.iter().flatten())
        .fold(0u8, |a, &b| a ^ b);
    println!(
        "stripes cell k={k} p={p} len={len} reps={reps} stripes={stripes} threads={threads} kernels={}",
        c.kernels().name
    );
    println!(
        "aggregate: wall_s={wall:.2} src_GBps={:.3} checksum={checksum:#04x} (wall basis — aggregate multi-thread throughput)",
        src_bytes / wall / 1e9
    );
    Ok(())
}

fn census_verb() -> ExitCode {
    let matrix = Matrix::cauchy(10, 4).expect("valid config");
    let c = coder(matrix).expect("has parity");
    let chosen = c.kernels().name;
    let data = build_stripe(10, 65536);
    let data_refs: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
    let mut parity = vec![vec![0u8; 65536]; 4];
    for _ in 0..200 {
        let mut refs: Vec<&mut [u8]> = parity.iter_mut().map(|b| b.as_mut_slice()).collect();
        c.encode(&data_refs, &mut refs).expect("validated");
    }
    // Recovery exercises the same kernel seam.
    let shards: Vec<Option<&[u8]>> = (0..14)
        .map(|i| {
            if i < 2 {
                None
            } else if i < 10 {
                Some(data[i].as_slice())
            } else {
                Some(parity[i - 10].as_slice())
            }
        })
        .collect();
    let mut out = vec![vec![0u8; 65536]; 2];
    let mut orefs: Vec<&mut [u8]> = out.iter_mut().map(|b| b.as_mut_slice()).collect();
    c.recover(&shards, &[0, 1], &mut orefs).expect("recoverable");

    let cen = census::read();
    println!("kernels selected: {chosen}");
    println!("scalar_bytes={} accel_bytes={}", cen.scalar_bytes, cen.accel_bytes);
    match cen.accel_percent() {
        Some(pct) => println!("accel share: {pct:.2}%"),
        None => println!("accel share: n/a (no bytes counted)"),
    }
    if chosen != "scalar" && cen.scalar_bytes > 0 {
        eprintln!(
            "DEFECT: {} selected but {} bytes took the scalar path — an unreached kernel is a bug",
            chosen, cen.scalar_bytes
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(verb) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match verb.as_str() {
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        "bench" => match bench(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("rerasure bench: {e}");
                ExitCode::from(2)
            }
        },
        "census" => census_verb(),
        "encode" | "recover" | "verify" => {
            eprintln!(
                "rerasure {verb}: not built yet — lands at M7 with the streaming API \
                 (docs/plans/erasure_mission.md §8)"
            );
            ExitCode::from(3)
        }
        other => {
            eprintln!("rerasure: unknown verb '{other}'\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

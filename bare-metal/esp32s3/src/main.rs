//! rusty_erasure on an ESP32-S3, `no_std + alloc`.
//!
//! Two claims, in this order, because the second is worthless without the
//! first (codec-measurement §0: the gate comes before the number):
//!
//! 1. **It is correct on the part.** Encode, verify, and recover a stripe with
//!    two shards destroyed, comparing the rebuilt bytes against the originals.
//!    A wrong codec's throughput is not a result.
//! 2. **It is this fast on the part.** Source-basis throughput for the encode,
//!    recovery and RAID-6 kernels, with the harness's own floor and the
//!    timer's resolution printed beside the numbers so a reader can audit
//!    them.
//!
//! Xtensa LX7 is 32-bit, so `core::sync::atomic::AtomicU64` does not exist and
//! every reach-census counter in the codec is the `census64` stub. That is the
//! exact configuration the 0.4.1 fix created, which is why this part is the
//! right place to run it: it is the same stub arm Cortex-M4F and RV32 compile.
//! `CENSUS_LIVE` is printed so the log says which build this was.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::time::Instant;
use esp_println::println;

// The image header espflash refuses to flash without.
esp_bootloader_esp_idf::esp_app_desc!();

/// Aim each timed region at this many microseconds. The timer resolves 1 µs,
/// so at 60 ms a single tick is 0.0017% — the quantisation is far below any
/// effect being reported (codec-measurement §2: confirm the effect is larger
/// than the clock's resolution).
const TARGET_US: u64 = 60_000;

/// Best-of-N per cell. The floor is what survives on a part that is also
/// servicing interrupts; a mean would measure the interrupts.
const ROUNDS: u32 = 5;

/// Deterministic filler — splitmix64, so the corpus is identical on every run
/// and on the host. Erasure coding is data-independent (the same GF(2^8)
/// multiply-accumulate runs whatever the bytes are), so content does not steer
/// the timing here; determinism is for the CORRECTNESS gate's reproducibility.
fn fill(buf: &mut [u8], mut seed: u64) {
    for b in buf.iter_mut() {
        seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        *b = (z ^ (z >> 31)) as u8;
    }
}

/// One measured cell: the floor microseconds, and how many reps produced them.
struct Timed {
    us: u64,
    reps: u32,
}

/// Calibrate the rep count, then take the best of [`ROUNDS`] passes.
///
/// The calibration pass is deliberately OUTSIDE the reported number: it also
/// serves as the warm-up, so the first timed round is not paying for cold
/// caches (codec-measurement: discard obvious cold-start samples explicitly
/// rather than averaging them in).
fn bench<F: FnMut()>(mut f: F) -> Timed {
    let t0 = Instant::now();
    f();
    let one = t0.elapsed().as_micros().max(1);

    let reps = ((TARGET_US / one) as u32).clamp(2, 2_000_000);

    let mut best = u64::MAX;
    for _ in 0..ROUNDS {
        let t = Instant::now();
        for _ in 0..reps {
            f();
        }
        let e = t.elapsed().as_micros();
        if e < best {
            best = e;
        }
    }
    Timed { us: best, reps }
}

/// Report a cell on the SOURCE-byte basis — the bytes fed in per rep — which
/// is the basis every number in this repo's LEDGER uses, so the board's
/// figures are comparable with the host's.
fn report(name: &str, t: &Timed, src_per_rep: usize, cpu_hz: u32) {
    let bytes = src_per_rep as u64 * t.reps as u64;
    // 1 byte/µs == 1 MB/s exactly, so this division needs no scale factor.
    let mb_s = bytes as f32 / t.us as f32;
    let cycles_per_byte = (t.us as f64 * cpu_hz as f64) / (bytes as f64 * 1.0e6);
    println!(
        "  {:<28} {:>8.3} MB/s   {:>7.2} cyc/B   ({} reps, {} us, {} KiB src)",
        name,
        mb_s,
        cycles_per_byte,
        t.reps,
        t.us,
        bytes / 1024
    );
}

#[esp_hal::main]
fn main() -> ! {
    // Pin the clock explicitly rather than inheriting a default: the reported
    // cycles/byte is only meaningful against a known frequency.
    let _p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(size: 192 * 1024);

    let cpu_hz = esp_hal::clock::cpu_clock().as_hz();

    println!();
    println!("=== rusty_erasure on ESP32-S3 (xtensa, no_std + alloc, scalar kernels) ===");
    println!(
        "census::CENSUS_LIVE = {}  (false is expected here: no 64-bit atomics)",
        rusty_erasure::census::CENSUS_LIVE
    );
    println!("cpu_clock           = {} MHz", cpu_hz / 1_000_000);

    // ---------------------------------------------------------------- clock
    // Measure the timer's own quantum instead of quoting the datasheet: the
    // smallest non-zero gap between distinct readings IS the resolution, and a
    // harness that cannot state its clock's resolution cannot defend a number.
    let mut quantum = u64::MAX;
    for _ in 0..2000 {
        let a = Instant::now();
        let mut b = Instant::now();
        while b == a {
            b = Instant::now();
        }
        let d = (b - a).as_micros();
        if d > 0 && d < quantum {
            quantum = d;
        }
    }
    println!(
        "timer resolution    = {} us (measured, not quoted)",
        quantum
    );

    // ------------------------------------------------------- correctness gate
    // RS(10,4): ten data shards, four parity, 1 KiB each. Encode, verify, then
    // destroy two data shards and rebuild them, comparing byte for byte.
    const K: usize = 10;
    const P: usize = 4;
    const LEN: usize = 1024;

    let matrix = match rusty_erasure::Matrix::reed_solomon(K, P) {
        Ok(m) => m,
        Err(e) => {
            println!("matrix build FAILED: {e:?}");
            println!("RESULT: FAIL");
            park();
        }
    };
    let cdr = match rusty_erasure::coder(matrix) {
        Ok(c) => c,
        Err(e) => {
            println!("coder build FAILED: {e:?}");
            println!("RESULT: FAIL");
            park();
        }
    };
    println!("kernel set          = {}", cdr.kernels().name);

    let mut data = vec![0u8; K * LEN];
    fill(&mut data, 0x1234_5678_9ABC_DEF0);
    let mut parity = vec![0u8; P * LEN];

    let mut all_ok = true;

    {
        let drefs: Vec<&[u8]> = data.chunks_exact(LEN).collect();
        let mut prefs: Vec<&mut [u8]> = parity.chunks_exact_mut(LEN).collect();
        match cdr.encode(&drefs, &mut prefs) {
            Ok(()) => {}
            Err(e) => {
                all_ok = false;
                println!("encode FAILED: {e:?}");
            }
        }
    }

    {
        let drefs: Vec<&[u8]> = data.chunks_exact(LEN).collect();
        let prefs: Vec<&[u8]> = parity.chunks_exact(LEN).collect();
        match cdr.verify(&drefs, &prefs) {
            Ok(true) => println!("encode+verify       OK   ({K},{P}) x {LEN} B shards"),
            Ok(false) => {
                all_ok = false;
                println!("verify said the parity does NOT match its data");
            }
            Err(e) => {
                all_ok = false;
                println!("verify FAILED: {e:?}");
            }
        }
    }

    // Lose data shards 2 and 7 and rebuild them from the survivors.
    const LOST: [usize; 2] = [2, 7];
    {
        let drefs: Vec<&[u8]> = data.chunks_exact(LEN).collect();
        let prefs: Vec<&[u8]> = parity.chunks_exact(LEN).collect();
        let mut shards: Vec<Option<&[u8]>> = Vec::with_capacity(K + P);
        for (i, s) in drefs.iter().enumerate() {
            shards.push(if LOST.contains(&i) { None } else { Some(*s) });
        }
        for s in prefs.iter() {
            shards.push(Some(*s));
        }

        let mut rebuilt = vec![0u8; LOST.len() * LEN];
        let mut outs: Vec<&mut [u8]> = rebuilt.chunks_exact_mut(LEN).collect();

        match cdr.recover(&shards, &LOST, &mut outs) {
            Ok(()) => {
                drop(outs);
                let mut good = true;
                for (n, &lost) in LOST.iter().enumerate() {
                    let want = &data[lost * LEN..(lost + 1) * LEN];
                    let got = &rebuilt[n * LEN..(n + 1) * LEN];
                    if want != got {
                        good = false;
                        println!("  shard {lost} rebuilt WRONG");
                    }
                }
                all_ok &= good;
                if good {
                    println!(
                        "recover 2 lost      OK   shards {:?} rebuilt byte-identical",
                        LOST
                    );
                }
            }
            Err(e) => {
                all_ok = false;
                println!("recover FAILED: {e:?}");
            }
        }
    }

    if !all_ok {
        println!();
        println!("RESULT: FAIL -- correctness gate did not pass; no timings reported");
        park();
    }

    // ------------------------------------------------------------ throughput
    println!();
    println!("-- throughput (source-byte basis, best of {ROUNDS}, scalar kernels) --");

    // The NULL ARM first: the same loop shape doing no codec work. This is the
    // harness's own floor. If a cell ever lands near it, that cell is
    // measuring the harness, not the kernel.
    {
        let mut sink = 0u32;
        let t = bench(|| {
            sink = sink.wrapping_add(core::hint::black_box(1));
        });
        println!(
            "  {:<28} {:>8} us for {} reps  (harness floor; codec cells must be >> this)",
            "null arm (no codec work)", t.us, t.reps
        );
        core::hint::black_box(sink);
    }

    // Encode RS(4,2) @ 1 KiB
    bench_encode(4, 2, 1024, cpu_hz);
    // Encode RS(10,4) @ 1 KiB -- the LEDGER's S2 shape at a heap-sized shard
    bench_encode(10, 4, 1024, cpu_hz);
    // Encode RS(10,4) @ 4 KiB
    bench_encode(10, 4, 4096, cpu_hz);

    // Recovery: RS(10,4), two lost data shards, using a prepared DecodePlan so
    // the number is pure kernel cost and not repeated matrix inversion.
    {
        let m = rusty_erasure::Matrix::reed_solomon(10, 4).unwrap();
        let c = rusty_erasure::coder(m).unwrap();
        let len = 1024usize;
        let mut d = vec![0u8; 10 * len];
        fill(&mut d, 0xA5A5_5A5A_C3C3_3C3C);
        let mut par = vec![0u8; 4 * len];
        {
            let dr: Vec<&[u8]> = d.chunks_exact(len).collect();
            let mut pr: Vec<&mut [u8]> = par.chunks_exact_mut(len).collect();
            c.encode(&dr, &mut pr).unwrap();
        }
        let dr: Vec<&[u8]> = d.chunks_exact(len).collect();
        let pr: Vec<&[u8]> = par.chunks_exact(len).collect();
        let mut shards: Vec<Option<&[u8]>> = Vec::with_capacity(14);
        for (i, s) in dr.iter().enumerate() {
            shards.push(if LOST.contains(&i) { None } else { Some(*s) });
        }
        for s in pr.iter() {
            shards.push(Some(*s));
        }
        let present: Vec<bool> = shards.iter().map(|s| s.is_some()).collect();
        let plan = c.decode_plan(&present, &LOST).unwrap();

        let mut out = vec![0u8; LOST.len() * len];
        let mut outs: Vec<&mut [u8]> = out.chunks_exact_mut(len).collect();

        // Source basis: recovery reads k survivor shards per rebuilt stripe.
        let src = 10 * len;
        let t = bench(|| {
            c.recover_with(&plan, &shards, &mut outs).unwrap();
            core::hint::black_box(&outs);
        });
        report("recover (10,4) 2 lost 1KiB", &t, src, cpu_hz);
    }

    // RAID-6 P (xor) and P+Q over 10 sources @ 4 KiB.
    {
        let len = 4096usize;
        let n = 10usize;
        let mut src = vec![0u8; n * len];
        fill(&mut src, 0x0F1E_2D3C_4B5A_6978);
        let refs: Vec<&[u8]> = src.chunks_exact(len).collect();

        let mut pbuf = vec![0u8; len];
        let t = bench(|| {
            raid_xor(&refs, &mut pbuf);
        });
        report("raid xor P (10 src, 4KiB)", &t, n * len, cpu_hz);

        let mut p2 = vec![0u8; len];
        let mut q2 = vec![0u8; len];
        let t = bench(|| {
            raid_pq(&refs, &mut p2, &mut q2);
        });
        report("raid pq P+Q (10 src, 4KiB)", &t, n * len, cpu_hz);
    }

    // ---------------------------------------------------------------- census
    let cen = rusty_erasure::census::read();
    println!();
    println!(
        "census: scalar_bytes={} accel_bytes={}  CENSUS_LIVE={}",
        cen.scalar_bytes,
        cen.accel_bytes,
        rusty_erasure::census::CENSUS_LIVE
    );
    println!("        ^ zero because this target has no 64-bit atomics, so every");
    println!("          counter is the census64 stub. That is NOT 'measured zero':");
    println!("          it is 'not measurable on this target'.");

    println!();
    println!("RESULT: PASS -- encoded, verified, recovered and timed on the board");

    park()
}

/// Encode cell: build the coder and buffers ONCE, outside the timed region, so
/// the number is the kernel and not the allocator (codec-measurement: an
/// allocating entry point does not belong in a hot-loop probe).
fn bench_encode(k: usize, p: usize, len: usize, cpu_hz: u32) {
    let m = match rusty_erasure::Matrix::reed_solomon(k, p) {
        Ok(m) => m,
        Err(e) => {
            println!("  encode ({k},{p}) matrix FAILED: {e:?}");
            return;
        }
    };
    let c = match rusty_erasure::coder(m) {
        Ok(c) => c,
        Err(e) => {
            println!("  encode ({k},{p}) coder FAILED: {e:?}");
            return;
        }
    };

    let mut data = vec![0u8; k * len];
    fill(&mut data, 0xDEAD_BEEF_CAFE_F00D);
    let mut par = vec![0u8; p * len];

    let drefs: Vec<&[u8]> = data.chunks_exact(len).collect();
    let mut prefs: Vec<&mut [u8]> = par.chunks_exact_mut(len).collect();

    let t = bench(|| {
        c.encode(&drefs, &mut prefs).unwrap();
        core::hint::black_box(&prefs);
    });

    let mut name = alloc::string::String::new();
    use core::fmt::Write as _;
    let _ = write!(name, "encode ({k},{p}) {}KiB", len / 1024);
    report(&name, &t, k * len, cpu_hz);
}

fn raid_xor(sources: &[&[u8]], parity: &mut [u8]) {
    rusty_erasure::raid::xor_gen(sources, parity).unwrap();
    core::hint::black_box(&parity);
}

fn raid_pq(sources: &[&[u8]], p: &mut [u8], q: &mut [u8]) {
    rusty_erasure::raid::pq_gen(sources, p, q).unwrap();
    core::hint::black_box(&p);
    core::hint::black_box(&q);
}

fn park() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

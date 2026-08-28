//! ISA-L's own erasure-code test suite, ported through the compat layer.
//!
//! Sources (tag v2.32.1): `erasure_code/erasure_code_test.c` (the RS and
//! Cauchy fixed phases, the random phase, the buffer-end sweep, and the
//! `gf_gen_decode_matrix` construction with its singular-retry loop — ported
//! line-for-line), plus the core contracts of `erasure_code_update_test.c`,
//! `gf_vect_mul_test.c`, and `gf_inverse_test.c`.
//!
//! Documented deviations, none semantic: the C `rand()` stream is replaced by
//! a seeded splitmix64, and the random phase runs 64 x 2048-byte cases rather
//! than 200 x 8192 to keep debug-mode test time sane (the fixed phases keep
//! ISA-L's full 8192). Under Miri everything shrinks further.

use rusty_erasure::isal::*;

const MMAX: usize = 127;
const TEST_LEN: usize = if cfg!(miri) { 128 } else { 8192 };
const RANDOMS: usize = if cfg!(miri) { 2 } else { 64 };
const RAND_LEN: usize = if cfg!(miri) { 64 } else { 2048 };
const RAND_MMAX: usize = if cfg!(miri) { 16 } else { MMAX };

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn fill(&mut self, buf: &mut [u8]) {
        for b in buf {
            *b = self.next() as u8;
        }
    }
}

/// Port of the C test's `gen_err_list`: random erasures over the whole stripe,
/// at most `m - k`, at least one; source errors always precede parity errors.
// Ported verbatim from ISA-L's `erasure_code_test.c`: the index-walk shape is
// kept so this stays diffable against the reference it gates us on.
#[allow(clippy::needless_range_loop)]
fn gen_err_list(rng: &mut Rng, k: usize, m: usize) -> (Vec<usize>, Vec<bool>, usize) {
    let mut err_list = Vec::new();
    let mut in_err = vec![false; m];
    let mut nsrcerrs = 0;
    for i in 0..m {
        if err_list.len() >= m - k {
            break;
        }
        if rng.next() & 1 != 0 {
            in_err[i] = true;
            err_list.push(i);
            if i < k {
                nsrcerrs += 1;
            }
        }
    }
    if err_list.is_empty() {
        let err = rng.below(m);
        err_list.push(err);
        in_err[err] = true;
        if err < k {
            nsrcerrs = 1;
        }
    }
    (err_list, in_err, nsrcerrs)
}

/// Port of the C test's `gf_gen_decode_matrix`, including the singular-retry
/// loop that substitutes the last survivor row with further parity rows (only
/// reachable for RS matrices — Cauchy submatrices always invert).
#[allow(clippy::needless_range_loop)] // see gen_err_list: ported index shape
fn gen_decode_matrix(
    encode_matrix: &[u8],
    err_list: &[usize],
    in_err: &[bool],
    nsrcerrs: usize,
    k: usize,
    m: usize,
) -> Option<(Vec<u8>, Vec<usize>)> {
    let nerrs = err_list.len();
    let mut b = vec![0u8; k * k];
    let mut backup = vec![0u8; k * k];
    let mut decode_index = vec![0usize; k];

    // Construct matrix b by removing error rows.
    let mut r = 0usize;
    for i in 0..k {
        while in_err[r] {
            r += 1;
        }
        for j in 0..k {
            b[k * i + j] = encode_matrix[k * r + j];
            backup[k * i + j] = encode_matrix[k * r + j];
        }
        decode_index[i] = r;
        r += 1;
    }

    let mut invert_matrix = vec![0u8; k * k];
    let mut incr = 0usize;
    while gf_invert_matrix(&mut b, &mut invert_matrix, k).is_err() {
        if nerrs == m - k {
            return None; // BAD MATRIX
        }
        incr += 1;
        b.copy_from_slice(&backup);
        for i in nsrcerrs..nerrs.saturating_sub(nsrcerrs) {
            if err_list[i] == decode_index[k - 1] + incr {
                // skip the erased parity line
                incr += 1;
                continue;
            }
        }
        if decode_index[k - 1] + incr >= m {
            return None; // BAD MATRIX
        }
        decode_index[k - 1] += incr;
        for j in 0..k {
            b[k * (k - 1) + j] = encode_matrix[k * decode_index[k - 1] + j];
        }
    }

    let mut decode_matrix = vec![0u8; k * (nerrs.max(1))];
    for i in 0..nsrcerrs {
        for j in 0..k {
            decode_matrix[k * i + j] = invert_matrix[k * err_list[i] + j];
        }
    }
    // err rows from encode_matrix * invert of b, for parity decoding.
    for p in nsrcerrs..nerrs {
        for i in 0..k {
            let mut s = 0u8;
            for j in 0..k {
                s ^= gf_mul(invert_matrix[j * k + i], encode_matrix[k * err_list[p] + j]);
            }
            decode_matrix[k * p + i] = s;
        }
    }
    Some((decode_matrix, decode_index))
}

/// One full phase of erasure_code_test.c: encode, erase, build the decode
/// matrix manually through compat, recover, compare against the originals.
fn encode_recover_phase(rng: &mut Rng, cauchy: bool, k: usize, m: usize, len: usize) {
    let tag = format!(
        "kind={} k={k} m={m} len={len}",
        if cauchy { "cauchy" } else { "rs" }
    );
    let mut encode_matrix = vec![0u8; m * k];
    if cauchy {
        gf_gen_cauchy1_matrix(&mut encode_matrix, m, k).expect("gen");
    } else {
        gf_gen_rs_matrix(&mut encode_matrix, m, k).expect("gen");
    }

    // Sources 0..k random; parity k..m written by encode.
    let mut stripe: Vec<Vec<u8>> = (0..m).map(|_| vec![0u8; len]).collect();
    for buf in stripe.iter_mut().take(k) {
        rng.fill(buf);
    }

    let mut g_tbls = vec![0u8; (m - k) * k * 32];
    ec_init_tables(k, m - k, &encode_matrix[k * k..], &mut g_tbls).expect("init");
    {
        let (data, coding) = stripe.split_at_mut(k);
        let data_refs: Vec<&[u8]> = data.iter().map(|b| b.as_slice()).collect();
        let mut coding_refs: Vec<&mut [u8]> = coding.iter_mut().map(|b| b.as_mut_slice()).collect();
        ec_encode_data(len, k, m - k, &g_tbls, &data_refs, &mut coding_refs).expect("encode");
    }

    let (err_list, in_err, nsrcerrs) = gen_err_list(rng, k, m);
    let nerrs = err_list.len();
    let (decode_matrix, decode_index) =
        gen_decode_matrix(&encode_matrix, &err_list, &in_err, nsrcerrs, k, m)
            .unwrap_or_else(|| panic!("{tag}: BAD MATRIX errs={err_list:?}"));

    // Pack recovery array as the list of valid sources, decode-index order.
    let recov: Vec<&[u8]> = decode_index.iter().map(|&r| stripe[r].as_slice()).collect();

    let mut g2 = vec![0u8; nerrs * k * 32];
    ec_init_tables(k, nerrs, &decode_matrix, &mut g2).expect("init decode");
    let mut temp: Vec<Vec<u8>> = (0..nerrs).map(|_| vec![0u8; len]).collect();
    {
        let mut temp_refs: Vec<&mut [u8]> = temp.iter_mut().map(|b| b.as_mut_slice()).collect();
        ec_encode_data(len, k, nerrs, &g2, &recov, &mut temp_refs).expect("recover");
    }
    for (i, &e) in err_list.iter().enumerate() {
        assert_eq!(
            temp[i], stripe[e],
            "{tag}: recovery of shard {e}, errs={err_list:?}"
        );
    }
}

#[test]
fn rs_matrix_first_test_9x5() {
    let mut rng = Rng(11); // TEST_SEED
    encode_recover_phase(&mut rng, false, 5, 9, TEST_LEN);
}

#[test]
fn cauchy_matrix_first_test_9x5() {
    let mut rng = Rng(12);
    encode_recover_phase(&mut rng, true, 5, 9, TEST_LEN);
}

#[test]
fn random_cauchy_configs() {
    let mut rng = Rng(13);
    for _ in 0..RANDOMS {
        let m = loop {
            let m = rng.below(RAND_MMAX);
            if m >= 2 {
                break m;
            }
        };
        let k = loop {
            let k = rng.below(RAND_MMAX);
            if k >= 1 && k < m {
                break k;
            }
        };
        encode_recover_phase(&mut rng, true, k, m, RAND_LEN);
    }
}

#[test]
fn buffer_end_size_sweep() {
    // The efence-shaped tail of erasure_code_test.c: k=16, rows 1..=16,
    // small and odd sizes (the memory-safety half of their test is what ASan
    // and Miri cover for us; the size/tail coverage is ported here).
    let mut rng = Rng(14);
    let k = 16;
    let rows_max = if cfg!(miri) { 2 } else { 16 };
    for rows in 1..=rows_max {
        for &size in &[16usize, 29, 61, 128, 272] {
            encode_recover_phase(&mut rng, true, k, k + rows, size);
        }
    }
}

#[test]
fn update_sequence_equals_one_shot_through_compat() {
    // The core contract of erasure_code_update_test.c.
    let mut rng = Rng(15);
    for &(k, p, len) in &[(5usize, 4usize, 1027usize), (16, 4, 96), (10, 4, 33)] {
        let m = k + p;
        let mut enc = vec![0u8; m * k];
        gf_gen_cauchy1_matrix(&mut enc, m, k).expect("gen");
        let mut g = vec![0u8; p * k * 32];
        ec_init_tables(k, p, &enc[k * k..], &mut g).expect("init");

        let data: Vec<Vec<u8>> = (0..k)
            .map(|_| {
                let mut b = vec![0u8; len];
                rng.fill(&mut b);
                b
            })
            .collect();
        let data_refs: Vec<&[u8]> = data.iter().map(|b| b.as_slice()).collect();

        let mut one_shot = vec![vec![0u8; len]; p];
        {
            let mut refs: Vec<&mut [u8]> = one_shot.iter_mut().map(|b| b.as_mut_slice()).collect();
            ec_encode_data(len, k, p, &g, &data_refs, &mut refs).expect("encode");
        }

        let mut updated = vec![vec![0u8; len]; p];
        {
            let mut refs: Vec<&mut [u8]> = updated.iter_mut().map(|b| b.as_mut_slice()).collect();
            for (j, d) in data.iter().enumerate() {
                ec_encode_data_update(len, k, p, j, &g, d, &mut refs).expect("update");
            }
        }
        assert_eq!(updated, one_shot, "k={k} p={p} len={len}");
    }
}

#[test]
fn vect_mul_contract() {
    // gf_vect_mul_test.c: correct for 32-multiple lengths, byte-compared
    // against gf_mul; non-multiple lengths are refused (ISA-L returns error).
    let mut rng = Rng(16);
    let len = if cfg!(miri) { 64 } else { 8192 };
    let mut src = vec![0u8; len];
    rng.fill(&mut src);
    let mut dest = vec![0u8; len];
    for &c in &[0u8, 1, 2, 0x1d, 0x8f, 0xff] {
        let mut tbl = vec![0u8; 32];
        ec_init_tables(1, 1, &[c], &mut tbl).expect("one coeff");
        gf_vect_mul(len, &tbl, &src, &mut dest).expect("aligned len");
        for (i, (&s, &d)) in src.iter().zip(&dest).enumerate() {
            assert_eq!(d, gf_mul(c, s), "c={c} i={i}");
        }
        assert!(
            gf_vect_mul(len - 13, &tbl, &src, &mut dest).is_err(),
            "unaligned len refused"
        );
    }
}

#[test]
fn inverse_random_matrices() {
    // gf_inverse_test.c shape: random matrices through compat
    // gf_invert_matrix (input destroyed), product must be the identity.
    let mut rng = Rng(17);
    let rounds = if cfg!(miri) { 4 } else { 100 };
    let mut inverted = 0;
    for _ in 0..rounds {
        let n = 1 + rng.below(24);
        let mut a = vec![0u8; n * n];
        rng.fill(&mut a);
        let orig = a.clone();
        let mut inv = vec![0u8; n * n];
        if gf_invert_matrix(&mut a, &mut inv, n).is_ok() {
            inverted += 1;
            // orig * inv == I
            for i in 0..n {
                for j in 0..n {
                    let mut s = 0u8;
                    for t in 0..n {
                        s ^= gf_mul(orig[n * i + t], inv[n * t + j]);
                    }
                    assert_eq!(s, u8::from(i == j), "n={n} ({i},{j})");
                }
            }
        }
    }
    assert!(
        inverted > rounds / 2,
        "only {inverted}/{rounds} inverted — probe broken?"
    );
}

//! The `Coder`: encode, incremental update, verify, and recover — ISA-L's
//! `ec_encode_data` / `ec_encode_data_update` semantics plus the recovery flow
//! ISA-L leaves as an exercise, all behind validated, panic-free APIs.
//!
//! Conformance: `encode` and `update` are byte-identical to ISA-L's `_base`
//! implementations (golden-vector gated); a completed `update` sequence is
//! byte-identical to one-shot `encode`; `recover` is gated against ground
//! truth (the original shards ARE the expected output).

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{CodeError, MatrixError, RecoverError};
use crate::kernel::Kernels;
use crate::matrix::Matrix;

/// A prepared decode: the survivor selection, inverted submatrix (composed
/// into per-target coefficient rows), and expanded tables for ONE loss
/// pattern — reusable across every stripe sharing that pattern. Built by
/// [`Coder::decode_plan`], consumed by [`Coder::recover_with`].
#[derive(Debug, Clone)]
pub struct DecodePlan {
    gftbls: Vec<u8>,
    survivors: Vec<usize>,
    rebuild: Vec<usize>,
    n: usize,
}

/// An erasure coder for one `(matrix)` configuration: `k` source shards in,
/// `p` parity shards out, recovery from any `k` survivors.
///
/// Construction expands the parity coefficients into ISA-L-layout tables once;
/// encode/update/recover then run with zero allocations on the data path
/// (recover allocates only its small decode-matrix scratch).
#[derive(Debug, Clone)]
pub struct Coder {
    matrix: Matrix,
    /// Expanded tables for the parity rows: `p * k * 32` bytes, row-major —
    /// table for (parity row `l`, source `j`) at `(l*k + j) * 32`.
    gftbls: Vec<u8>,
    /// The kernel set chosen ONCE at construction (dispatch at the surface,
    /// never in the loop).
    kernels: Kernels,
}

impl Coder {
    /// Build a coder from an encode matrix (`rows = k + p`, `cols = k`, top
    /// block identity — what [`Matrix::reed_solomon`] / [`Matrix::cauchy`]
    /// produce). A matrix with no parity rows is a dimension error.
    ///
    /// This constructor uses the **scalar** kernel set — core carries no
    /// detection machinery. The `rusty_erasure` facade's `coder()` picks the
    /// best SIMD set for the running CPU via [`Coder::with_kernels`]; prefer
    /// it in applications.
    pub fn new(matrix: Matrix) -> Result<Self, MatrixError> {
        Self::with_kernels(matrix, Kernels::scalar())
    }

    /// Build a coder driving an explicit kernel set (see [`Kernels`]).
    pub fn with_kernels(matrix: Matrix, kernels: Kernels) -> Result<Self, MatrixError> {
        if matrix.rows() <= matrix.cols() {
            return Err(MatrixError::Dimensions {
                k: matrix.cols(),
                p: matrix.rows().saturating_sub(matrix.cols()),
            });
        }
        let gftbls = (kernels.init)(matrix.parity_bytes());
        Ok(Self { matrix, gftbls, kernels })
    }

    /// The kernel set this coder drives (name is useful for reporting).
    pub fn kernels(&self) -> &Kernels {
        &self.kernels
    }

    /// Source-shard count.
    pub fn k(&self) -> usize {
        self.matrix.cols()
    }

    /// Parity-shard count.
    pub fn p(&self) -> usize {
        self.matrix.rows() - self.matrix.cols()
    }

    /// The encode matrix this coder was built from.
    pub fn matrix(&self) -> &Matrix {
        &self.matrix
    }

    /// The expanded parity tables, in THIS coder's kernel-set format
    /// (`kernels().table_bytes` per coefficient — ISA-L nibble layout for the
    /// scalar/PSHUFB sets, affine matrices for GFNI). Exposed for the compat
    /// layer and the conformance tests.
    pub fn gftbls(&self) -> &[u8] {
        &self.gftbls
    }

    fn check_data(&self, data: &[&[u8]], len: usize) -> Result<(), CodeError> {
        if data.len() != self.k() {
            return Err(CodeError::ShardCount { expected: self.k(), got: data.len() });
        }
        for (index, d) in data.iter().enumerate() {
            if d.len() != len {
                return Err(CodeError::ShardLength { index, expected: len, got: d.len() });
            }
        }
        Ok(())
    }

    /// Encode: `k` equal-length source shards in, `p` parity shards out
    /// (overwritten). Byte-identical to ISA-L `ec_encode_data`.
    pub fn encode(&self, data: &[&[u8]], parity: &mut [&mut [u8]]) -> Result<(), CodeError> {
        if parity.len() != self.p() {
            return Err(CodeError::ShardCount { expected: self.p(), got: parity.len() });
        }
        let len = parity.first().map_or(0, |b| b.len());
        for (index, b) in parity.iter().enumerate() {
            if b.len() != len {
                return Err(CodeError::ShardLength {
                    index: self.k() + index,
                    expected: len,
                    got: b.len(),
                });
            }
        }
        self.check_data(data, len)?;
        (self.kernels.encode)(&self.gftbls, data, parity);
        Ok(())
    }

    /// Incremental encode: fold ONE source shard (index `shard_index`) into
    /// all parity shards. Starting from zeroed parity buffers and calling this
    /// once per source (any order) yields byte-identical output to
    /// [`Coder::encode`] — ISA-L `ec_encode_data_update` semantics.
    pub fn update(
        &self,
        shard_index: usize,
        data: &[u8],
        parity: &mut [&mut [u8]],
    ) -> Result<(), CodeError> {
        let k = self.k();
        if shard_index >= k {
            return Err(CodeError::ShardIndex { index: shard_index, k });
        }
        if parity.len() != self.p() {
            return Err(CodeError::ShardCount { expected: self.p(), got: parity.len() });
        }
        for (index, b) in parity.iter().enumerate() {
            if b.len() != data.len() {
                return Err(CodeError::ShardLength {
                    index: k + index,
                    expected: data.len(),
                    got: b.len(),
                });
            }
        }
        (self.kernels.update)(&self.gftbls, k, shard_index, data, parity);
        Ok(())
    }

    /// Check that `parity` is consistent with `data`. `Ok(true)` means every
    /// parity shard matches a fresh encode.
    pub fn verify(&self, data: &[&[u8]], parity: &[&[u8]]) -> Result<bool, CodeError> {
        if parity.len() != self.p() {
            return Err(CodeError::ShardCount { expected: self.p(), got: parity.len() });
        }
        let len = parity.first().map_or(0, |b| b.len());
        for (index, b) in parity.iter().enumerate() {
            if b.len() != len {
                return Err(CodeError::ShardLength {
                    index: self.k() + index,
                    expected: len,
                    got: b.len(),
                });
            }
        }
        self.check_data(data, len)?;
        // One fused kernel call re-encodes every parity row in a single walk
        // of the sources (brick: data was read p times, once per row — the
        // census counter shows k*len counted bytes per verify instead of
        // p*k*len). Memory cost: a p*len scratch instead of len.
        let p = self.p();
        if len == 0 {
            return Ok(true);
        }
        let mut scratch = vec![0u8; p * len];
        {
            let mut rows: Vec<&mut [u8]> = scratch.chunks_mut(len).collect();
            (self.kernels.encode)(&self.gftbls, data, &mut rows);
        }
        for (l, expect) in parity.iter().enumerate() {
            if &scratch[l * len..(l + 1) * len] != *expect {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Prepare a reusable decode plan for one loss pattern: which shards are
    /// present (`present[i]`), and which indices to rebuild. The expensive,
    /// data-independent work — survivor selection, submatrix inversion,
    /// coefficient composition, table expansion — happens ONCE here;
    /// [`Coder::recover_with`] then rebuilds any number of stripes with that
    /// pattern at pure kernel cost (repair jobs and steady-state degraded
    /// reads reuse one plan across every stripe).
    pub fn decode_plan(
        &self,
        present: &[bool],
        rebuild: &[usize],
    ) -> Result<DecodePlan, RecoverError> {
        let k = self.k();
        let n = self.matrix.rows();
        if present.len() != n {
            return Err(CodeError::ShardCount { expected: n, got: present.len() }.into());
        }
        for &x in rebuild {
            if x >= n {
                return Err(CodeError::ShardIndex { index: x, k: n }.into());
            }
        }
        let mut survivors: Vec<usize> = Vec::with_capacity(k);
        let mut have = 0usize;
        for (i, &ok) in present.iter().enumerate() {
            if ok {
                have += 1;
                if survivors.len() < k {
                    survivors.push(i);
                }
            }
        }
        if survivors.len() < k {
            return Err(RecoverError::TooManyMissing { missing: n - have, p: self.p() });
        }
        let b = self.matrix.select_rows(&survivors)?;
        let d = b.invert()?;
        let mut coeffs = vec![0u8; rebuild.len() * k];
        for (r, &x) in rebuild.iter().enumerate() {
            let row = &mut coeffs[r * k..(r + 1) * k];
            if x < k {
                for (t, c) in row.iter_mut().enumerate() {
                    *c = d.get(x, t).expect("in range");
                }
            } else {
                for (t, c) in row.iter_mut().enumerate() {
                    let mut s = 0u8;
                    for j in 0..k {
                        s ^= crate::gf::mul(
                            self.matrix.get(x, j).expect("in range"),
                            d.get(j, t).expect("in range"),
                        );
                    }
                    *c = s;
                }
            }
        }
        Ok(DecodePlan {
            gftbls: (self.kernels.init)(&coeffs),
            survivors,
            rebuild: rebuild.to_vec(),
            n,
        })
    }

    /// Rebuild one stripe with a prepared [`DecodePlan`] — pure kernel cost,
    /// no matrix work, no table expansion, one small scratch collection.
    pub fn recover_with(
        &self,
        plan: &DecodePlan,
        shards: &[Option<&[u8]>],
        out: &mut [&mut [u8]],
    ) -> Result<(), RecoverError> {
        if shards.len() != plan.n {
            return Err(CodeError::ShardCount { expected: plan.n, got: shards.len() }.into());
        }
        if out.len() != plan.rebuild.len() {
            return Err(
                CodeError::ShardCount { expected: plan.rebuild.len(), got: out.len() }.into()
            );
        }
        let mut src: Vec<&[u8]> = Vec::with_capacity(plan.survivors.len());
        let len = out.first().map_or(0, |b| b.len());
        for &i in &plan.survivors {
            let s = shards[i].ok_or(RecoverError::TooManyMissing {
                missing: 1,
                p: self.p(),
            })?;
            if s.len() != len {
                return Err(CodeError::ShardLength { index: i, expected: len, got: s.len() }.into());
            }
            src.push(s);
        }
        for b in out.iter() {
            if b.len() != len {
                return Err(
                    CodeError::ShardLength { index: 0, expected: len, got: b.len() }.into()
                );
            }
        }
        (self.kernels.encode)(&plan.gftbls, &src, out);
        Ok(())
    }

    /// Rebuild shards from survivors.
    ///
    /// `shards` is the full stripe in index order — `k` sources then `p`
    /// parity — with `None` for anything lost. `rebuild` names the shard
    /// indices to reconstruct (source or parity), and `out` supplies one
    /// equal-length buffer per rebuild target. Any `k` present shards suffice;
    /// fewer is [`RecoverError::TooManyMissing`].
    ///
    /// For many stripes with one loss pattern, build a [`Coder::decode_plan`]
    /// once and use [`Coder::recover_with`] — this one-shot form re-derives
    /// the decode matrix every call.
    pub fn recover(
        &self,
        shards: &[Option<&[u8]>],
        rebuild: &[usize],
        out: &mut [&mut [u8]],
    ) -> Result<(), RecoverError> {
        let k = self.k();
        let n = self.matrix.rows();
        if shards.len() != n {
            return Err(CodeError::ShardCount { expected: n, got: shards.len() }.into());
        }
        if rebuild.len() != out.len() {
            return Err(CodeError::ShardCount { expected: rebuild.len(), got: out.len() }.into());
        }
        for &x in rebuild {
            if x >= n {
                return Err(CodeError::ShardIndex { index: x, k: n }.into());
            }
        }

        // Survivors: the first k present shards, in index order.
        let mut survivors: Vec<usize> = Vec::with_capacity(k);
        let mut present = 0usize;
        for (i, s) in shards.iter().enumerate() {
            if s.is_some() {
                present += 1;
                if survivors.len() < k {
                    survivors.push(i);
                }
            }
        }
        if survivors.len() < k {
            return Err(RecoverError::TooManyMissing { missing: n - present, p: self.p() });
        }

        // Shard length agreement across every present shard and output buffer.
        let len = shards[survivors[0]].expect("survivor is present").len();
        for (index, s) in shards.iter().enumerate() {
            if let Some(s) = s {
                if s.len() != len {
                    return Err(
                        CodeError::ShardLength { index, expected: len, got: s.len() }.into()
                    );
                }
            }
        }
        for (i, b) in out.iter().enumerate() {
            if b.len() != len {
                return Err(
                    CodeError::ShardLength { index: rebuild[i], expected: len, got: b.len() }
                        .into(),
                );
            }
        }

        // Decode matrix: invert the survivors' rows. d maps survivor shard
        // values back to the original sources.
        let b = self.matrix.select_rows(&survivors)?;
        let d = b.invert()?;

        let src: Vec<&[u8]> = survivors
            .iter()
            .map(|&i| shards[i].expect("survivor is present"))
            .collect();

        // One decode-coefficient row per rebuild target, expanded together so
        // a SINGLE kernels.encode call (with its row fusion) rebuilds all of
        // them in one walk over the survivors.
        let mut coeffs = vec![0u8; rebuild.len() * k];
        for (r, &x) in rebuild.iter().enumerate() {
            let row = &mut coeffs[r * k..(r + 1) * k];
            if x < k {
                // Missing source: row x of the inverse maps survivors -> source x.
                for (t, c) in row.iter_mut().enumerate() {
                    *c = d.get(x, t).expect("in range");
                }
            } else {
                // Missing parity: compose the parity row with the inverse so a
                // single pass over the survivors rebuilds it directly.
                for (t, c) in row.iter_mut().enumerate() {
                    let mut s = 0u8;
                    for j in 0..k {
                        s ^= crate::gf::mul(
                            self.matrix.get(x, j).expect("in range"),
                            d.get(j, t).expect("in range"),
                        );
                    }
                    *c = s;
                }
            }
        }
        let gftbls = (self.kernels.init)(&coeffs);
        (self.kernels.encode)(&gftbls, &src, out);
        Ok(())
    }
}

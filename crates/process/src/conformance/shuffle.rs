// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Polynomial-time order-preserving shuffle membership.
//!
//! Provenance: the membership semantics are ex4pm `Shuffle.op_shuffle/2`'s
//! (a trace is accepted when it interleaves the child sequences while
//! respecting the poset); the readiness gate is bcinr `scheduler_v2.rs`'s
//! `pred_mask & !done_mask == 0`, evaluated over u64 bitset words with
//! `purrdf-branchless`'s slice kernels.

use purrdf_branchless::bitset::any_bit_set_u64_slice;

use crate::error::ProcessError;

/// The partial order to shuffle-check against: each child is a label
/// sequence; `precedence` is the poset over child indices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShuffleSpec {
    /// Per-child label sequences.
    pub child_sequences: Vec<Vec<String>>,
    /// Precedence edges over child indices.
    pub precedence: Vec<(usize, usize)>,
}

/// One membership verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShuffleVerdict {
    /// The trace is an order-preserving interleaving.
    Accepted,
    /// The trace is not.
    Rejected {
        /// The first failing position.
        position: usize,
        /// The label at that position.
        label: String,
        /// Why it failed: no eligible child carries this label.
        reason: &'static str,
    },
}

impl ShuffleSpec {
    /// Validate indices and build.
    ///
    /// # Errors
    /// The order-pair refusals ([`ProcessError::OrderNotIrreflexive`] and
    /// siblings).
    pub fn new(
        child_sequences: Vec<Vec<String>>,
        precedence: Vec<(usize, usize)>,
    ) -> Result<Self, ProcessError> {
        crate::model::powl::validate_order(child_sequences.len(), &precedence)?;
        Ok(Self {
            child_sequences,
            precedence,
        })
    }

    /// Whether `trace` is an order-preserving interleaving of the child
    /// sequences. Ambiguous positions (several eligible children carry the
    /// same label) trigger deterministic backtracking, counted against
    /// `budget`.
    ///
    /// # Errors
    /// [`ProcessError::AmbiguityBudgetExceeded`].
    pub fn accepts(&self, trace: &[&str], budget: u64) -> Result<ShuffleVerdict, ProcessError> {
        let children = self.child_sequences.len();
        let words = words_for(children);
        let pred_masks = self.predecessor_masks(words);
        let mut done = vec![0u64; words];
        let mut cursors = vec![0usize; children];
        let mut nodes = 0u64;
        let verdict = self.search(
            0,
            trace,
            &mut cursors,
            &mut done,
            &pred_masks,
            &mut nodes,
            budget,
        );
        verdict
    }

    /// DFS over trace positions with deterministic backtracking.
    #[allow(clippy::too_many_arguments)]
    fn search(
        &self,
        position: usize,
        trace: &[&str],
        cursors: &mut [usize],
        done: &mut [u64],
        pred_masks: &[Vec<u64>],
        nodes: &mut u64,
        budget: u64,
    ) -> Result<ShuffleVerdict, ProcessError> {
        if position == trace.len() {
            return Ok(ShuffleVerdict::Accepted);
        }
        *nodes += 1;
        if *nodes > budget {
            return Err(ProcessError::AmbiguityBudgetExceeded { budget });
        }
        let label = trace[position];
        let mut candidates: Vec<usize> = Vec::new();
        for child in 0..self.child_sequences.len() {
            let cursor = cursors[child];
            if cursor >= self.child_sequences[child].len() {
                continue;
            }
            if !self.child_sequences[child][cursor].eq(label) {
                continue;
            }
            if ready(&pred_masks[child], done) {
                candidates.push(child);
            }
        }
        for child in candidates {
            cursors[child] += 1;
            if cursors[child] == self.child_sequences[child].len() {
                set_bit(done, child);
            }
            let verdict = self.search(
                position + 1,
                trace,
                cursors,
                done,
                pred_masks,
                nodes,
                budget,
            )?;
            match verdict {
                ShuffleVerdict::Accepted => return Ok(ShuffleVerdict::Accepted),
                ShuffleVerdict::Rejected { .. } => {}
            }
            if cursors[child] == self.child_sequences[child].len() {
                clear_bit(done, child);
            }
            cursors[child] -= 1;
        }
        Ok(ShuffleVerdict::Rejected {
            position,
            label: label.to_owned(),
            reason: "no eligible child carries this label",
        })
    }

    /// Per-child predecessor bit masks.
    fn predecessor_masks(&self, words: usize) -> Vec<Vec<u64>> {
        let children = self.child_sequences.len();
        let mut masks = vec![vec![0u64; words]; children];
        for (from, to) in &self.precedence {
            masks[*to][*from / 64] |= 1u64 << (*from % 64);
        }
        masks
    }
}

/// Whether every predecessor of one child is done
/// (`pred & !done == 0`, bcinr's readiness gate).
fn ready(pred: &[u64], done: &[u64]) -> bool {
    let blocked: Vec<u64> = pred.iter().zip(done.iter()).map(|(p, d)| p & !d).collect();
    !any_bit_set_u64_slice(&blocked)
}

/// Set child `index` in the done-word vector.
fn set_bit(done: &mut [u64], index: usize) {
    done[index / 64] |= 1u64 << (index % 64);
}

/// Clear child `index` in the done-word vector.
fn clear_bit(done: &mut [u64], index: usize) {
    done[index / 64] &= !(1u64 << (index % 64));
}

/// Word count for `children` bits.
#[must_use]
pub fn words_for(children: usize) -> usize {
    children.div_ceil(64)
}

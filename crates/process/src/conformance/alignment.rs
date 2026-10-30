// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Deterministic A\* alignment (h = 0 Dijkstra over integer costs).
//!
//! Provenance: state space, move taxonomy, and cost defaults follow wasm4pm
//! `alignments.rs` (invisible transitions are free model moves, the closed
//! set is keyed on `(trace_index, marking)`, the first goal pop is optimal);
//! the fitness derivation follows ex4pm `alignment.ex`
//! (`1 - cost / (trace_len + max(1, transitions * 0.5))`). Expansion order
//! is total: cost, then insertion sequence — no hash-order dependence.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use purrdf_hash::fixed::FixedState;
use std::collections::HashSet;

use crate::error::ProcessError;
use crate::model::petri::PetriNet;

/// Alignment move costs (u64; 0 costs are legal, e.g. invisible moves).
#[derive(Clone, Copy, Debug)]
pub struct AlignmentCosts {
    /// Cost of a synchronous move.
    pub sync: u64,
    /// Cost of a log move.
    pub log_move: u64,
    /// Cost of a model move on a visible transition.
    pub model_move: u64,
}

impl Default for AlignmentCosts {
    fn default() -> Self {
        Self {
            sync: 0,
            log_move: 1,
            model_move: 1,
        }
    }
}

/// One alignment move.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Move {
    /// Trace event and transition agree.
    Sync(String),
    /// The trace event has no model counterpart.
    Log(String),
    /// The model fires without consuming the trace (label `None` =
    /// invisible).
    Model(Option<String>),
}

/// One optimal alignment.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignmentResult {
    /// Total cost.
    pub cost: u64,
    /// The move sequence.
    pub moves: Vec<Move>,
    /// Synchronous-move count.
    pub syncs: u64,
    /// Log-move count.
    pub log_moves: u64,
    /// Model-move count.
    pub model_moves: u64,
    /// ex4pm's derived fitness, clamped to `[0, 1]`.
    pub fitness: f64,
}

/// Compute an optimal alignment of one trace.
///
/// # Errors
/// [`ProcessError::AlignmentBudgetExceeded`] past `max_iterations`, or
/// [`ProcessError::NoAlignment`] when the open set drains.
pub fn align(
    net: &PetriNet,
    activities: &[&str],
    costs: &AlignmentCosts,
    max_iterations: u64,
) -> Result<AlignmentResult, ProcessError> {
    let initial: Vec<u32> = net.initial().to_vec();
    let mut open: BinaryHeap<Reverse<(u64, u64, usize, Vec<u32>, Vec<Move>)>> = BinaryHeap::new();
    let mut sequence = 0u64;
    open.push(Reverse((0, sequence, 0, initial.clone(), Vec::new())));
    let mut closed: HashSet<(usize, Vec<u32>), FixedState> =
        HashSet::with_hasher(FixedState::new());
    let mut iterations = 0u64;
    let goal = |marking: &[u32]| -> bool {
        if net.final_markings().is_empty() {
            return true;
        }
        net.final_markings()
            .iter()
            .any(|final_marking| final_marking == marking)
    };
    let mut best: Option<(u64, Vec<Move>, u64, u64, u64)> = None;
    while let Some(Reverse((cost, _seq, index, marking, moves))) = open.pop() {
        iterations += 1;
        if iterations > max_iterations {
            return Err(ProcessError::AlignmentBudgetExceeded {
                iterations: max_iterations,
            });
        }
        if closed.contains(&(index, marking.clone())) {
            continue;
        }
        closed.insert((index, marking.clone()));
        if index == activities.len() && goal(&marking) {
            let syncs = moves.iter().filter(|m| matches!(m, Move::Sync(_))).count() as u64;
            let log_moves = moves.iter().filter(|m| matches!(m, Move::Log(_))).count() as u64;
            let model_moves = moves.iter().filter(|m| matches!(m, Move::Model(_))).count() as u64;
            best = Some((cost, moves, syncs, log_moves, model_moves));
            break;
        }
        // Log move.
        if index < activities.len() {
            let label = activities[index];
            let next_marking = marking.clone();
            let mut next_moves = moves.clone();
            next_moves.push(Move::Log(label.to_owned()));
            open.push(Reverse((
                cost + costs.log_move,
                sequence,
                index + 1,
                next_marking,
                next_moves,
            )));
            sequence += 1;
            // Sync moves: every enabled transition carrying the label.
            for (t_index, t) in net.transitions().iter().enumerate() {
                if t.label.as_deref() != Some(label) {
                    continue;
                }
                let mut fired = marking.clone();
                if !net.fire(&mut fired, t_index) {
                    continue;
                }
                let mut sync_moves = moves.clone();
                sync_moves.push(Move::Sync(label.to_owned()));
                open.push(Reverse((
                    cost + costs.sync,
                    sequence,
                    index + 1,
                    fired,
                    sync_moves,
                )));
                sequence += 1;
            }
        }
        // Model moves: every enabled transition (invisible ones are free).
        for (t_index, t) in net.transitions().iter().enumerate() {
            let mut fired = marking.clone();
            if !net.fire(&mut fired, t_index) {
                continue;
            }
            let move_cost = if t.label.is_none() {
                0
            } else {
                costs.model_move
            };
            let mut model_moves_list = moves.clone();
            model_moves_list.push(Move::Model(t.label.clone()));
            open.push(Reverse((
                cost + move_cost,
                sequence,
                index,
                fired,
                model_moves_list,
            )));
            sequence += 1;
        }
    }
    match best {
        Some((cost, moves, syncs, log_moves, model_moves)) => Ok(AlignmentResult {
            cost,
            moves,
            syncs,
            log_moves,
            model_moves,
            fitness: fitness_of(cost, activities.len(), net.transitions().len()),
        }),
        None => Err(ProcessError::NoAlignment),
    }
}

/// ex4pm's derived fitness.
fn fitness_of(cost: u64, trace_len: usize, transitions: usize) -> f64 {
    let worst = trace_len as f64 + f64::max(1.0, transitions as f64 * 0.5);
    (1.0 - cost as f64 / worst).clamp(0.0, 1.0)
}

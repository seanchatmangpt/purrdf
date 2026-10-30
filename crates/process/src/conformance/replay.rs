// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Token replay with pm4py-convention fitness.
//!
//! Provenance: the bitmask fast path and the invisible-enable BFS follow
//! wasm4pm `conformance.rs`; the accounting (missing counted into consumed,
//! end-of-trace deficit/excess vs the best final marking) follows beam4pm
//! `art72_conformance.ex`; fitness is
//! `0.5*(1 - missing/max(1,consumed)) + 0.5*(1 - remaining/max(1,produced))`
//! clamped to `[0, 1]` (both donors' formula).

use std::collections::BTreeMap;

use purrdf_hash::fixed::FixedState;
use std::collections::HashSet;

use crate::model::petri::PetriNet;

/// One trace's replay result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraceReplay {
    /// Consumed tokens (underfed inputs count into this, art72).
    pub consumed: u64,
    /// Produced tokens.
    pub produced: u64,
    /// Missing tokens (deficits).
    pub missing: u64,
    /// Remaining tokens at the end (excess over the best final marking).
    pub remaining: u64,
    /// Whether the trace ends at a final marking.
    pub final_reached: bool,
    /// The deviations observed.
    pub deviations: Vec<Deviation>,
    /// The trace fitness.
    pub fitness: f64,
}

/// One replay deviation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deviation {
    /// The label names no transition.
    UnknownLabel(String),
    /// A transition could not fire (missing input tokens).
    MissingTokens(u32),
    /// The final marking was never reached.
    NoFinalMarking,
}

/// One log's replay: per-trace results plus the average fitness.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogReplay {
    /// Per-trace results, in input order.
    pub traces: Vec<TraceReplay>,
    /// The average fitness (0.0 for an empty log).
    pub average_fitness: f64,
}

/// Replay one trace against a net (the general weighted path).
#[must_use]
pub fn replay_trace(net: &PetriNet, activities: &[&str]) -> TraceReplay {
    let mut replay = TraceReplay::default();
    let mut marking: Vec<u32> = net.initial().to_vec();
    for label in activities {
        let matches: Vec<(usize, &crate::model::petri::Transition)> = net
            .transitions()
            .iter()
            .enumerate()
            .filter(|(_, t)| t.label.as_deref() == Some(label))
            .collect();
        if matches.is_empty() {
            replay
                .deviations
                .push(Deviation::UnknownLabel((*label).to_owned()));
            replay.missing += 1;
            continue;
        }
        let mut fired = false;
        for (_, t) in matches {
            let enabled = t
                .inputs
                .iter()
                .all(|(place, weight)| marking.get(*place).copied().unwrap_or(0) >= *weight);
            if enabled {
                for (place, weight) in &t.inputs {
                    replay.consumed += u64::from(*weight);
                    if let Some(slot) = marking.get_mut(*place) {
                        *slot = slot.saturating_sub(*weight);
                    }
                }
                for (place, weight) in &t.outputs {
                    replay.produced += u64::from(*weight);
                    if let Some(slot) = marking.get_mut(*place) {
                        *slot = slot.saturating_add(*weight);
                    }
                }
                fired = true;
                break;
            }
        }
        if !fired {
            replay.deviations.push(Deviation::MissingTokens(1));
            replay.missing += 1;
            replay.consumed += 1;
        }
    }
    finish(net, &marking, &mut replay);
    replay.fitness = fitness(&replay);
    replay
}

/// Replay one log: per-trace plus average fitness.
#[must_use]
pub fn replay_log(net: &PetriNet, log: &[Vec<&str>]) -> LogReplay {
    let traces: Vec<TraceReplay> = log.iter().map(|trace| replay_trace(net, trace)).collect();
    let average_fitness = if traces.is_empty() {
        0.0
    } else {
        traces.iter().map(|t| t.fitness).sum::<f64>() / traces.len() as f64
    };
    LogReplay {
        traces,
        average_fitness,
    }
}

/// End-of-trace accounting against the final marking with the smallest
/// deficit+excess (art72's deficit/excess, wasm4pm's best-final rule).
fn finish(net: &PetriNet, marking: &[u32], replay: &mut TraceReplay) {
    if net.final_markings().is_empty() {
        replay.final_reached = false;
        replay.deviations.push(Deviation::NoFinalMarking);
        replay.remaining = marking.iter().map(|count| u64::from(*count)).sum();
        return;
    }
    let mut best: Option<(u64, u64, u64)> = None;
    for final_marking in net.final_markings() {
        let mut deficit = 0u64;
        let mut excess = 0u64;
        let mut exact = true;
        for (index, count) in final_marking.iter().enumerate() {
            let have = marking.get(index).copied().unwrap_or(0);
            if have < *count {
                deficit += u64::from(count - have);
                exact = false;
            }
            if have > *count {
                excess += u64::from(have - count);
                exact = false;
            }
        }
        replay.final_reached = replay.final_reached || exact;
        let total = deficit + excess;
        if best.map_or(true, |(best_total, _, _)| total < best_total) {
            best = Some((total, deficit, excess));
        }
    }
    if let Some((_, deficit, excess)) = best {
        replay.missing += deficit;
        replay.consumed += deficit;
        replay.produced += excess;
        replay.remaining += excess;
    }
}

/// The van der Aalst fitness formula, clamped to `[0, 1]`.
#[must_use]
pub fn fitness(replay: &TraceReplay) -> f64 {
    let consumed = replay.consumed.max(1) as f64;
    let produced = replay.produced.max(1) as f64;
    let term1 = 1.0 - (replay.missing as f64) / consumed;
    let term2 = 1.0 - (replay.remaining as f64) / produced;
    (0.5 * term1 + 0.5 * term2).clamp(0.0, 1.0)
}

/// The u64-bitmask fast path: at most 64 places, unit weights, a safe
/// initial marking (wasm4pm `uses_bitmask_engine`'s conditions).
#[derive(Debug)]
pub struct BitmaskNet {
    in_masks: Vec<u64>,
    out_masks: Vec<u64>,
    labels: BTreeMap<String, Vec<usize>>,
    invisible: Vec<usize>,
    initial_mask: u64,
    final_masks: Vec<u64>,
}

impl BitmaskNet {
    /// Build the bitmask engine, or `None` when the net exceeds its
    /// conditions.
    #[must_use]
    pub fn try_from_net(net: &PetriNet) -> Option<Self> {
        if net.places() > 64 {
            return None;
        }
        let unit_weights = net.transitions().iter().all(|t| {
            t.inputs
                .iter()
                .chain(t.outputs.iter())
                .all(|(_, w)| *w == 1)
        });
        if !unit_weights || net.initial().iter().any(|count| *count > 1) {
            return None;
        }
        let mut in_masks = vec![0u64; net.transitions().len()];
        let mut out_masks = vec![0u64; net.transitions().len()];
        for (index, t) in net.transitions().iter().enumerate() {
            for (place, _) in &t.inputs {
                in_masks[index] |= 1u64 << place;
            }
            for (place, _) in &t.outputs {
                out_masks[index] |= 1u64 << place;
            }
        }
        let mut labels: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut invisible = Vec::new();
        for (index, t) in net.transitions().iter().enumerate() {
            match &t.label {
                Some(label) => labels.entry(label.clone()).or_default().push(index),
                None => invisible.push(index),
            }
        }
        let mut initial_mask = 0u64;
        for (place, count) in net.initial().iter().enumerate() {
            if *count > 0 {
                initial_mask |= 1u64 << place;
            }
        }
        let final_masks = net
            .final_markings()
            .iter()
            .map(|marking| {
                marking
                    .iter()
                    .enumerate()
                    .fold(0u64, |mask, (place, count)| {
                        if *count > 0 {
                            mask | (1u64 << place)
                        } else {
                            mask
                        }
                    })
            })
            .collect();
        Some(Self {
            in_masks,
            out_masks,
            labels,
            invisible,
            initial_mask,
            final_masks,
        })
    }

    /// Replay one trace on the bitmask path.
    #[must_use]
    pub fn replay(&self, activities: &[&str]) -> TraceReplay {
        let mut replay = TraceReplay::default();
        let mut marking = self.initial_mask;
        for label in activities {
            let Some(candidates) = self.labels.get(*label) else {
                replay
                    .deviations
                    .push(Deviation::UnknownLabel((*label).to_owned()));
                replay.missing += 1;
                continue;
            };
            let mut fired = false;
            for index in candidates {
                let in_mask = self.in_masks[*index];
                let mut enabled_via = marking & in_mask == in_mask;
                if !enabled_via {
                    if let Some(via) = self.enable_via_invisible(marking, in_mask, 8) {
                        marking = via;
                        enabled_via = true;
                    }
                }
                if enabled_via {
                    marking = (marking & !in_mask) | self.out_masks[*index];
                    replay.consumed += 1;
                    replay.produced += 1;
                    fired = true;
                    break;
                }
            }
            if !fired {
                replay.deviations.push(Deviation::MissingTokens(1));
                replay.missing += 1;
                replay.consumed += 1;
            }
        }
        // Final-marking accounting (unit weights: counts are popcounts).
        let mut best: Option<(u64, u64, u64)> = None;
        for final_mask in &self.final_masks {
            let deficit = (final_mask & !marking).count_ones() as u64;
            let excess = (marking & !final_mask).count_ones() as u64;
            if deficit == 0 && excess == 0 {
                replay.final_reached = true;
            }
            let total = deficit + excess;
            if best.map_or(true, |(best_total, _, _)| total < best_total) {
                best = Some((total, deficit, excess));
            }
        }
        if let Some((_, deficit, excess)) = best {
            replay.missing += deficit;
            replay.consumed += deficit;
            replay.produced += excess;
            replay.remaining += excess;
        }
        replay.fitness = fitness(&replay);
        replay
    }

    /// Minimal-enable BFS over invisible firings (wasm4pm
    /// `enable_via_invisible_bitmask`), bounded depth.
    fn enable_via_invisible(&self, marking: u64, target: u64, depth: usize) -> Option<u64> {
        if marking & target == target {
            return Some(marking);
        }
        let mut visited: HashSet<u64, FixedState> = HashSet::with_hasher(FixedState::new());
        let mut frontier = vec![marking];
        visited.insert(marking);
        for _ in 0..depth {
            let mut next_frontier = Vec::new();
            for state in frontier {
                for index in &self.invisible {
                    let in_mask = self.in_masks[*index];
                    if state & in_mask == in_mask {
                        let next = (state & !in_mask) | self.out_masks[*index];
                        if next & target == target {
                            return Some(next);
                        }
                        if visited.insert(next) {
                            next_frontier.push(next);
                        }
                    }
                }
            }
            frontier = next_frontier;
            if frontier.is_empty() {
                break;
            }
        }
        None
    }
}

// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Workflow nets: construction, structural validation, and soundness.
//!
//! Provenance: structure checks and the soundness property list follow ex4pm
//! `workflow_net.ex` (`validate_structure/1`, `verify_soundness/2`); the
//! token game follows its produce/consume weights with the pm4py convention
//! adopted crate-wide. Reachability is a deterministic sweep in transition
//! index order; `option_to_complete` subsumes the donors' separate
//! livelock/deadlock scans.

use purrdf_hash::fixed::FixedState;
use std::collections::{BTreeSet, HashSet};

use crate::error::ProcessError;

/// One net transition: an optional visible label (`None` = silent), and its
/// weighted input/output arcs as `(place_index, weight)` pairs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    /// The visible label; `None` fires invisibly.
    pub label: Option<String>,
    /// Consumed tokens: `(place, weight)`.
    pub inputs: Vec<(usize, u32)>,
    /// Produced tokens: `(place, weight)`.
    pub outputs: Vec<(usize, u32)>,
}

/// A place/transition net with one initial marking and any number of final
/// markings. Places are indices; transitions are indices into
/// [`PetriNet::transitions`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PetriNet {
    place_names: Vec<String>,
    transitions: Vec<Transition>,
    initial: Vec<u32>,
    final_markings: Vec<Vec<u32>>,
}

/// The reported soundness properties (ex4pm's check list; every property is
/// reported, never silently skipped).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundnessReport {
    /// Reachability stayed under the caller's marking budget.
    pub bounded: bool,
    /// No reachable marking puts more than one token in any place.
    pub one_safe: bool,
    /// Some reachable marking equals a final marking.
    pub final_reached: bool,
    /// Exactly one reachable marking equals a final marking.
    pub one_final_marking: bool,
    /// Every transition fires under some reachable marking.
    pub no_dead_transitions: bool,
    /// Every reachable marking can still reach a final marking.
    pub option_to_complete: bool,
    /// Every reachable non-final marking has an outgoing transition.
    pub deadlock_free: bool,
}

impl SoundnessReport {
    /// The conjunction of every reported property.
    #[must_use]
    pub fn sound(&self) -> bool {
        self.bounded
            && self.one_safe
            && self.final_reached
            && self.one_final_marking
            && self.no_dead_transitions
            && self.option_to_complete
            && self.deadlock_free
    }

    /// The failed property names, in check order.
    #[must_use]
    pub fn failures(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !self.bounded {
            out.push("bounded");
        }
        if !self.one_safe {
            out.push("one_safe");
        }
        if !self.final_reached {
            out.push("final_reached");
        }
        if !self.one_final_marking {
            out.push("one_final_marking");
        }
        if !self.no_dead_transitions {
            out.push("no_dead_transitions");
        }
        if !self.option_to_complete {
            out.push("option_to_complete");
        }
        if !self.deadlock_free {
            out.push("deadlock_free");
        }
        out
    }
}

/// Structural facts of a valid workflow net: the unique source and sink
/// places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WfFacts {
    /// The unique place index with no incoming arcs.
    pub source: usize,
    /// The unique place index with no outgoing arcs.
    pub sink: usize,
}

impl PetriNet {
    /// An empty net.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a place, returning its index.
    pub fn add_place(&mut self, name: impl Into<String>) -> usize {
        self.place_names.push(name.into());
        self.place_names.len() - 1
    }

    /// Add a transition, validating its arc place indices.
    ///
    /// # Errors
    /// [`ProcessError::OrderIndexOutOfRange`] naming the place count when any
    /// arc cites an unknown place.
    pub fn add_transition(
        &mut self,
        label: Option<String>,
        inputs: &[(usize, u32)],
        outputs: &[(usize, u32)],
    ) -> Result<usize, ProcessError> {
        let places = self.place_names.len();
        for (place, _) in inputs.iter().chain(outputs) {
            if *place >= places {
                return Err(ProcessError::OrderIndexOutOfRange {
                    pair: (*place, *place),
                    len: places,
                });
            }
        }
        self.transitions.push(Transition {
            label,
            inputs: inputs.to_vec(),
            outputs: outputs.to_vec(),
        });
        Ok(self.transitions.len() - 1)
    }

    /// Set the initial marking (one entry per place).
    ///
    /// # Errors
    /// [`ProcessError::MarkingLengthMismatch`] when the length differs from
    /// the place count.
    pub fn set_initial(&mut self, marking: Vec<u32>) -> Result<(), ProcessError> {
        if marking.len() != self.place_names.len() {
            return Err(ProcessError::MarkingLengthMismatch {
                got: marking.len(),
                expected: self.place_names.len(),
            });
        }
        self.initial = marking;
        Ok(())
    }

    /// Add a final marking (one entry per place).
    ///
    /// # Errors
    /// [`ProcessError::MarkingLengthMismatch`] when the length differs from
    /// the place count.
    pub fn add_final_marking(&mut self, marking: Vec<u32>) -> Result<(), ProcessError> {
        if marking.len() != self.place_names.len() {
            return Err(ProcessError::MarkingLengthMismatch {
                got: marking.len(),
                expected: self.place_names.len(),
            });
        }
        self.final_markings.push(marking);
        Ok(())
    }

    /// How many places.
    #[must_use]
    pub fn places(&self) -> usize {
        self.place_names.len()
    }

    /// A place's name, if the index is in range.
    #[must_use]
    pub fn place_name(&self, index: usize) -> Option<&str> {
        self.place_names.get(index).map(String::as_str)
    }

    /// The transitions.
    #[must_use]
    pub fn transitions(&self) -> &[Transition] {
        &self.transitions
    }

    /// The initial marking.
    #[must_use]
    pub fn initial(&self) -> &[u32] {
        &self.initial
    }

    /// The final markings.
    #[must_use]
    pub fn final_markings(&self) -> &[Vec<u32>] {
        &self.final_markings
    }

    /// Whether `transition` is enabled at `marking`.
    #[must_use]
    pub fn enabled(&self, marking: &[u32], transition: usize) -> bool {
        self.transitions.get(transition).map_or(false, |t| {
            t.inputs
                .iter()
                .all(|(place, weight)| marking.get(*place).copied().unwrap_or(0) >= *weight)
        })
    }

    /// Fire `transition` on `marking` in place; returns `false` when it is
    /// not enabled or the marking is not full-length (the marking is then
    /// untouched).
    #[must_use]
    pub fn fire(&self, marking: &mut Vec<u32>, transition: usize) -> bool {
        if marking.len() != self.places() || !self.enabled(marking, transition) {
            return false;
        }
        let Some(t) = self.transitions.get(transition) else {
            return false;
        };
        for (place, weight) in &t.inputs {
            if let Some(slot) = marking.get_mut(*place) {
                *slot -= *weight;
            }
        }
        for (place, weight) in &t.outputs {
            if let Some(slot) = marking.get_mut(*place) {
                *slot += *weight;
            }
        }
        true
    }

    /// Structural workflow-net validation (ex4pm `validate_structure/1`):
    /// exactly one source place, exactly one sink place, and every node on a
    /// source-to-sink path.
    ///
    /// # Errors
    /// [`ProcessError::InvalidWorkflowNet`] naming the failing rule.
    pub fn validate_workflow_net(&self) -> Result<WfFacts, ProcessError> {
        let places = self.places();
        if places == 0 {
            return Err(ProcessError::InvalidWorkflowNet {
                reason: "net has no places",
            });
        }
        let mut in_degree = vec![0usize; places];
        let mut out_degree = vec![0usize; places];
        for t in &self.transitions {
            for (place, _) in &t.inputs {
                out_degree[*place] += 1;
            }
            for (place, _) in &t.outputs {
                in_degree[*place] += 1;
            }
        }
        let sources: Vec<usize> = (0..places).filter(|p| in_degree[*p] == 0).collect();
        let sinks: Vec<usize> = (0..places).filter(|p| out_degree[*p] == 0).collect();
        if sources.len() != 1 {
            return Err(ProcessError::InvalidWorkflowNet {
                reason: "exactly one source place is required",
            });
        }
        if sinks.len() != 1 {
            return Err(ProcessError::InvalidWorkflowNet {
                reason: "exactly one sink place is required",
            });
        }
        let source = sources[0];
        let sink = sinks[0];
        // Bipartite forward reachability from the source, backward from the
        // sink. A node reaches itself trivially; arcs go place -> transition
        // (input side) and transition -> place (output side).
        let forward = self.reach(source, true);
        let backward = self.reach(sink, false);
        for node in 0..places {
            if !forward[node] || !backward[node] {
                return Err(ProcessError::InvalidWorkflowNet {
                    reason: "every place must lie on a source-to-sink path",
                });
            }
        }
        for t in 0..self.transitions.len() {
            if !forward[places + t] || !backward[places + t] {
                return Err(ProcessError::InvalidWorkflowNet {
                    reason: "every transition must lie on a source-to-sink path",
                });
            }
        }
        Ok(WfFacts { source, sink })
    }

    /// Reachability over the bipartite flow graph. With `forward`, walk
    /// source-side: a place reaches its consuming transitions, a transition
    /// reaches its output places. Backward is the mirror.
    fn reach(&self, start: usize, forward: bool) -> Vec<bool> {
        let total = self.places() + self.transitions.len();
        let mut seen = vec![false; total];
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(node) = stack.pop() {
            let neighbors: Vec<usize> = if node < self.places() {
                let place = node;
                if forward {
                    self.transitions
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| t.inputs.iter().any(|(p, _)| *p == place))
                        .map(|(t, _)| self.places() + t)
                        .collect()
                } else {
                    self.transitions
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| t.outputs.iter().any(|(p, _)| *p == place))
                        .map(|(t, _)| self.places() + t)
                        .collect()
                }
            } else {
                let t = &self.transitions[node - self.places()];
                if forward {
                    t.outputs.iter().map(|(p, _)| *p).collect()
                } else {
                    t.inputs.iter().map(|(p, _)| *p).collect()
                }
            };
            for neighbor in neighbors {
                if !seen[neighbor] {
                    seen[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
        seen
    }

    /// Soundness over the bounded reachability graph (ex4pm
    /// `verify_soundness/2`'s property list, each reported).
    ///
    /// Successors are generated in transition index order, so the visited
    /// set and every derived property are deterministic.
    #[must_use]
    pub fn verify_soundness(&self, max_markings: usize) -> SoundnessReport {
        let places = self.places();
        let mut report = SoundnessReport::default();
        if places == 0 {
            return report;
        }
        let mut visited: HashSet<Vec<u32>, FixedState> = HashSet::with_hasher(FixedState::new());
        let mut order: Vec<Vec<u32>> = Vec::new();
        let mut adjacency: Vec<Vec<usize>> = Vec::new();
        let mut fired: BTreeSet<usize> = BTreeSet::new();
        let mut queue: Vec<usize> = Vec::new();
        let initial = self.initial.clone();
        visited.insert(initial.clone());
        order.push(initial.clone());
        adjacency.push(Vec::new());
        queue.push(0);
        report.bounded = true;
        while let Some(index) = queue.pop() {
            let marking = order[index].clone();
            for t in 0..self.transitions.len() {
                if !self.enabled(&marking, t) {
                    continue;
                }
                fired.insert(t);
                let mut next = marking.clone();
                if !self.fire(&mut next, t) {
                    continue;
                }
                adjacency[index].push(order.len());
                if visited.insert(next.clone()) {
                    order.push(next);
                    adjacency.push(Vec::new());
                    if order.len() > max_markings {
                        report.bounded = false;
                        queue.clear();
                        break;
                    }
                    queue.push(order.len() - 1);
                }
            }
        }
        // one_safe: no place ever exceeds one token.
        report.one_safe = order
            .iter()
            .all(|marking| marking.iter().all(|count| *count <= 1));
        // Final-marking facts.
        let final_indices: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, marking)| {
                self.final_markings
                    .iter()
                    .any(|final_marking| final_marking == *marking)
            })
            .map(|(index, _)| index)
            .collect();
        report.final_reached = !final_indices.is_empty();
        report.one_final_marking = final_indices.len() == 1;
        // no_dead_transitions.
        report.no_dead_transitions = fired.len() == self.transitions.len();
        // option_to_complete: every visited marking reaches some final
        // marking, over the visited adjacency (backward sweep).
        let mut reaches_final = vec![false; order.len()];
        for index in &final_indices {
            reaches_final[*index] = true;
        }
        let mut changed = true;
        while changed {
            changed = false;
            for index in 0..order.len() {
                if reaches_final[index] {
                    continue;
                }
                if adjacency[index].iter().any(|succ| reaches_final[*succ]) {
                    reaches_final[index] = true;
                    changed = true;
                }
            }
        }
        report.option_to_complete = reaches_final.iter().all(|reaches| *reaches);
        // deadlock_free: every non-final reachable marking has a successor.
        report.deadlock_free = order.iter().enumerate().all(|(index, marking)| {
            !adjacency[index].is_empty()
                || self
                    .final_markings
                    .iter()
                    .any(|final_marking| final_marking == marking)
        });
        report
    }
}

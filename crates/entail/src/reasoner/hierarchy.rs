// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The shared quotient and strict reduction of a decided subsumption relation.

use purrdf_core::TermValue;

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Relations {
    pub(super) subsumptions: Vec<(TermValue, TermValue)>,
    pub(super) equivalences: Vec<(TermValue, TermValue)>,
    pub(super) direct: Vec<(TermValue, TermValue)>,
}

impl Relations {
    pub(super) fn record_pair(
        &mut self,
        left: TermValue,
        right: TermValue,
        forward: bool,
        backward: bool,
    ) {
        if forward {
            self.subsumptions.push((left.clone(), right.clone()));
        }
        if backward {
            self.subsumptions.push((right.clone(), left.clone()));
        }
        if forward && backward {
            self.equivalences.push((left, right));
        }
    }

    pub(super) fn reduce(
        &mut self,
        n: usize,
        name: &impl Fn(usize) -> TermValue,
        holds: &impl Fn(usize, usize) -> bool,
        poll: &mut impl FnMut() -> bool,
    ) {
        'derive: {
            // Representatives come from a union-find over the established
            // equivalence pairs (min-root union). A per-index scan for the lowest
            // pointwise-equivalent index is not transitive when the matrix holds an
            // undecided pair: `A = B` and `B = C` can hold while `A = C` does not, a
            // whole class splits, and the reduction emits an edge another edge
            // already implies through the split. The union closes the chain, and a
            // component's root is its lowest member, so the order is unchanged.
            fn find(parent: &mut [usize], x: usize) -> usize {
                let mut root = x;
                while parent[root] != root {
                    root = parent[root];
                }
                let mut at = x;
                while parent[at] != root {
                    let next = parent[at];
                    parent[at] = root;
                    at = next;
                }
                root
            }
            let mut parent: Vec<usize> = (0..n).collect();
            for i in 0..n {
                for j in 0..i {
                    if !poll() {
                        break 'derive;
                    }
                    if holds(i, j) && holds(j, i) {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        if a != b {
                            parent[a.max(b)] = a.min(b);
                        }
                    }
                }
            }
            let canonical: Vec<usize> = (0..n).filter(|&i| find(&mut parent, i) == i).collect();
            let strict = |a, b| holds(a, b) && !holds(b, a);
            for &i in &canonical {
                for &j in &canonical {
                    if !poll() {
                        break 'derive;
                    }
                    if !strict(i, j) {
                        continue;
                    }
                    let mut interposed = false;
                    for &k in &canonical {
                        if !poll() {
                            break 'derive;
                        }
                        if strict(i, k) && strict(k, j) {
                            interposed = true;
                            break;
                        }
                    }
                    if !interposed {
                        self.direct.push((name(i), name(j)));
                    }
                }
            }
        }
    }

    pub(super) fn sort(&mut self) {
        self.subsumptions.sort();
        self.equivalences.sort();
        self.direct.sort();
    }
}

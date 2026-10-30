// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The POWL 2.0 AST with bounded language semantics.
//!
//! Provenance: node set and language evaluation follow ex4pm
//! `lib/ex4pm_engine/powl.ex` + `powl/language.ex`; choice-graph validation
//! (delimiters, unique start/end, full connectivity, recorded cycles) follows
//! `powl/choice_graph.ex`; order-pair validation follows beam4pm
//! `hddl_powl.ex`; the loop unrolling and partial-order shuffle follow ex4pm
//! `powl/language.ex` + `powl/shuffle.ex`. Digests and ordering are
//! deterministic here where the Elixir donors were not.

use crate::error::ProcessError;

/// Where a choice-graph edge can attach.
///
/// The two sentinels are ex4pm's delimiters: an artificial start (`"▷"`)
/// and end (`"□"`) that every real node must sit between.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endpoint {
    /// The artificial start delimiter.
    Start,
    /// The artificial end delimiter.
    End,
    /// Child `i` of the enclosing choice graph.
    Child(usize),
}

/// Bounds for every exploration this module performs. All defaults follow the
/// donors' defaults (unroll 2, per-node visit cap 2).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Loop unroll bound and per-node visit cap for path enumeration.
    pub max_unroll: usize,
    /// Ceiling on materialized language strings; exceeded means refusal, not
    /// truncation.
    pub max_language: usize,
    /// Ceiling on materialized choice-graph paths.
    pub max_paths: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_unroll: 2,
            max_language: 65_536,
            max_paths: 1_024,
        }
    }
}

/// A POWL 2.0 model node.
///
/// Construct through the validated helpers ([`Powl::activity`],
/// [`Powl::partial_order`], [`Powl::choice_graph`], …); raw struct literals
/// bypass the order/choice-graph invariants and are the caller's own risk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Powl {
    /// A labelled activity.
    Activity {
        /// Model-local identifier.
        id: String,
        /// The visible label.
        label: String,
    },
    /// The silent step (tau).
    Silent {
        /// Model-local identifier.
        id: String,
    },
    /// Exclusive choice over children.
    Xor {
        /// Model-local identifier.
        id: String,
        /// Alternatives.
        children: Vec<Powl>,
    },
    /// Strict sequence over children.
    Sequence {
        /// Model-local identifier.
        id: String,
        /// Executed in order.
        children: Vec<Powl>,
    },
    /// Redo loop: `do_branch` then, zero or more times, `redo_branch` +
    /// `do_branch`.
    Loop {
        /// Model-local identifier.
        id: String,
        /// The body.
        do_branch: Box<Powl>,
        /// The redo part.
        redo_branch: Box<Powl>,
    },
    /// Concurrent children under a strict partial order over child indices.
    PartialOrder {
        /// Model-local identifier.
        id: String,
        /// The concurrent blocks.
        children: Vec<Powl>,
        /// Precedence edges over child indices, irreflexive and
        /// antisymmetric.
        order: Vec<(usize, usize)>,
    },
    /// A (possibly cyclic) choice graph between two delimiters.
    ChoiceGraph {
        /// Model-local identifier.
        id: String,
        /// The branch bodies.
        children: Vec<Powl>,
        /// Can-follow edges between [`Endpoint`]s; a bidirectional pair is
        /// the loop face.
        edges: Vec<(Endpoint, Endpoint)>,
    },
}

impl Powl {
    /// A labelled activity.
    #[must_use]
    pub fn activity(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Activity {
            id: id.into(),
            label: label.into(),
        }
    }

    /// The silent step.
    #[must_use]
    pub fn silent(id: impl Into<String>) -> Self {
        Self::Silent { id: id.into() }
    }

    /// A validated partial order: edges must be in range, irreflexive, and
    /// antisymmetric (beam4pm `hddl_powl.ex` rules).
    ///
    /// # Errors
    /// [`ProcessError::OrderNotIrreflexive`], [`ProcessError::OrderIndexOutOfRange`],
    /// or [`ProcessError::OrderNotAntisymmetric`].
    pub fn partial_order(
        id: impl Into<String>,
        children: Vec<Powl>,
        order: Vec<(usize, usize)>,
    ) -> Result<Self, ProcessError> {
        validate_order(children.len(), &order)?;
        Ok(Self::PartialOrder {
            id: id.into(),
            children,
            order,
        })
    }

    /// A validated choice graph (ex4pm `ChoiceGraph.new/3` rules): edge
    /// endpoints in range, no incoming edges into the start delimiter, no
    /// outgoing edges out of the end delimiter, and every child on a
    /// start-to-end path. Cycles are recorded, not refused (POWL 2.0 admits
    /// cyclic choice graphs).
    ///
    /// # Errors
    /// [`ProcessError::InvalidChoiceGraph`].
    pub fn choice_graph(
        id: impl Into<String>,
        children: Vec<Powl>,
        edges: Vec<(Endpoint, Endpoint)>,
    ) -> Result<Self, ProcessError> {
        validate_choice_graph(children.len(), &edges)?;
        Ok(Self::ChoiceGraph {
            id: id.into(),
            children,
            edges,
        })
    }

    /// The node's model-local identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Activity { id, .. }
            | Self::Silent { id }
            | Self::Xor { id, .. }
            | Self::Sequence { id, .. }
            | Self::Loop { id, .. }
            | Self::PartialOrder { id, .. }
            | Self::ChoiceGraph { id, .. } => id,
        }
    }

    /// The bounded language, sorted and deduplicated.
    ///
    /// Semantics per ex4pm `Language.evaluate/2`: sequences concatenate,
    /// choices union, loops unroll `(do·redo)^k·do` for `k ≤ max_unroll`,
    /// partial orders take the order-preserving shuffles of the child
    /// languages, and choice graphs concatenate along enumerated paths. The
    /// empty product budgets refuse loudly rather than truncate.
    ///
    /// # Errors
    /// [`ProcessError::LanguageBudgetExceeded`] when the product ceiling is
    /// hit.
    pub fn language(&self, limits: &Limits) -> Result<Vec<Vec<String>>, ProcessError> {
        let mut words = Vec::new();
        self.language_inner(limits, &mut words)?;
        words.sort_dedup();
        if words.len() > limits.max_language {
            return Err(ProcessError::LanguageBudgetExceeded {
                limit: limits.max_language,
            });
        }
        Ok(words)
    }

    fn language_inner(&self, limits: &Limits, out: &mut Words) -> Result<(), ProcessError> {
        match self {
            Self::Activity { label, .. } => {
                out.push(vec![label.clone()]);
            }
            Self::Silent { .. } => {
                out.push(Vec::new());
            }
            Self::Sequence { children, .. } => {
                let mut acc: Words = vec![Vec::new()];
                for child in children {
                    let mut next = Vec::new();
                    let mut part = Vec::new();
                    child.language_inner(limits, &mut part)?;
                    for prefix in &acc {
                        for suffix in &part {
                            let mut joined = prefix.clone();
                            joined.extend(suffix.iter().cloned());
                            next.push(joined);
                        }
                    }
                    acc = next;
                }
                out.extend(acc);
            }
            Self::Xor { children, .. } => {
                for child in children {
                    child.language_inner(limits, out)?;
                }
            }
            Self::Loop {
                do_branch,
                redo_branch,
                ..
            } => {
                let mut body = Vec::new();
                do_branch.language_inner(limits, &mut body)?;
                let mut redo = Vec::new();
                redo_branch.language_inner(limits, &mut redo)?;
                let mut step = Vec::new();
                for b in &body {
                    for r in &redo {
                        let mut joined = b.clone();
                        joined.extend(r.iter().cloned());
                        step.push(joined);
                    }
                }
                // k = 0: the body; k >= 1: (do·redo)^k · do.
                for prefix_len in 0..=limits.max_unroll {
                    let mut prefixes: Words = vec![Vec::new()];
                    for _ in 0..prefix_len {
                        let mut next = Vec::new();
                        for p in &prefixes {
                            for s in &step {
                                let mut joined = p.clone();
                                joined.extend(s.iter().cloned());
                                next.push(joined);
                            }
                        }
                        prefixes = next;
                    }
                    for p in &prefixes {
                        for b in &body {
                            let mut joined = p.clone();
                            joined.extend(b.iter().cloned());
                            out.push(joined);
                        }
                    }
                }
            }
            Self::PartialOrder {
                children, order, ..
            } => {
                let mut parts = Vec::new();
                for child in children {
                    let mut part = Vec::new();
                    child.language_inner(limits, &mut part)?;
                    part.sort_dedup();
                    parts.push(part);
                }
                out.extend(shuffle(&parts, order, limits)?);
            }
            Self::ChoiceGraph {
                children, edges, ..
            } => {
                let mut parts = Vec::new();
                for child in children {
                    let mut part = Vec::new();
                    child.language_inner(limits, &mut part)?;
                    part.sort_dedup();
                    parts.push(part);
                }
                for path in choice_graph_paths(children.len(), edges, limits)? {
                    let mut acc: Words = vec![Vec::new()];
                    for index in path {
                        let part = parts.get(index).ok_or(ProcessError::InvalidChoiceGraph {
                            reason: "path cites out-of-range child",
                        })?;
                        let mut next = Vec::new();
                        for prefix in &acc {
                            for suffix in part {
                                let mut joined = prefix.clone();
                                joined.extend(suffix.iter().cloned());
                                next.push(joined);
                            }
                        }
                        acc = next;
                    }
                    out.extend(acc);
                }
            }
        }
        if out.len() > limits.max_language {
            return Err(ProcessError::LanguageBudgetExceeded {
                limit: limits.max_language,
            });
        }
        Ok(())
    }
}

/// A list of words (each a label sequence) with sort-and-dedup.
type Words = Vec<Vec<String>>;

trait SortDedup {
    fn sort_dedup(&mut self);
}

impl SortDedup for Vec<Vec<String>> {
    fn sort_dedup(&mut self) {
        self.sort();
        self.dedup();
    }
}

/// Validate order edges (range, irreflexive, antisymmetric).
pub(crate) fn validate_order(len: usize, order: &[(usize, usize)]) -> Result<(), ProcessError> {
    for (i, j) in order {
        if *i >= len || *j >= len {
            return Err(ProcessError::OrderIndexOutOfRange {
                pair: (*i, *j),
                len,
            });
        }
        if i == j {
            return Err(ProcessError::OrderNotIrreflexive { index: *i });
        }
    }
    for (i, j) in order {
        if order.contains(&(*j, *i)) {
            return Err(ProcessError::OrderNotAntisymmetric {
                forward: (*i, *j),
                backward: (*j, *i),
            });
        }
    }
    Ok(())
}

/// Validate choice-graph edges (ex4pm `ChoiceGraph` rules).
fn validate_choice_graph(len: usize, edges: &[(Endpoint, Endpoint)]) -> Result<(), ProcessError> {
    let in_range = |e: &Endpoint| matches!(e, Endpoint::Child(i) if *i >= len);
    if edges.iter().any(|(a, b)| in_range(a) || in_range(b)) {
        return Err(ProcessError::InvalidChoiceGraph {
            reason: "edge endpoint out of range",
        });
    }
    if edges.iter().any(|(_, b)| *b == Endpoint::Start) {
        return Err(ProcessError::InvalidChoiceGraph {
            reason: "start delimiter has incoming edges",
        });
    }
    if edges.iter().any(|(a, _)| *a == Endpoint::End) {
        return Err(ProcessError::InvalidChoiceGraph {
            reason: "end delimiter has outgoing edges",
        });
    }
    if !edges.iter().any(|(a, _)| *a == Endpoint::Start) {
        return Err(ProcessError::InvalidChoiceGraph {
            reason: "start delimiter needs at least one outgoing edge",
        });
    }
    if !edges.iter().any(|(_, b)| *b == Endpoint::End) {
        return Err(ProcessError::InvalidChoiceGraph {
            reason: "end delimiter needs at least one incoming edge",
        });
    }
    // Every child must be reachable forward from Start and backward from End.
    let successors = |node: Endpoint| -> Vec<Endpoint> {
        edges
            .iter()
            .filter(|(a, _)| *a == node)
            .map(|(_, b)| *b)
            .collect()
    };
    let predecessors = |node: Endpoint| -> Vec<Endpoint> {
        edges
            .iter()
            .filter(|(_, b)| *b == node)
            .map(|(a, _)| *a)
            .collect()
    };
    let reachable = |start: Endpoint, next: &dyn Fn(Endpoint) -> Vec<Endpoint>| {
        let mut seen = vec![false; len];
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for succ in next(node) {
                if let Endpoint::Child(i) = succ {
                    if !seen[i] {
                        seen[i] = true;
                        stack.push(succ);
                    }
                }
            }
        }
        seen
    };
    let forward = reachable(Endpoint::Start, &successors);
    let backward = reachable(Endpoint::End, &predecessors);
    for i in 0..len {
        if !forward[i] || !backward[i] {
            return Err(ProcessError::InvalidChoiceGraph {
                reason: "child not on a start-to-end path",
            });
        }
    }
    Ok(())
}

/// Enumerate choice-graph paths from the start delimiter to the end,
/// returning child-index paths (delimiters excluded, as in ex4pm
/// `enumerate_paths/2`). Per-node visits are capped at `max_unroll`, and the
/// path count at `max_paths`.
///
/// # Errors
/// [`ProcessError::LanguageBudgetExceeded`] when the path ceiling is hit.
pub fn choice_graph_paths(
    len: usize,
    edges: &[(Endpoint, Endpoint)],
    limits: &Limits,
) -> Result<Vec<Vec<usize>>, ProcessError> {
    let successors = |node: Endpoint| -> Vec<Endpoint> {
        edges
            .iter()
            .filter(|(a, _)| *a == node)
            .map(|(_, b)| *b)
            .collect()
    };
    let mut paths = Vec::new();
    // Stack frames: (node, path-so-far, per-path visit counts).
    let mut stack: Vec<(Endpoint, Vec<usize>, Vec<usize>)> =
        vec![(Endpoint::Start, Vec::new(), vec![0; len])];
    while let Some((node, path, visits)) = stack.pop() {
        if node == Endpoint::End {
            paths.push(path);
            if paths.len() > limits.max_paths {
                return Err(ProcessError::LanguageBudgetExceeded {
                    limit: limits.max_paths,
                });
            }
            continue;
        }
        for succ in successors(node) {
            match succ {
                Endpoint::End => stack.push((Endpoint::End, path.clone(), visits.clone())),
                Endpoint::Start => {}
                Endpoint::Child(i) => {
                    if visits[i] < limits.max_unroll {
                        let mut next_visits = visits.clone();
                        next_visits[i] += 1;
                        let mut next_path = path.clone();
                        next_path.push(i);
                        stack.push((succ, next_path, next_visits));
                    }
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// All order-preserving interleavings of `parts` under the index poset
/// `order` (ex4pm `Shuffle.op_shuffle/2`).
///
/// A head of part `j` is eligible only when every poset predecessor of `j`
/// is fully consumed. The cartesian-product ceiling is checked up front.
///
/// # Errors
/// [`ProcessError::LanguageBudgetExceeded`] when the product ceiling is hit.
pub fn shuffle(
    parts: &[Words],
    order: &[(usize, usize)],
    limits: &Limits,
) -> Result<Words, ProcessError> {
    let mut total: usize = 1;
    for part in parts {
        total = total.saturating_mul(part.len());
        if total > limits.max_language {
            return Err(ProcessError::LanguageBudgetExceeded {
                limit: limits.max_language,
            });
        }
    }
    let predecessors = |j: usize| -> Vec<usize> {
        order
            .iter()
            .filter(|(_, t)| *t == j)
            .map(|(s, _)| *s)
            .collect()
    };
    let mut out = Vec::new();
    // Stack frames: (active remaining word lists, accumulated word).
    let mut stack: Vec<(Vec<&[Vec<String>]>, Vec<String>)> = vec![(
        parts.iter().map(|part| part.as_slice()).collect(),
        Vec::new(),
    )];
    while let Some((active, acc)) = stack.pop() {
        if active.iter().all(|part| part.is_empty()) {
            out.push(acc);
            if out.len() > limits.max_language {
                return Err(ProcessError::LanguageBudgetExceeded {
                    limit: limits.max_language,
                });
            }
            continue;
        }
        for j in 0..active.len() {
            if active[j].is_empty() {
                continue;
            }
            let head = &active[j][0];
            let eligible = predecessors(j).iter().all(|p| active[*p].is_empty());
            if !eligible {
                continue;
            }
            let next_active: Vec<&[Vec<String>]> = active
                .iter()
                .enumerate()
                .map(|(k, rest)| if k == j { &rest[1..] } else { *rest })
                .collect();
            let mut next_acc = acc.clone();
            next_acc.extend(head.iter().cloned());
            stack.push((next_active, next_acc));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! HTN networks and workflows projected to POWL.
//!
//! Provenance: the [`HtnNetwork`] type, its validation refusals, exact
//! order preservation (unclosed edge set on the node, closure only for the
//! digest), and plan digests follow beam4pm `hddl_powl.ex`; workflow
//! `after`-dependency validation follows ash_pplan's workflow verifiers.

use std::collections::BTreeSet;

use purrdf_hash::blake3;
use purrdf_lex::json::{Object as Json, Value, write_compact};

use crate::error::ProcessError;
use crate::model::powl::Powl;

/// An explicit HTN network: tasks with a strict ordering over task indices
/// (beam4pm `Network`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtnNetwork {
    /// Task names, one POWL activity each.
    pub tasks: Vec<String>,
    /// Strict order edges over task indices.
    pub ordering: Vec<(usize, usize)>,
}

impl HtnNetwork {
    /// Validate and construct: non-empty named tasks, ordering pairs
    /// in-range/irreflexive/antisymmetric (beam4pm's refusal set).
    ///
    /// # Errors
    /// [`ProcessError::MissingField`], [`ProcessError::OrderNotIrreflexive`],
    /// [`ProcessError::OrderIndexOutOfRange`],
    /// [`ProcessError::OrderNotAntisymmetric`].
    pub fn new(tasks: Vec<String>, ordering: Vec<(usize, usize)>) -> Result<Self, ProcessError> {
        if tasks.is_empty() {
            return Err(ProcessError::MissingField {
                document: "htn network".to_owned(),
                field: "tasks",
            });
        }
        for name in &tasks {
            if name.is_empty() {
                return Err(ProcessError::MissingField {
                    document: "htn network".to_owned(),
                    field: "task names",
                });
            }
        }
        crate::model::powl::validate_order(tasks.len(), &ordering)?;
        Ok(Self { tasks, ordering })
    }

    /// The network as a POWL partial order (exact edge preservation — the
    /// node keeps the unclosed edge set; closure is only for the digest).
    #[must_use]
    pub fn to_powl(&self) -> Powl {
        let children: Vec<Powl> = self
            .tasks
            .iter()
            .map(|name| Powl::activity(name.clone(), name.clone()))
            .collect();
        Powl::PartialOrder {
            id: "hddl-plan".to_owned(),
            children,
            order: self.ordering.clone(),
        }
    }

    /// The transitive closure of the ordering (beam4pm
    /// `transitive_closure/2` fixpoint).
    #[must_use]
    pub fn closed_ordering(&self) -> BTreeSet<(usize, usize)> {
        let n = self.tasks.len();
        let mut closed: BTreeSet<(usize, usize)> = self.ordering.iter().copied().collect();
        let mut changed = true;
        while changed {
            changed = false;
            for a in 0..n {
                for b in 0..n {
                    if closed.contains(&(a, b)) {
                        for c in 0..n {
                            if closed.contains(&(b, c)) && closed.insert((a, c)) {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        closed
    }

    /// The plan digest: `"hddl-plan-"` + 16 hex chars of BLAKE3 over the
    /// canonical serialization of tasks and the closed ordering.
    #[must_use]
    pub fn plan_digest(&self) -> String {
        let closed: Vec<(usize, usize)> = self.closed_ordering().into_iter().collect();
        let mut record = Json::new();
        record.push(
            "tasks",
            Value::Array(self.tasks.iter().map(|t| Value::from(t.as_str())).collect()),
        );
        record.push(
            "order",
            Value::Array(
                closed
                    .iter()
                    .map(|(a, b)| {
                        Value::Array(vec![Value::from(*a as u64), Value::from(*b as u64)])
                    })
                    .collect(),
            ),
        );
        let digest = blake3::hash(write_compact(&Value::Object(record)).as_bytes());
        format!("hddl-plan-{}", crate::log::integrity::short_digest(&digest))
    }
}

/// One declarative workflow: tasks with capabilities/outcomes and
/// `after`-dependencies, plus HTN methods (ash_pplan's `Model`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtnWorkflow {
    /// Workflow name.
    pub name: String,
    /// The goal capability, if named.
    pub goal: Option<String>,
    /// Declared tasks.
    pub tasks: Vec<HtnTaskDef>,
    /// Declared methods.
    pub methods: Vec<HtnMethod>,
}

/// One workflow task declaration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtnTaskDef {
    /// Task id.
    pub id: String,
    /// The capability this task requires.
    pub capability: String,
    /// Task ids that must complete before this task.
    pub after: Vec<String>,
    /// Possible outcome codes (FOND nondeterminism).
    pub outcomes: Vec<String>,
}

/// One HTN method.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtnMethod {
    /// Method id.
    pub id: String,
    /// The (abstract) task this method decomposes.
    pub task: String,
    /// The subtask ids in execution order.
    pub subtasks: Vec<String>,
    /// Optional precondition capability.
    pub precondition: Option<String>,
}

impl HtnWorkflow {
    /// Validate and construct: unique task/method ids, every method's task
    /// and subtask references resolve, and the `after` graph is acyclic.
    ///
    /// # Errors
    /// [`ProcessError::UnknownTask`] or
    /// [`ProcessError::DependencyCycle`].
    pub fn new(
        name: impl Into<String>,
        goal: Option<String>,
        tasks: Vec<HtnTaskDef>,
        methods: Vec<HtnMethod>,
    ) -> Result<Self, ProcessError> {
        let task_ids: BTreeSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        if task_ids.len() != tasks.len() {
            return Err(ProcessError::UnknownTask {
                reference: "duplicate task id".to_owned(),
            });
        }
        let method_ids: BTreeSet<&str> = methods.iter().map(|m| m.id.as_str()).collect();
        if method_ids.len() != methods.len() {
            return Err(ProcessError::UnknownTask {
                reference: "duplicate method id".to_owned(),
            });
        }
        for method in &methods {
            if !task_ids.contains(method.task.as_str()) {
                return Err(ProcessError::UnknownTask {
                    reference: method.task.clone(),
                });
            }
            for subtask in &method.subtasks {
                let decomposable = task_ids.contains(subtask.as_str())
                    || methods.iter().any(|m| m.task == *subtask);
                if !decomposable {
                    return Err(ProcessError::UnknownTask {
                        reference: subtask.clone(),
                    });
                }
            }
        }
        let cycle = find_cycle(&tasks);
        if let Some(cycle) = cycle {
            return Err(ProcessError::DependencyCycle { cycle });
        }
        Ok(Self {
            name: name.into(),
            goal,
            tasks,
            methods,
        })
    }
}

/// An `after`-dependency cycle, in walk order.
fn find_cycle(tasks: &[HtnTaskDef]) -> Option<Vec<String>> {
    let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let index_of = |id: &str| ids.iter().position(|candidate| *candidate == id);
    let mut state: Vec<u8> = vec![0; tasks.len()];
    let mut path: Vec<String> = Vec::new();
    for start in 0..tasks.len() {
        if state[start] == 0 {
            if let Some(cycle) = dfs(start, tasks, &index_of, &mut state, &mut path) {
                return Some(cycle);
            }
        }
    }
    None
}

/// DFS cycle detection over `after` edges (state 0/1/2 = new/open/closed).
fn dfs(
    node: usize,
    tasks: &[HtnTaskDef],
    index_of: &dyn Fn(&str) -> Option<usize>,
    state: &mut Vec<u8>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    state[node] = 1;
    path.push(tasks[node].id.clone());
    for dep in &tasks[node].after {
        let Some(next) = index_of(dep) else {
            return Some(vec![dep.clone()]);
        };
        if state[next] == 1 {
            let Some(start) = path.iter().position(|id| *id == tasks[next].id) else {
                return None;
            };
            let mut cycle = path[start..].to_vec();
            cycle.push(tasks[next].id.clone());
            state[node] = 2;
            path.pop();
            return Some(cycle);
        }
        if state[next] == 0 {
            if let Some(cycle) = dfs(next, tasks, index_of, state, path) {
                return Some(cycle);
            }
        }
    }
    state[node] = 2;
    path.pop();
    None
}

/// The workflow as a POWL partial order over its tasks, edges from the
/// `after` dependency set.
///
/// # Errors
/// [`ProcessError::UnknownTask`] for a dangling `after` reference.
pub fn workflow_to_powl(workflow: &HtnWorkflow) -> Result<Powl, ProcessError> {
    let ids: Vec<&str> = workflow.tasks.iter().map(|t| t.id.as_str()).collect();
    let mut ordering: Vec<(usize, usize)> = Vec::new();
    for (index, task) in workflow.tasks.iter().enumerate() {
        for dep in &task.after {
            let Some(dep_index) = ids.iter().position(|candidate| *candidate == dep.as_str())
            else {
                return Err(ProcessError::UnknownTask {
                    reference: dep.clone(),
                });
            };
            ordering.push((dep_index, index));
        }
    }
    let children: Vec<Powl> = workflow
        .tasks
        .iter()
        .map(|task| Powl::activity(task.id.clone(), task.capability.clone()))
        .collect();
    Powl::partial_order(workflow.name.clone(), children, ordering)
}

/// A workflow's network view (tasks + derived ordering), for digesting.
#[must_use]
pub fn network_of(workflow: &HtnWorkflow) -> HtnNetwork {
    let tasks: Vec<String> = workflow.tasks.iter().map(|t| t.id.clone()).collect();
    let ids: Vec<&str> = tasks.iter().map(String::as_str).collect();
    let mut ordering: Vec<(usize, usize)> = Vec::new();
    for (index, task) in workflow.tasks.iter().enumerate() {
        for dep in &task.after {
            if let Some(dep_index) = ids.iter().position(|candidate| *candidate == dep.as_str()) {
                ordering.push((dep_index, index));
            }
        }
    }
    HtnNetwork { tasks, ordering }
}

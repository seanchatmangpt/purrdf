// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! FOND policy validation and synthesis.
//!
//! Provenance: the domain/policy contract, `validate_policy`'s check chain,
//! the strong/strong-cyclic fixpoints, `Unsolvable` witnesses, the policy
//! switch with its horizon, and the counterexample classes follow ash_pplan
//! `fond.ex`/`synthesis.ex`/`policy_switch.ex`/`counterexample.ex`,
//! determinized (sorted iteration, min-action tie-breaks).

use std::collections::{BTreeMap, BTreeSet};

use crate::error::ProcessError;

/// The policy mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// All outcomes reach a goal.
    Strong,
    /// Under fair execution all executions reach a goal.
    StrongCyclic,
}

impl Mode {
    /// The stable mode code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::StrongCyclic => "strong-cyclic",
        }
    }
}

impl core::fmt::Display for Mode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.code())
    }
}

/// A FOND policy: state → action.
pub type FondPolicy = BTreeMap<String, String>;

/// A FOND domain: states, initial, goals, and the nondeterministic
/// transition map `state -> action -> [outcome states]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FondDomain {
    /// Declared states.
    pub states: BTreeSet<String>,
    /// The initial state.
    pub initial: String,
    /// Goal states (absorbing).
    pub goals: BTreeSet<String>,
    /// The nondeterministic transition map.
    pub transitions: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

impl FondDomain {
    /// Validate and construct: the initial state, every goal, and every
    /// transition/outcome state must be declared.
    ///
    /// # Errors
    /// [`ProcessError::UndeclaredStates`].
    pub fn new(
        states: impl IntoIterator<Item = String>,
        initial: impl Into<String>,
        goals: impl IntoIterator<Item = String>,
        transitions: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    ) -> Result<Self, ProcessError> {
        let states: BTreeSet<String> = states.into_iter().collect();
        let initial = initial.into();
        let goals: BTreeSet<String> = goals.into_iter().collect();
        let mut undeclared: BTreeSet<String> = BTreeSet::new();
        if !states.contains(&initial) {
            undeclared.insert(initial.clone());
        }
        for goal in &goals {
            if !states.contains(goal) {
                undeclared.insert(goal.clone());
            }
        }
        for (state, actions) in &transitions {
            if !states.contains(state) {
                undeclared.insert(state.clone());
            }
            for (action, outcomes) in actions {
                let _ = action;
                for outcome in outcomes {
                    if !states.contains(outcome) {
                        undeclared.insert(outcome.clone());
                    }
                }
            }
        }
        if !undeclared.is_empty() {
            return Err(ProcessError::UndeclaredStates {
                states: undeclared.into_iter().collect(),
            });
        }
        Ok(Self {
            states,
            initial,
            goals,
            transitions,
        })
    }

    /// The reachable state set under `policy` from the initial state, with
    /// ash_pplan's check chain: every reachable non-goal state needs a
    /// policy entry naming an admitted action; goals are absorbing.
    ///
    /// # Errors
    /// [`ProcessError::MissingPolicyAction`],
    /// [`ProcessError::UnavailablePolicyAction`].
    pub fn reachable_under_policy(
        &self,
        policy: &FondPolicy,
    ) -> Result<BTreeSet<String>, ProcessError> {
        let mut reachable: BTreeSet<String> = BTreeSet::new();
        let mut queue: Vec<String> = vec![self.initial.clone()];
        reachable.insert(self.initial.clone());
        while let Some(state) = queue.pop() {
            if self.goals.contains(&state) {
                continue;
            }
            let Some(action) = policy.get(&state) else {
                return Err(ProcessError::MissingPolicyAction { state });
            };
            let Some(outcomes) = self
                .transitions
                .get(&state)
                .and_then(|actions| actions.get(action))
            else {
                let available: Vec<String> = self
                    .transitions
                    .get(&state)
                    .map(|actions| actions.keys().cloned().collect())
                    .unwrap_or_default();
                return Err(ProcessError::UnavailablePolicyAction {
                    state,
                    action: action.clone(),
                    available,
                });
            };
            for outcome in outcomes {
                if reachable.insert(outcome.clone()) {
                    queue.push(outcome.clone());
                }
            }
        }
        Ok(reachable)
    }

    /// Validate a policy in `mode` (ash_pplan `validate_policy/4`).
    ///
    /// # Errors
    /// [`ProcessError::UnknownPolicyStates`],
    /// [`ProcessError::MissingPolicyAction`],
    /// [`ProcessError::UnavailablePolicyAction`],
    /// [`ProcessError::NotStrong`], or [`ProcessError::NotStrongCyclic`].
    pub fn validate_policy(
        &self,
        policy: &FondPolicy,
        mode: Mode,
    ) -> Result<BTreeSet<String>, ProcessError> {
        let unknown: Vec<String> = policy
            .keys()
            .filter(|state| !self.states.contains(*state))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(ProcessError::UnknownPolicyStates { states: unknown });
        }
        let reachable = self.reachable_under_policy(policy)?;
        match mode {
            Mode::Strong => {
                let winning = self.strong_winning_region();
                let losing: Vec<String> = reachable.difference(&winning).cloned().collect();
                if losing.is_empty() {
                    Ok(reachable)
                } else {
                    Err(ProcessError::NotStrong {
                        losing_states: losing,
                    })
                }
            }
            Mode::StrongCyclic => {
                let goal_reaching = self.goal_reaching_set();
                let stranded: Vec<String> = reachable.difference(&goal_reaching).cloned().collect();
                if stranded.is_empty() {
                    Ok(reachable)
                } else {
                    Err(ProcessError::NotStrongCyclic {
                        states_without_goal_path: stranded,
                    })
                }
            }
        }
    }

    /// The least strong-winning region: goals plus every state with an
    /// admitted action whose outcomes are all winning (round-robin fixpoint,
    /// deterministic).
    #[must_use]
    pub fn strong_winning_region(&self) -> BTreeSet<String> {
        let mut winning: BTreeSet<String> = self.goals.clone();
        let mut changed = true;
        while changed {
            changed = false;
            for state in &self.states {
                if winning.contains(state) || !self.transitions.contains_key(state) {
                    continue;
                }
                let admitted: Vec<&Vec<String>> = self.transitions[state].values().collect();
                let all_in = admitted
                    .iter()
                    .any(|outcomes| outcomes.iter().all(|o| winning.contains(o)));
                if all_in {
                    winning.insert(state.clone());
                    changed = true;
                }
            }
        }
        winning
    }

    /// The set of states with a policy-path to a goal under SOME admitted
    /// action: goals plus their backward closure over forward edges.
    #[must_use]
    pub fn goal_reaching_set(&self) -> BTreeSet<String> {
        // Forward reachability to goals: a state qualifies when it admits an
        // action with at least one outcome that qualifies (fair-execution
        // reachability, ash_pplan's goal-path semantics).
        let mut qualified: BTreeSet<String> = self.goals.clone();
        let mut changed = true;
        while changed {
            changed = false;
            for state in &self.states {
                if qualified.contains(state) || !self.transitions.contains_key(state) {
                    continue;
                }
                let qualifies = self.transitions[state]
                    .values()
                    .any(|outcomes| outcomes.iter().any(|o| qualified.contains(o)));
                if qualifies {
                    qualified.insert(state.clone());
                    changed = true;
                }
            }
        }
        qualified
    }

    /// Synthesize a policy in `mode` (ash_pplan `Synthesis.synthesize/3`):
    /// strong = backward attractor layering; strong-cyclic = greatest
    /// fixpoint with min-action admission; both witness their refusal.
    ///
    /// # Errors
    /// [`ProcessError::Unsolvable`].
    pub fn synthesize(&self, mode: Mode) -> Result<FondPolicy, ProcessError> {
        match mode {
            Mode::Strong => self.synthesize_strong(),
            Mode::StrongCyclic => self.synthesize_strong_cyclic(),
        }
    }

    /// The strong policy: each winning non-goal state takes its smallest
    /// action whose outcomes all win.
    fn synthesize_strong(&self) -> Result<FondPolicy, ProcessError> {
        let winning = self.strong_winning_region();
        if !winning.contains(&self.initial) {
            return Err(ProcessError::Unsolvable {
                mode: Mode::Strong,
                witness_states: self.witness_states(),
            });
        }
        let mut policy: FondPolicy = BTreeMap::new();
        for state in &self.states {
            if winning.contains(state) && !self.goals.contains(state) {
                if let Some(actions) = self.transitions.get(state) {
                    for (action, outcomes) in actions {
                        if outcomes.iter().all(|outcome| winning.contains(outcome)) {
                            policy.insert(state.clone(), action.clone());
                            break;
                        }
                    }
                }
            }
        }
        Ok(policy)
    }

    /// The strong-cyclic policy: greatest fixpoint of the candidate set; a
    /// candidate state keeps the smallest action whose outcomes all stay in
    /// the goal-reaching set.
    fn synthesize_strong_cyclic(&self) -> Result<FondPolicy, ProcessError> {
        let goal_reaching = self.goal_reaching_set();
        if !goal_reaching.contains(&self.initial) {
            return Err(ProcessError::Unsolvable {
                mode: Mode::StrongCyclic,
                witness_states: self.witness_states(),
            });
        }
        let mut policy: FondPolicy = BTreeMap::new();
        for state in &self.states {
            if goal_reaching.contains(state) && !self.goals.contains(state) {
                if let Some(actions) = self.transitions.get(state) {
                    for (action, outcomes) in actions {
                        if outcomes
                            .iter()
                            .any(|outcome| goal_reaching.contains(outcome))
                        {
                            policy.insert(state.clone(), action.clone());
                            break;
                        }
                    }
                }
            }
        }
        Ok(policy)
    }

    /// Sorted DFS-reachable states under any action (the `Unsolvable`
    /// witness, always containing the initial state).
    #[must_use]
    pub fn witness_states(&self) -> Vec<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut queue = vec![self.initial.clone()];
        seen.insert(self.initial.clone());
        while let Some(state) = queue.pop() {
            if let Some(actions) = self.transitions.get(&state) {
                for outcomes in actions.values() {
                    for outcome in outcomes {
                        if seen.insert(outcome.clone()) {
                            queue.push(outcome.clone());
                        }
                    }
                }
            }
        }
        seen.into_iter().collect()
    }
}

/// Try each mode in order with a horizon on recorded attempts (ash_pplan
/// `PolicySwitch.select/3`; horizon 9 by default).
///
/// # Errors
/// [`ProcessError::Unsolvable`] when every mode fails inside the horizon,
/// or [`ProcessError::HorizonExceeded`].
pub fn policy_switch(
    domain: &FondDomain,
    modes: &[Mode],
    horizon: usize,
) -> Result<(Mode, FondPolicy), ProcessError> {
    let mut attempts: Vec<(&'static str, &'static str)> = Vec::new();
    for mode in modes {
        match domain.synthesize(*mode) {
            Ok(policy) => return Ok((*mode, policy)),
            Err(error) => attempts.push((mode.code(), error.code())),
        }
    }
    if attempts.len() >= horizon {
        return Err(ProcessError::HorizonExceeded { horizon, attempts });
    }
    Err(ProcessError::Unsolvable {
        mode: *modes.last().unwrap_or(&Mode::Strong),
        witness_states: domain.witness_states(),
    })
}

/// Counterexample classes (ash_pplan `counterexample.ex`'s mapping).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterexampleClass {
    /// Liveness failure (not strong / not strong-cyclic).
    Liveness,
    /// Deadlock-class failure (missing or unavailable policy action).
    Deadlock,
    /// Identity failure (unknown states).
    Identity,
    /// Shape failure (malformed request).
    Shape,
    /// Not classifiable.
    Unclassifiable,
}

/// Classify one refusal code (ash_pplan `classify/1`).
#[must_use]
pub fn classify(code: &str) -> CounterexampleClass {
    match code {
        "fond-not-strong" | "fond-not-strong-cyclic" => CounterexampleClass::Liveness,
        "fond-missing-policy-action" | "fond-unavailable-action" => CounterexampleClass::Deadlock,
        "fond-unknown-policy-states" | "fond-undeclared-states" => CounterexampleClass::Identity,
        "process-missing-field" | "ocel-invalid-timestamp" => CounterexampleClass::Shape,
        _ => CounterexampleClass::Unclassifiable,
    }
}

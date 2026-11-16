// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The crate's typed refusals: every variant a stable code, in the
//! `purrdf-hash` structured-error style.

use purrdf_hash::error::{ErrorValue, StructuredError};
use purrdf_hash::{error_sources, error_visit};
use purrdf_lex::json::Error as JsonError;
use purrdf_xsd::XsdError;

/// Everything this crate refuses, typed. Refusals are claims: each variant
/// names its subject and, where a bound exists, the bound that was exceeded.
#[derive(Debug)]
pub enum ProcessError {
    /// A JSON document failed to parse (`purrdf-lex` owns the grammar).
    Json(JsonError),
    /// A lexical timestamp failed xsd:dateTime validation.
    Xsd(XsdError),
    /// The log document has no `events` key in any accepted dialect.
    MissingEvents,
    /// The log document has no `objects` key in any accepted dialect.
    MissingObjects,
    /// An event or object cites an object id absent from the log.
    UnknownObjectReference {
        /// The citing document (`event:<id>` / `object:<id>`).
        subject: String,
        /// The unknown object id.
        object: String,
    },
    /// A lexical timestamp is not a valid xsd:dateTime.
    InvalidTimestamp {
        /// The rejected lexical form.
        lexical: String,
    },
    /// Two documents claim one identity in a single log.
    DuplicateIdentity {
        /// `"event"` or `"object"`.
        kind: &'static str,
        /// The duplicated id.
        id: String,
    },
    /// An object type is absent from the declared `objectTypes` (only
    /// enforced when the log declares any).
    UndeclaredObjectType {
        /// The undeclared type name.
        type_name: String,
    },
    /// An event type is absent from the declared `eventTypes` (only enforced
    /// when the log declares any).
    UndeclaredEventType {
        /// The undeclared type name.
        type_name: String,
    },
    /// An attribute value is not an OCEL scalar (arrays/objects are
    /// extensions, refused here).
    UnsupportedAttributeShape {
        /// Where the value sat (`event:<id>.<name>` / `object:<id>.<name>`).
        context: String,
    },
    /// A relationship document is malformed (non-string ids, wrong shape).
    InvalidRelationship {
        /// Where the malformed value sat.
        context: String,
    },
    /// Input exceeds the 512 MB ingestion guard.
    InputTooLarge {
        /// Observed byte length.
        bytes: u64,
        /// The refused ceiling.
        limit: u64,
    },
    /// A chained event lacks its `pi_hash`/`pi_parent` attributes.
    MissingChainAttribute {
        /// The event's canonical index (0-based).
        index: usize,
    },
    /// A chained event does not match the recomputed chain.
    ChainBreak {
        /// The event's canonical index (0-based).
        index: usize,
        /// What broke: the `pi_parent` link or the recomputed `pi_hash`.
        detail: &'static str,
    },
    /// A net fails structural validation (single source/sink, connectivity).
    InvalidWorkflowNet {
        /// The failing structural rule.
        reason: &'static str,
    },
    /// One or more soundness properties failed; each reason is one reported
    /// property.
    NetNotSound {
        /// The failed properties, in check order.
        reasons: Vec<&'static str>,
    },
    /// A choice graph violates its formalism (delimiter wiring,
    /// connectivity).
    InvalidChoiceGraph {
        /// The failing formalism rule.
        reason: &'static str,
    },
    /// A can-follow/order relation repeats an index (`{i, i}`).
    OrderNotIrreflexive {
        /// The offending index.
        index: usize,
    },
    /// An order pair is out of range for the child count.
    OrderIndexOutOfRange {
        /// The offending pair.
        pair: (usize, usize),
        /// The child count the pair must index.
        len: usize,
    },
    /// Both `{i, j}` and `{j, i}` are asserted.
    OrderNotAntisymmetric {
        /// The forward pair.
        forward: (usize, usize),
        /// The reversed pair.
        backward: (usize, usize),
    },
    /// Bounded language enumeration exceeded its product budget.
    LanguageBudgetExceeded {
        /// The product-size ceiling that was exceeded.
        limit: usize,
    },
    /// A workflow names a method or dependency targeting an unknown task.
    UnknownTask {
        /// The dangling reference.
        reference: String,
    },
    /// A workflow's `after` dependencies contain a cycle.
    DependencyCycle {
        /// The tasks on the detected cycle, in walk order.
        cycle: Vec<String>,
    },
    /// A FOND transition graph names undeclared states.
    UndeclaredStates {
        /// The sorted undeclared states.
        states: Vec<String>,
    },
    /// A policy names states absent from the domain.
    UnknownPolicyStates {
        /// The sorted unknown states.
        states: Vec<String>,
    },
    /// A reachable non-goal state has no policy entry.
    MissingPolicyAction {
        /// The state without a policy entry.
        state: String,
    },
    /// A policy action is not admitted by the state's transition map.
    UnavailablePolicyAction {
        /// The state whose transition map was consulted.
        state: String,
        /// The refused action.
        action: String,
        /// The actions the state does admit.
        available: Vec<String>,
    },
    /// The reachable space under the policy is not strong-winning.
    NotStrong {
        /// Sorted reachable states outside the winning region.
        losing_states: Vec<String>,
    },
    /// The reachable space under the policy lacks goal-paths under fair
    /// execution.
    NotStrongCyclic {
        /// Sorted reachable states without a goal path.
        states_without_goal_path: Vec<String>,
    },
    /// No policy exists in the requested mode; the witness is the set of
    /// states the synthesis could not lift into the winning region.
    Unsolvable {
        /// The requested mode.
        mode: crate::synth::Mode,
        /// Sorted witness states (always contains the initial state).
        witness_states: Vec<String>,
    },
    /// Policy-switch attempts exceeded their horizon.
    HorizonExceeded {
        /// The horizon that was exceeded.
        horizon: usize,
        /// One (mode, refusal code) pair per attempt.
        attempts: Vec<(&'static str, &'static str)>,
    },
    /// Shuffle membership exceeded its backtracking budget on an ambiguous
    /// position.
    AmbiguityBudgetExceeded {
        /// The backtracking-node budget that was exceeded.
        budget: u64,
    },
    /// Alignment search exceeded its iteration budget.
    AlignmentBudgetExceeded {
        /// The iteration budget that was exceeded.
        iterations: u64,
    },
    /// The A\* open set drained without reaching a final marking.
    NoAlignment,
    /// A marking's length differs from the net's place count.
    MarkingLengthMismatch {
        /// Observed marking length.
        got: usize,
        /// Required place count.
        expected: usize,
    },
    /// A required field is absent from one document.
    MissingField {
        /// Which document (`event[3]`, `object[1]`, …).
        document: String,
        /// The absent field's accepted spellings.
        field: &'static str,
    },
    /// The cgap `Batch` layer refused a projected table.
    BatchRefused {
        /// Which projection table (`events`, `event_attrs`, …).
        table: &'static str,
        /// The batch layer's refusal detail.
        detail: String,
    },
}

impl core::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "json: {e}"),
            Self::Xsd(e) => write!(f, "xsd: {e}"),
            Self::MissingEvents => f.write_str("log has no events key"),
            Self::MissingObjects => f.write_str("log has no objects key"),
            Self::UnknownObjectReference { subject, object } => {
                write!(f, "{subject} cites unknown object {object:?}")
            }
            Self::InvalidTimestamp { lexical } => write!(f, "invalid xsd:dateTime {lexical:?}"),
            Self::DuplicateIdentity { kind, id } => write!(f, "duplicate {kind} id {id:?}"),
            Self::UndeclaredObjectType { type_name } => {
                write!(f, "object type {type_name:?} is undeclared")
            }
            Self::UndeclaredEventType { type_name } => {
                write!(f, "event type {type_name:?} is undeclared")
            }
            Self::UnsupportedAttributeShape { context } => {
                write!(f, "attribute at {context} is not an OCEL scalar")
            }
            Self::InvalidRelationship { context } => {
                write!(f, "malformed relationship at {context}")
            }
            Self::InputTooLarge { bytes, limit } => {
                write!(f, "input too large: {bytes} bytes (max {limit})")
            }
            Self::MissingChainAttribute { index } => {
                write!(f, "event {index} lacks its integrity-chain attributes")
            }
            Self::ChainBreak { index, detail } => {
                write!(f, "integrity chain breaks at event {index}: {detail}")
            }
            Self::InvalidWorkflowNet { reason } => {
                write!(f, "invalid workflow net: {reason}")
            }
            Self::NetNotSound { reasons } => {
                let list = reasons.join(", ");
                write!(f, "net is not sound: {list}")
            }
            Self::InvalidChoiceGraph { reason } => {
                write!(f, "invalid choice graph: {reason}")
            }
            Self::OrderNotIrreflexive { index } => {
                write!(f, "order pair ({index}, {index}) is not irreflexive")
            }
            Self::OrderIndexOutOfRange { pair, len } => {
                write!(f, "order pair {pair:?} out of range for {len} children")
            }
            Self::OrderNotAntisymmetric { forward, backward } => {
                write!(f, "order asserts both {forward:?} and {backward:?}")
            }
            Self::LanguageBudgetExceeded { limit } => {
                write!(f, "language enumeration exceeds {limit} strings")
            }
            Self::UnknownTask { reference } => write!(f, "unknown task {reference:?}"),
            Self::DependencyCycle { cycle } => {
                let list = cycle.join(" -> ");
                write!(f, "dependency cycle: {list}")
            }
            Self::UndeclaredStates { states } => {
                write!(f, "transitions name undeclared states: {:?}", states)
            }
            Self::UnknownPolicyStates { states } => {
                write!(f, "policy names unknown states: {:?}", states)
            }
            Self::MissingPolicyAction { state } => {
                write!(f, "reachable non-goal state {state:?} has no policy action")
            }
            Self::UnavailablePolicyAction {
                state,
                action,
                available,
            } => {
                write!(
                    f,
                    "state {state:?} refuses action {action:?} (available: {available:?})"
                )
            }
            Self::NotStrong { losing_states } => {
                write!(
                    f,
                    "policy is not strong; losing states: {:?}",
                    losing_states
                )
            }
            Self::NotStrongCyclic {
                states_without_goal_path,
            } => {
                write!(
                    f,
                    "policy is not strong-cyclic; no goal path from: {:?}",
                    states_without_goal_path
                )
            }
            Self::Unsolvable {
                mode,
                witness_states,
            } => {
                write!(
                    f,
                    "unsolvable ({mode:?}); witness states: {:?}",
                    witness_states
                )
            }
            Self::HorizonExceeded { horizon, attempts } => {
                write!(
                    f,
                    "policy switch exceeded horizon {horizon}; attempts: {attempts:?}"
                )
            }
            Self::AmbiguityBudgetExceeded { budget } => {
                write!(f, "shuffle backtracking exceeded {budget} nodes")
            }
            Self::AlignmentBudgetExceeded { iterations } => {
                write!(f, "alignment search exceeded {iterations} iterations")
            }
            Self::NoAlignment => f.write_str("no alignment exists"),
            Self::MarkingLengthMismatch { got, expected } => {
                write!(f, "marking length {got} does not match {expected} places")
            }
            Self::MissingField { document, field } => {
                write!(f, "{document} is missing its {field:?} field")
            }
            Self::BatchRefused { table, detail } => {
                write!(f, "batch layer refused the {table} table: {detail}")
            }
        }
    }
}

error_sources!(ProcessError, Json, Xsd);

impl ProcessError {
    /// The stable variant code (the same identity the structured-error
    /// visitor reports).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Json(_) => "process-json",
            Self::Xsd(_) => "process-xsd",
            Self::MissingEvents => "ocel-missing-events",
            Self::MissingObjects => "ocel-missing-objects",
            Self::UnknownObjectReference { .. } => "ocel-unknown-object",
            Self::InvalidTimestamp { .. } => "ocel-invalid-timestamp",
            Self::DuplicateIdentity { .. } => "ocel-duplicate-identity",
            Self::UndeclaredObjectType { .. } => "ocel-undeclared-object-type",
            Self::UndeclaredEventType { .. } => "ocel-undeclared-event-type",
            Self::UnsupportedAttributeShape { .. } => "ocel-attribute-shape",
            Self::InvalidRelationship { .. } => "ocel-invalid-relationship",
            Self::InputTooLarge { .. } => "ocel-input-too-large",
            Self::MissingChainAttribute { .. } => "ocel-chain-missing-attribute",
            Self::ChainBreak { .. } => "ocel-chain-break",
            Self::InvalidWorkflowNet { .. } => "petri-invalid-structure",
            Self::NetNotSound { .. } => "petri-not-sound",
            Self::InvalidChoiceGraph { .. } => "powl-invalid-choice-graph",
            Self::OrderNotIrreflexive { .. } => "powl-order-irreflexive",
            Self::OrderIndexOutOfRange { .. } => "powl-order-range",
            Self::OrderNotAntisymmetric { .. } => "powl-order-antisymmetric",
            Self::LanguageBudgetExceeded { .. } => "powl-language-budget",
            Self::UnknownTask { .. } => "htn-unknown-task",
            Self::DependencyCycle { .. } => "htn-dependency-cycle",
            Self::UndeclaredStates { .. } => "fond-undeclared-states",
            Self::UnknownPolicyStates { .. } => "fond-unknown-policy-states",
            Self::MissingPolicyAction { .. } => "fond-missing-policy-action",
            Self::UnavailablePolicyAction { .. } => "fond-unavailable-action",
            Self::NotStrong { .. } => "fond-not-strong",
            Self::NotStrongCyclic { .. } => "fond-not-strong-cyclic",
            Self::Unsolvable { .. } => "fond-unsolvable",
            Self::HorizonExceeded { .. } => "fond-horizon-exceeded",
            Self::AmbiguityBudgetExceeded { .. } => "shuffle-budget",
            Self::AlignmentBudgetExceeded { .. } => "alignment-budget",
            Self::NoAlignment => "alignment-none",
            Self::MarkingLengthMismatch { .. } => "petri-marking-length",
            Self::MissingField { .. } => "process-missing-field",
            Self::BatchRefused { .. } => "process-batch-refused",
        }
    }
}

impl StructuredError for ProcessError {
    error_visit! {
        Json(_json) => "process-json" {},
        Xsd(_xsd) => "process-xsd" {},
        MissingEvents => "ocel-missing-events" {},
        MissingObjects => "ocel-missing-objects" {},
        UnknownObjectReference { subject, object } => "ocel-unknown-object" {
            "subject" => Text(subject),
            "object" => Text(object),
        },
        InvalidTimestamp { lexical } => "ocel-invalid-timestamp" { "lexical" => Text(lexical) },
        DuplicateIdentity { kind, id } => "ocel-duplicate-identity" {
            "kind" => Code(kind),
            "id" => Text(id),
        },
        UndeclaredObjectType { type_name } => "ocel-undeclared-object-type" {
            "type_name" => Text(type_name),
        },
        UndeclaredEventType { type_name } => "ocel-undeclared-event-type" {
            "type_name" => Text(type_name),
        },
        UnsupportedAttributeShape { context } => "ocel-attribute-shape" {
            "context" => Text(context),
        },
        InvalidRelationship { context } => "ocel-invalid-relationship" {
            "context" => Text(context),
        },
        InputTooLarge { bytes, limit } => "ocel-input-too-large" {
            "bytes" => Unsigned(u128::from(*bytes)),
            "limit" => Unsigned((*limit) as u128),
        },
        MissingChainAttribute { index } => "ocel-chain-missing-attribute" {
            "index" => Unsigned((*index) as u128),
        },
        ChainBreak { index, detail } => "ocel-chain-break" {
            "index" => Unsigned((*index) as u128),
            "detail" => Code(detail),
        },
        InvalidWorkflowNet { reason } => "petri-invalid-structure" { "reason" => Code(reason) },
        NetNotSound { reasons } => "petri-not-sound" {
            "reasons" => List(reasons.iter().map(|r| ErrorValue::Code(r))),
        },
        InvalidChoiceGraph { reason } => "powl-invalid-choice-graph" { "reason" => Code(reason) },
        OrderNotIrreflexive { index } => "powl-order-irreflexive" {
            "index" => Unsigned((*index) as u128),
        },
        OrderIndexOutOfRange { pair, len } => "powl-order-range" {
            "pair" => List([(pair.0) as u128, (pair.1) as u128].into_iter().map(ErrorValue::Unsigned)),
            "len" => Unsigned((*len) as u128),
        },
        OrderNotAntisymmetric { forward, backward } => "powl-order-antisymmetric" {
            "forward" => List([(forward.0) as u128, (forward.1) as u128].into_iter().map(ErrorValue::Unsigned)),
            "backward" => List([(backward.0) as u128, (backward.1) as u128].into_iter().map(ErrorValue::Unsigned)),
        },
        LanguageBudgetExceeded { limit } => "powl-language-budget" {
            "limit" => Unsigned((*limit) as u128),
        },
        UnknownTask { reference } => "htn-unknown-task" { "reference" => Text(reference) },
        DependencyCycle { cycle } => "htn-dependency-cycle" {
            "cycle" => List(cycle.iter().map(|s| ErrorValue::Text(s))),
        },
        UndeclaredStates { states } => "fond-undeclared-states" {
            "states" => List(states.iter().map(|s| ErrorValue::Text(s))),
        },
        UnknownPolicyStates { states } => "fond-unknown-policy-states" {
            "states" => List(states.iter().map(|s| ErrorValue::Text(s))),
        },
        MissingPolicyAction { state } => "fond-missing-policy-action" { "state" => Text(state) },
        UnavailablePolicyAction { state, action, available } => "fond-unavailable-action" {
            "state" => Text(state),
            "action" => Text(action),
            "available" => List(available.iter().map(|s| ErrorValue::Text(s))),
        },
        NotStrong { losing_states } => "fond-not-strong" {
            "losing_states" => List(losing_states.iter().map(|s| ErrorValue::Text(s))),
        },
        NotStrongCyclic { states_without_goal_path } => "fond-not-strong-cyclic" {
            "states_without_goal_path" => List(states_without_goal_path.iter().map(|s| ErrorValue::Text(s))),
        },
        Unsolvable { mode, witness_states } => "fond-unsolvable" {
            "mode" => Code(mode.code()),
            "witness_states" => List(witness_states.iter().map(|s| ErrorValue::Text(s))),
        },
        HorizonExceeded { horizon, attempts } => "fond-horizon-exceeded" {
            "horizon" => Unsigned((*horizon) as u128),
            "attempts" => List((0..attempts.len()).map(|i| ErrorValue::Record(&attempts[i]))),
        },
        AmbiguityBudgetExceeded { budget } => "shuffle-budget" {
            "budget" => Unsigned(u128::from(*budget)),
        },
        AlignmentBudgetExceeded { iterations } => "alignment-budget" {
            "iterations" => Unsigned(u128::from(*iterations)),
        },
        NoAlignment => "alignment-none" {},
        MarkingLengthMismatch { got, expected } => "petri-marking-length" {
            "got" => Unsigned((*got) as u128),
            "expected" => Unsigned((*expected) as u128),
        },
        MissingField { document, field } => "process-missing-field" {
            "document" => Text(document),
            "field" => Code(field),
        },
        BatchRefused { table, detail } => "process-batch-refused" {
            "table" => Code(table),
            "detail" => Message(detail),
        },
    }
}

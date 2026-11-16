// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Chicago-style courts over the process crate's public surface: inline
//! fixtures, hand-derived final states, exact refusals. Every expected value
//! is derived from the donors' documented formulas and semantics, never from
//! observing the implementation's output.

use std::collections::{BTreeMap, BTreeSet};

use purrdf_lex::json::{Number, Value};
use purrdf_process::ProcessError;
use purrdf_process::conformance::alignment::{AlignmentCosts, Move, align};
use purrdf_process::conformance::replay::{BitmaskNet, Deviation, replay_trace};
use purrdf_process::conformance::shuffle::{ShuffleSpec, ShuffleVerdict};
use purrdf_process::log::integrity::{stamp_pi_chain, verify_pi_chain};
use purrdf_process::log::model::{AttrValue, ObjectAttribute, ValueAtTime};
use purrdf_process::log::{from_json_str, from_ndjson_str, to_json_string, validate};
use purrdf_process::model::decompose::decompose;
use purrdf_process::model::petri::PetriNet;
use purrdf_process::model::powl::{Endpoint, Limits, Powl};
use purrdf_process::synth::fond::{CounterexampleClass, FondDomain, Mode, classify, policy_switch};
use purrdf_process::synth::htn::{
    HtnMethod, HtnNetwork, HtnTaskDef, HtnWorkflow, workflow_to_powl,
};
use purrdf_process::synth::receipt::{ExecutionReceipt, RunStatus, Standing};

/// The OCEL 2.0 fixture: two declared types per family (validation enforces
/// declared-type coverage over every document), one change series on `o1`,
/// one omap-cited event and one relationship-cited event, one top-level O2O
/// edge.
const OCEL_JSON: &str = r#"{
  "objectTypes": [
    {"name": "order", "attributes": [{"name": "total", "attribute_type": "string"}]},
    {"name": "payment", "attributes": []}
  ],
  "eventTypes": [
    {"name": "create order", "attributes": []},
    {"name": "ship order", "attributes": []}
  ],
  "objects": [
    {
      "id": "o1",
      "type": "order",
      "attributes": [
        {"name": "total", "value": "42", "time": "2026-01-01T00:00:00Z"},
        {"name": "total", "value": "45", "time": "2026-01-02T00:00:00Z"}
      ]
    },
    {"id": "o2", "type": "payment"}
  ],
  "events": [
    {
      "id": "e1",
      "type": "create order",
      "time": "2026-01-01T00:00:00Z",
      "omap": ["o1"],
      "attributes": {"amount": 42}
    },
    {
      "id": "e2",
      "type": "ship order",
      "time": "2026-01-02T00:00:00Z",
      "relationships": [{"objectId": "o1", "qualifier": "ships"}]
    }
  ],
  "o2o": [{"sourceId": "o1", "targetId": "o2"}]
}"#;

/// Lexical instants used across the fixtures (valid xsd:dateTime).
const T0: &str = "2026-01-01T00:00:00Z";
const T1: &str = "2026-01-02T00:00:00Z";

/// Expected words builder: `&[&["a", "b"], &["c"]]` into the sorted word
/// vectors [`Powl::language`] returns.
fn words(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|label| (*label).to_owned()).collect())
        .collect()
}

/// A text attribute value.
fn text(value: &str) -> AttrValue {
    AttrValue::Text(value.to_owned())
}

/// The sound three-transition sequence net `p0 -a-> p1 -b-> p2 -c-> p3`
/// with one initial and one final marking.
fn sequence_net() -> PetriNet {
    let mut net = PetriNet::new();
    let p0 = net.add_place("p0");
    let p1 = net.add_place("p1");
    let p2 = net.add_place("p2");
    let p3 = net.add_place("p3");
    net.add_transition(Some("a".to_owned()), &[(p0, 1)], &[(p1, 1)])
        .unwrap();
    net.add_transition(Some("b".to_owned()), &[(p1, 1)], &[(p2, 1)])
        .unwrap();
    net.add_transition(Some("c".to_owned()), &[(p2, 1)], &[(p3, 1)])
        .unwrap();
    net.set_initial(vec![1, 0, 0, 0]).unwrap();
    net.add_final_marking(vec![0, 0, 0, 1]).unwrap();
    net
}

/// The FOND transition map from `(state, action, outcomes)` triples.
fn fond_transitions(
    entries: &[(&str, &str, &[&str])],
) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let mut transitions: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for (state, action, outcomes) in entries {
        transitions.entry((*state).to_owned()).or_default().insert(
            (*action).to_owned(),
            outcomes
                .iter()
                .map(|outcome| (*outcome).to_owned())
                .collect(),
        );
    }
    transitions
}

#[test]
/// Parse, validate, and write the canonical fixture; assert the exact
/// in-memory shape, the writer's determinism, and the JSON round-trip.
fn ocel_parse_validate_writer() {
    let log = from_json_str(OCEL_JSON).unwrap();
    assert_eq!(log.events.len(), 2);
    assert_eq!(log.objects.len(), 2);
    assert_eq!(log.object_types.len(), 2);
    assert_eq!(log.event_types.len(), 2);
    assert_eq!(log.object_relationships.len(), 1);
    assert_eq!(log.object_relationships[0].source_id, "o1");
    assert_eq!(log.object_relationships[0].target_id, "o2");
    assert_eq!(log.object_relationships[0].qualifier, "");

    // o1: the pair list lands as a two-entry change series; the current
    // attribute map keeps the latest stamped value.
    let o1 = &log.objects[0];
    assert_eq!(o1.id, "o1");
    assert_eq!(o1.changes.len(), 2);
    assert_eq!(o1.attributes.get("total"), Some(&text("45")));
    assert_eq!(o1.changes[0].time, T0);
    assert_eq!(o1.changes[0].value, text("42"));
    assert_eq!(o1.changes[1].time, T1);
    assert_eq!(o1.changes[1].value, text("45"));

    // o2: no attributes key, so empty current map and no changes.
    assert_eq!(log.objects[1].id, "o2");
    assert!(log.objects[1].attributes.is_empty());
    assert!(log.objects[1].changes.is_empty());

    // e1: exact-number attributes keep their lexeme.
    let e1 = &log.events[0];
    assert_eq!(e1.id.as_deref(), Some("e1"));
    assert_eq!(e1.object_ids, vec!["o1".to_owned()]);
    assert!(e1.relationships.is_empty());
    assert_eq!(
        e1.attributes.get("amount"),
        Some(&AttrValue::Number(Number::from_lexeme("42").unwrap()))
    );

    // e2: the qualified relationship survives ingestion, qualifier intact.
    let e2 = &log.events[1];
    assert_eq!(e2.relationships.len(), 1);
    assert_eq!(e2.relationships[0].object_id, "o1");
    assert_eq!(e2.relationships[0].qualifier, "ships");

    // Point-in-time resolution over a hand-built series: as-of T0 is 42, the
    // unqualified read is the latest entry, 45.
    let series = ObjectAttribute {
        name: "total".to_owned(),
        entries: vec![
            ValueAtTime {
                value: text("42"),
                time: Some(T0.to_owned()),
            },
            ValueAtTime {
                value: text("45"),
                time: Some(T1.to_owned()),
            },
        ],
    };
    assert_eq!(series.value_at(Some(T0)), Some(&text("42")));
    assert_eq!(series.value_at(Some(T1)), Some(&text("45")));
    assert_eq!(series.value_at(None), Some(&text("45")));

    // Writer determinism: two serializations are byte-identical, the omap
    // fallback carries ex4pm's "involved" qualifier, and the qualified edge
    // serializes in the canonical key order.
    let first = to_json_string(&log);
    let second = to_json_string(&log);
    assert_eq!(first, second);
    assert!(first.contains("\"qualifier\":\"involved\""));
    assert!(first.contains("\"relationships\":["));
    assert!(first.contains("\"objectId\":\"o1\""));

    validate(&log).expect("the fixture satisfies every validation rule");

    // Round trip: the canonical bytes reparse to two identified events.
    let reparsed = from_json_str(&to_json_string(&log)).unwrap();
    assert_eq!(reparsed.events.len(), 2);
    assert!(reparsed.events.iter().all(|event| event.id.is_some()));
    assert_eq!(reparsed.objects.len(), 2);
}

#[test]
/// Every OCEL refusal carries its variant, its field payload, and its stable
/// code; NDJSON streams ingest one event per line under the 1.0 dialect.
fn ocel_refusals_and_ndjson() {
    // An event citing an unresolvable object id.
    let log = from_json_str(
        r#"{"events": [{"id": "e1", "type": "t", "time": "2026-01-01T00:00:00Z", "omap": ["o99"]}],
            "objects": []}"#,
    )
    .unwrap();
    let error = validate(&log).expect_err("dangling object references must be refused");
    assert!(
        matches!(&error, ProcessError::UnknownObjectReference { object, .. } if object == "o99")
    );
    assert_eq!(error.code(), "ocel-unknown-object");

    // A lexical timestamp outside xsd:dateTime.
    let log = from_json_str(
        r#"{"events": [{"id": "e1", "type": "t", "time": "not-a-date"}], "objects": []}"#,
    )
    .unwrap();
    let error = validate(&log).expect_err("non-xsd timestamps must be refused");
    assert!(
        matches!(&error, ProcessError::InvalidTimestamp { lexical } if lexical == "not-a-date")
    );
    assert_eq!(error.code(), "ocel-invalid-timestamp");

    // Two objects claiming one identity.
    let log = from_json_str(
        r#"{"events": [],
            "objects": [{"id": "o1", "type": "t"}, {"id": "o1", "type": "t"}]}"#,
    )
    .unwrap();
    let error = validate(&log).expect_err("duplicate object ids must be refused");
    assert!(
        matches!(&error, ProcessError::DuplicateIdentity { kind, id } if *kind == "object" && id == "o1")
    );
    assert_eq!(error.code(), "ocel-duplicate-identity");

    // NDJSON stream: one `ocel:*` event per line, document order preserved.
    let streamed = from_ndjson_str(
        r#"{"ocel:eid": "x1", "ocel:activity": "A", "ocel:timestamp": "2026-01-01T00:00:00Z"}
{"ocel:eid": "x2", "ocel:activity": "B", "ocel:timestamp": "2026-01-01T00:00:00Z"}"#,
    )
    .unwrap();
    assert_eq!(streamed.events.len(), 2);
    assert_eq!(streamed.events[0].id.as_deref(), Some("x1"));
    assert_eq!(streamed.events[0].event_type, "A");
    assert_eq!(streamed.events[1].id.as_deref(), Some("x2"));
    assert_eq!(streamed.events[1].event_type, "B");
    validate(&streamed).expect("a two-event stream with no references validates");
}

#[test]
/// Activities, the silent step, sequences, and exclusive choices yield their
/// documented bounded languages.
fn powl_language_basics() {
    let limits = Limits::default();
    let language = |node: &Powl| node.language(&limits).unwrap();

    assert_eq!(language(&Powl::activity("a", "a")), words(&[&["a"]]));
    assert_eq!(language(&Powl::silent("tau")), words(&[&[]]));
    let sequence = Powl::Sequence {
        id: "s".to_owned(),
        children: vec![Powl::activity("a", "a"), Powl::activity("b", "b")],
    };
    assert_eq!(language(&sequence), words(&[&["a", "b"]]));
    let xor = Powl::Xor {
        id: "x".to_owned(),
        children: vec![Powl::activity("a", "a"), Powl::activity("b", "b")],
    };
    assert_eq!(language(&xor), words(&[&["a"], &["b"]]));
}

#[test]
/// Loop unrolling, partial-order shuffles under a poset, choice-graph path
/// enumeration, and the loud language-budget refusal all hold exactly.
fn powl_language_loop_order_choice_budget() {
    // Loop with max_unroll 1: the body alone, then body·redo·body.
    let loop_once = Powl::Loop {
        id: "l".to_owned(),
        do_branch: Box::new(Powl::activity("a", "a")),
        redo_branch: Box::new(Powl::activity("b", "b")),
    };
    let limits = Limits {
        max_unroll: 1,
        max_language: 65_536,
        max_paths: 1_024,
    };
    assert_eq!(
        loop_once.language(&limits).unwrap(),
        words(&[&["a"], &["a", "b", "a"]])
    );

    // Partial order, unconstrained: both order-preserving interleavings.
    let concurrent = Powl::partial_order(
        "po".to_owned(),
        vec![Powl::activity("a", "a"), Powl::activity("c", "c")],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        concurrent.language(&Limits::default()).unwrap(),
        words(&[&["a", "c"], &["c", "a"]])
    );

    // Partial order, edge (0 -> 1): exactly the ordered interleaving.
    let ordered = Powl::partial_order(
        "po".to_owned(),
        vec![Powl::activity("a", "a"), Powl::activity("c", "c")],
        vec![(0, 1)],
    )
    .unwrap();
    assert_eq!(
        ordered.language(&Limits::default()).unwrap(),
        words(&[&["a", "c"]])
    );

    // Choice graph: two parallel branches between the delimiters.
    let choice = Powl::choice_graph(
        "cg".to_owned(),
        vec![Powl::activity("a", "a"), Powl::activity("b", "b")],
        vec![
            (Endpoint::Start, Endpoint::Child(0)),
            (Endpoint::Start, Endpoint::Child(1)),
            (Endpoint::Child(0), Endpoint::End),
            (Endpoint::Child(1), Endpoint::End),
        ],
    )
    .unwrap();
    assert_eq!(
        choice.language(&Limits::default()).unwrap(),
        words(&[&["a"], &["b"]])
    );

    // A loop unrolled five deep over a three-string ceiling refuses loudly.
    let error = loop_once
        .language(&Limits {
            max_unroll: 4,
            max_language: 3,
            max_paths: 1_024,
        })
        .expect_err("the product ceiling must refuse, not truncate");
    assert!(matches!(&error, ProcessError::LanguageBudgetExceeded { limit } if *limit == 3));
    assert_eq!(error.code(), "powl-language-budget");
}

#[test]
/// HTN networks validate their orderings, keep exact edges on the POWL node,
/// close them only for the digest, and refuse reflexive orderings.
fn htn_network_courts() {
    let network = HtnNetwork::new(vec!["a".to_owned(), "b".to_owned()], vec![(0, 1)]).unwrap();
    assert!(matches!(network.to_powl(), Powl::PartialOrder { .. }));
    assert!(network.closed_ordering().contains(&(0, 1)));
    let digest = network.plan_digest();
    assert!(digest.starts_with("hddl-plan-"));
    assert_eq!(digest.len(), "hddl-plan-".len() + 16);
    let rebuilt = HtnNetwork::new(vec!["a".to_owned(), "b".to_owned()], vec![(0, 1)]).unwrap();
    assert_eq!(rebuilt.plan_digest(), digest);

    let error =
        HtnNetwork::new(vec!["a".to_owned()], vec![(0, 0)]).expect_err("(0, 0) is not irreflexive");
    assert!(matches!(&error, ProcessError::OrderNotIrreflexive { index } if *index == 0));
    assert_eq!(error.code(), "powl-order-irreflexive");
}

#[test]
/// Workflows project their `after` graph to an exact partial order and
/// refuse cycles and dangling method references with their stable codes.
fn htn_workflow_courts() {
    let tasks = vec![
        HtnTaskDef {
            id: "t1".to_owned(),
            capability: "cap.one".to_owned(),
            after: Vec::new(),
            outcomes: vec!["ok".to_owned()],
        },
        HtnTaskDef {
            id: "t2".to_owned(),
            capability: "cap.two".to_owned(),
            after: vec!["t1".to_owned()],
            outcomes: vec!["ok".to_owned()],
        },
    ];
    let methods = vec![HtnMethod {
        id: "m1".to_owned(),
        task: "t2".to_owned(),
        subtasks: vec!["t1".to_owned()],
        precondition: None,
    }];
    let workflow = HtnWorkflow::new("w", None, tasks, methods).unwrap();
    let projection = workflow_to_powl(&workflow).unwrap();
    match projection {
        Powl::PartialOrder {
            children, order, ..
        } => {
            assert_eq!(children.len(), 2);
            assert_eq!(order, vec![(0, 1)]);
        }
        other => panic!("expected a partial order, got {other:?}"),
    }

    // A two-task cycle.
    let cyclic = vec![
        HtnTaskDef {
            id: "t1".to_owned(),
            capability: "cap.one".to_owned(),
            after: vec!["t2".to_owned()],
            outcomes: Vec::new(),
        },
        HtnTaskDef {
            id: "t2".to_owned(),
            capability: "cap.two".to_owned(),
            after: vec!["t1".to_owned()],
            outcomes: Vec::new(),
        },
    ];
    let error = HtnWorkflow::new("cyclic", None, cyclic, Vec::new())
        .expect_err("a dependency cycle must be refused");
    assert!(matches!(&error, ProcessError::DependencyCycle { .. }));
    assert_eq!(error.code(), "htn-dependency-cycle");

    // A method naming an undeclared abstract task.
    let error = HtnWorkflow::new(
        "ghost",
        None,
        vec![HtnTaskDef {
            id: "t1".to_owned(),
            capability: "cap.one".to_owned(),
            after: Vec::new(),
            outcomes: Vec::new(),
        }],
        vec![HtnMethod {
            id: "m1".to_owned(),
            task: "ghost".to_owned(),
            subtasks: Vec::new(),
            precondition: None,
        }],
    )
    .expect_err("a method over an unknown task must be refused");
    assert!(matches!(&error, ProcessError::UnknownTask { reference } if reference == "ghost"));
    assert_eq!(error.code(), "htn-unknown-task");
}

#[test]
/// On the two-step chain, a complete strong policy validates, synthesis
/// recovers exactly the minimal policy, and a missing entry names its state.
fn fond_chain_validate_synthesize() {
    let domain = FondDomain::new(
        ["s0", "s1", "g"].into_iter().map(String::from),
        "s0",
        ["g"].into_iter().map(String::from),
        fond_transitions(&[("s0", "a", &["s1"]), ("s1", "b", &["g"])]),
    )
    .unwrap();

    let complete: BTreeMap<String, String> = [("s0", "a"), ("s1", "b")]
        .into_iter()
        .map(|(s, a)| (s.to_owned(), a.to_owned()))
        .collect();
    let reachable = domain.validate_policy(&complete, Mode::Strong).unwrap();
    assert_eq!(
        reachable,
        ["s0", "s1", "g"].into_iter().map(String::from).collect()
    );

    let synthesized = domain.synthesize(Mode::Strong).unwrap();
    assert_eq!(synthesized, complete);

    let error = domain
        .validate_policy(
            &[("s0", "a")]
                .into_iter()
                .map(|(s, a)| (s.to_owned(), a.to_owned()))
                .collect(),
            Mode::Strong,
        )
        .expect_err("a policy without an entry for s1 must be refused");
    assert!(matches!(&error, ProcessError::MissingPolicyAction { state } if state == "s1"));
    assert_eq!(error.code(), "fond-missing-policy-action");
}

#[test]
/// A policy whose only cycle never reaches the goal is not strong: both
/// reachable states lose, in sorted order.
fn fond_not_strong_looping_policy() {
    let domain = FondDomain::new(
        ["s0", "s1", "g"].into_iter().map(String::from),
        "s0",
        ["g"].into_iter().map(String::from),
        fond_transitions(&[("s0", "a", &["s1"]), ("s1", "b", &["s1"])]),
    )
    .unwrap();
    let looping: BTreeMap<String, String> = [("s0", "a"), ("s1", "b")]
        .into_iter()
        .map(|(s, a)| (s.to_owned(), a.to_owned()))
        .collect();
    let error = domain
        .validate_policy(&looping, Mode::Strong)
        .expect_err("a policy trapped off the goal is not strong");
    assert!(
        matches!(&error, ProcessError::NotStrong { losing_states } if *losing_states == vec!["s0".to_owned(), "s1".to_owned()])
    );
    assert_eq!(error.code(), "fond-not-strong");
}

#[test]
/// An unreachable goal is unsolvable with an initial-containing witness, the
/// policy switch refuses across modes, and the counterexample classes map
/// their codes exactly.
fn fond_unsolvable_switch_and_classes() {
    let stuck = FondDomain::new(
        ["s0", "g"].into_iter().map(String::from),
        "s0",
        ["g"].into_iter().map(String::from),
        fond_transitions(&[("s0", "a", &["s0"])]),
    )
    .unwrap();
    assert_eq!(stuck.witness_states(), vec!["s0".to_owned()]);
    let error = stuck
        .synthesize(Mode::Strong)
        .expect_err("a goal outside the winning region is unsolvable");
    match error {
        ProcessError::Unsolvable {
            mode,
            witness_states,
        } => {
            assert_eq!(mode, Mode::Strong);
            assert_eq!(witness_states, vec!["s0".to_owned()]);
        }
        other => panic!("expected Unsolvable, got {other:?}"),
    }

    let error = policy_switch(&stuck, &[Mode::Strong, Mode::StrongCyclic], 9)
        .expect_err("no mode solves a stuck domain");
    match error {
        ProcessError::Unsolvable {
            mode,
            witness_states,
        } => {
            assert_eq!(mode, Mode::StrongCyclic);
            assert_eq!(witness_states, vec!["s0".to_owned()]);
        }
        other => panic!("expected Unsolvable, got {other:?}"),
    }

    assert_eq!(classify("fond-not-strong"), CounterexampleClass::Liveness);
    assert_eq!(
        classify("fond-unavailable-action"),
        CounterexampleClass::Deadlock
    );
    assert_eq!(
        classify("fond-unknown-policy-states"),
        CounterexampleClass::Identity
    );
    assert_eq!(
        classify("process-missing-field"),
        CounterexampleClass::Shape
    );
}

#[test]
/// The sequence net is a sound workflow net with source 0 and sink 3, and
/// every soundness property reports true.
fn petri_sequence_net_soundness() {
    let net = sequence_net();
    let facts = net.validate_workflow_net().unwrap();
    assert_eq!(facts.source, 0);
    assert_eq!(facts.sink, 3);

    let report = net.verify_soundness(10_000);
    assert!(report.bounded);
    assert!(report.one_safe);
    assert!(report.final_reached);
    assert!(report.one_final_marking);
    assert!(report.no_dead_transitions);
    assert!(report.option_to_complete);
    assert!(report.deadlock_free);
    assert!(report.sound());
    assert!(report.failures().is_empty());
}

#[test]
/// A transition weighted past the reachable token count never fires: the
/// soundness report names `no_dead_transitions` and nothing else.
fn petri_dead_transition_court() {
    let mut net = PetriNet::new();
    let p0 = net.add_place("p0");
    let p1 = net.add_place("p1");
    let p2 = net.add_place("p2");
    let p3 = net.add_place("p3");
    net.add_transition(Some("a".to_owned()), &[(p0, 1)], &[(p1, 1)])
        .unwrap();
    net.add_transition(Some("b".to_owned()), &[(p1, 1)], &[(p2, 1)])
        .unwrap();
    net.add_transition(Some("c".to_owned()), &[(p2, 1)], &[(p3, 1)])
        .unwrap();
    // `x` needs two tokens on p0; the sequence net never marks a place twice.
    net.add_transition(Some("x".to_owned()), &[(p0, 2)], &[(p3, 1)])
        .unwrap();
    net.set_initial(vec![1, 0, 0, 0]).unwrap();
    net.add_final_marking(vec![0, 0, 0, 1]).unwrap();

    let report = net.verify_soundness(10_000);
    assert!(!report.no_dead_transitions);
    assert!(!report.sound());
    assert_eq!(report.failures(), vec!["no_dead_transitions"]);
}

#[test]
/// Decomposition: a single transition is its activity, a marked graph
/// projects its place-wise precedence, and a state machine wires its choice
/// graph through the delimiters.
fn decompose_courts() {
    // One transition.
    let mut single = PetriNet::new();
    let p0 = single.add_place("p0");
    let p1 = single.add_place("p1");
    single
        .add_transition(Some("a".to_owned()), &[(p0, 1)], &[(p1, 1)])
        .unwrap();
    match decompose(&single).unwrap() {
        Powl::Activity { label, .. } => assert_eq!(label, "a"),
        other => panic!("expected an activity, got {other:?}"),
    }

    // Marked graph: p0 -a-> p1 -b-> p2 gives precedence a before b.
    let mut marked = PetriNet::new();
    let m0 = marked.add_place("p0");
    let m1 = marked.add_place("p1");
    let m2 = marked.add_place("p2");
    marked
        .add_transition(Some("a".to_owned()), &[(m0, 1)], &[(m1, 1)])
        .unwrap();
    marked
        .add_transition(Some("b".to_owned()), &[(m1, 1)], &[(m2, 1)])
        .unwrap();
    match decompose(&marked).unwrap() {
        Powl::PartialOrder { order, .. } => assert_eq!(order, vec![(0, 1)]),
        other => panic!("expected a partial order, got {other:?}"),
    }

    // State machine: one choice at p1, rejoined at p3.
    let mut machine = PetriNet::new();
    let s0 = machine.add_place("p0");
    let s1 = machine.add_place("p1");
    let s2 = machine.add_place("p2");
    let s3 = machine.add_place("p3");
    machine
        .add_transition(Some("a".to_owned()), &[(s0, 1)], &[(s1, 1)])
        .unwrap();
    machine
        .add_transition(Some("b".to_owned()), &[(s1, 1)], &[(s3, 1)])
        .unwrap();
    machine
        .add_transition(Some("c".to_owned()), &[(s1, 1)], &[(s2, 1)])
        .unwrap();
    machine
        .add_transition(Some("d".to_owned()), &[(s2, 1)], &[(s3, 1)])
        .unwrap();
    match decompose(&machine).unwrap() {
        Powl::ChoiceGraph { edges, .. } => {
            let expected: BTreeSet<(Endpoint, Endpoint)> = [
                (Endpoint::Start, Endpoint::Child(0)),
                (Endpoint::Child(0), Endpoint::Child(1)),
                (Endpoint::Child(0), Endpoint::Child(2)),
                (Endpoint::Child(2), Endpoint::Child(3)),
                (Endpoint::Child(1), Endpoint::End),
                (Endpoint::Child(3), Endpoint::End),
            ]
            .into_iter()
            .collect();
            assert_eq!(edges.into_iter().collect::<BTreeSet<_>>(), expected);
        }
        other => panic!("expected a choice graph, got {other:?}"),
    }
}

#[test]
/// Token replay fitness follows the van der Aalst formula over art72
/// accounting: perfect fitness on the fit trace, 2/3 on the truncated trace,
/// 1/4 under an unknown label; the bitmask path agrees exactly.
fn replay_fitness_courts() {
    let net = sequence_net();

    let fitting = replay_trace(&net, &["a", "b", "c"]);
    assert_eq!(fitting.consumed, 3);
    assert_eq!(fitting.produced, 3);
    assert_eq!(fitting.missing, 0);
    assert_eq!(fitting.remaining, 0);
    assert!(fitting.final_reached);
    assert!(fitting.deviations.is_empty());
    assert!((fitting.fitness - 1.0).abs() < 1e-9);

    // Truncated at b: art72 accounting counts the end deficit into consumed
    // and the end excess into produced (c=p=3, m=r=1) — 0.5*(1-1/3) * 2 = 2/3.
    let truncated = replay_trace(&net, &["a", "b"]);
    assert_eq!(truncated.consumed, 3);
    assert_eq!(truncated.produced, 3);
    assert_eq!(truncated.missing, 1);
    assert_eq!(truncated.remaining, 1);
    assert!(!truncated.final_reached);
    assert!((truncated.fitness - 2.0 / 3.0).abs() < 1e-9);

    // Unknown label: wasm4pm's convention — the unknown label records a
    // missing token without consuming; a's tokens plus the end
    // deficit/excess land as c=2, p=2, m=2, r=1 — 0.5*(1-2/2) + 0.5*(1-1/2)
    // = 1/4.
    let unknown = replay_trace(&net, &["a", "x"]);
    assert_eq!(unknown.missing, 2);
    assert_eq!(unknown.consumed, 2);
    assert_eq!(unknown.produced, 2);
    assert_eq!(unknown.remaining, 1);
    assert!((unknown.fitness - 0.25).abs() < 1e-9);
    assert_eq!(
        unknown.deviations,
        vec![Deviation::UnknownLabel("x".to_owned())]
    );

    // Bitmask parity: identical results on both paths.
    let bitmask = BitmaskNet::try_from_net(&net).expect("the sequence net admits the u64 path");
    assert_eq!(bitmask.replay(&["a", "b", "c"]), fitting);
    assert_eq!(bitmask.replay(&["a", "b"]), truncated);
}

#[test]
/// A-star alignment is deterministic and exact: zero-cost synchrony on the
/// fit trace, cost 2 with the documented move sequence on the deviant trace.
fn alignment_courts() {
    let net = sequence_net();
    let costs = AlignmentCosts::default();

    let perfect = align(&net, &["a", "b", "c"], &costs, 100_000).unwrap();
    assert_eq!(perfect.cost, 0);
    assert_eq!(
        perfect.moves,
        vec![
            Move::Sync("a".to_owned()),
            Move::Sync("b".to_owned()),
            Move::Sync("c".to_owned())
        ]
    );
    assert_eq!(perfect.syncs, 3);
    assert_eq!(perfect.log_moves, 0);
    assert_eq!(perfect.model_moves, 0);
    assert!((perfect.fitness - 1.0).abs() < 1e-9);

    // After sync a and sync b the marking is {p2}: the optimal alignment
    // takes the log move for x, then model-fires c to reach the final
    // marking. Cost 1 + 1, ties broken by insertion sequence.
    let deviant = align(&net, &["a", "b", "x"], &costs, 100_000).unwrap();
    assert_eq!(deviant.cost, 2);
    assert_eq!(
        deviant.moves,
        vec![
            Move::Sync("a".to_owned()),
            Move::Sync("b".to_owned()),
            Move::Log("x".to_owned()),
            Move::Model(Some("c".to_owned()))
        ]
    );
    assert_eq!(deviant.syncs, 2);
    assert_eq!(deviant.log_moves, 1);
    assert_eq!(deviant.model_moves, 1);
    // ex4pm's derivation: 1 - 2 / (3 + max(1, 1.5)) = 5/9.
    assert!((deviant.fitness - 5.0 / 9.0).abs() < 1e-9);
}

#[test]
/// Shuffle membership respects the precedence gate, rejects at the first
/// ineligible position, and budgets its ambiguity backtracking.
fn shuffle_membership_courts() {
    let spec = ShuffleSpec::new(
        vec![vec!["a".to_owned(), "b".to_owned()], vec!["c".to_owned()]],
        vec![(0, 1)],
    )
    .unwrap();
    // Precedence (0 -> 1) means ALL of child 0 precedes child 1's head:
    // [a, b, c] is the only interleaving, and [a, c, b] dies at position 1
    // (child 0 still owes its "b" when "c" arrives).
    match spec.accepts(&["a", "c", "b"], 1_000_000).unwrap() {
        ShuffleVerdict::Rejected {
            position, label, ..
        } => {
            assert_eq!(position, 1);
            assert_eq!(label, "c");
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
    assert_eq!(
        spec.accepts(&["a", "b", "c"], 1_000_000).unwrap(),
        ShuffleVerdict::Accepted
    );
    match spec.accepts(&["c", "a", "b"], 1_000_000).unwrap() {
        ShuffleVerdict::Rejected {
            position,
            label,
            reason,
        } => {
            assert_eq!(position, 0);
            assert_eq!(label, "c");
            assert_eq!(reason, "no eligible child carries this label");
        }
        other => panic!("expected a rejection, got {other:?}"),
    }

    // Two identical children: every position is ambiguous.
    let ambiguous =
        ShuffleSpec::new(vec![vec!["a".to_owned()], vec!["a".to_owned()]], Vec::new()).unwrap();
    assert_eq!(
        ambiguous.accepts(&["a", "a"], 1_000_000).unwrap(),
        ShuffleVerdict::Accepted
    );
    let error = ambiguous
        .accepts(&["a", "a"], 1)
        .expect_err("a one-node budget cannot resolve an ambiguous position");
    assert!(matches!(&error, ProcessError::AmbiguityBudgetExceeded { budget } if *budget == 1));
}

#[test]
/// Receipts digest their canonical outcome to 16 stable hex chars and the
/// standing ladder moves forward only.
fn receipt_and_standing_courts() {
    let mint = |corresponds: Vec<String>| {
        ExecutionReceipt::mint(
            "plan-1",
            "run-1",
            RunStatus::Succeeded,
            42,
            &Value::Null,
            "subj",
            "base",
            corresponds,
        )
    };
    let receipt = mint(Vec::new());
    assert_eq!(receipt.outcome_digest.len(), 16);
    assert_eq!(receipt.plan_id, "plan-1");
    assert_eq!(receipt.run_id, "run-1");
    assert_eq!(mint(Vec::new()).outcome_digest, receipt.outcome_digest);
    assert_ne!(
        mint(vec!["step-7".to_owned()]).outcome_digest,
        receipt.outcome_digest
    );

    assert!(Standing::Unknown.can_transition_to(Standing::Verified));
    assert!(!Standing::Verified.can_transition_to(Standing::Unknown));
    assert!(Standing::Verified.can_transition_to(Standing::Verified));
}

#[test]
/// The integrity chain stamps, verifies, and detects a tampered event at its
/// canonical index.
fn integrity_chain_courts() {
    let mut stamped = from_json_str(OCEL_JSON).unwrap();
    let head = stamp_pi_chain(&mut stamped);
    assert!(!head.is_empty());
    assert_eq!(
        stamped.events.last().unwrap().attributes.get("pi_hash"),
        Some(&AttrValue::Text(head.clone()))
    );
    verify_pi_chain(&stamped).expect("a freshly stamped chain verifies");

    stamped.events[1].event_type = "tampered".to_owned();
    let error = verify_pi_chain(&stamped).expect_err("a tampered event must break the chain");
    match error {
        ProcessError::ChainBreak { index, detail } => {
            assert_eq!(index, 1);
            assert_eq!(detail, "pi_hash mismatch");
        }
        other => panic!("expected a chain break, got {other:?}"),
    }
}

/// The cgap git-rev pin and the substrate's ontology-driven version must
/// move together: a pin bump without an upstream release (or the reverse)
/// fails this court loudly instead of silently drifting.
#[test]
fn cgap_pin_matches_substrate_version() {
    assert_eq!(cgap_arrow::VERSION, "26.10.29");
}

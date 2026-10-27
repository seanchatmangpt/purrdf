//! Proof round-trip courts for the native N3 reasoner (harness = false, so
//! the same binary also runs through the workspace wasm runner).
//!
//! Chicago discipline: the real parser, real forward reasoner, real proof
//! writer and the real proof checker all execute; assertions land on the
//! checker's report (final state), never on call counts.

use purrdf_n3::parser::parse_n3_with_source;
use purrdf_n3::proof_check::Report;
use purrdf_n3::proof_check_n3::check_proof;
use purrdf_n3::proof_writer::proof_to_n3;
use purrdf_n3::reasoner::{ReasonerOptions, reason};

const SOURCE: &str = "@prefix : <http://example.org/> .\n\
                      @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
                      :Socrates a :Human .\n\
                      :Human rdfs:subClassOf :Mortal .\n\
                      { ?S a ?A . ?A rdfs:subClassOf ?B . } => { ?S a ?B . } .\n";

/// Derive with the real reasoner (proofs on), then serialize via the real
/// proof writer — the exact path the checker will be pointed back at.
fn proof_text() -> String {
    let document = parse_n3_with_source(SOURCE, None, Some("test.n3")).expect("source parses");
    let result = reason(
        &document,
        &ReasonerOptions {
            proof: true,
            ..ReasonerOptions::default()
        },
    );
    assert!(
        result.is_complete(),
        "forward run must be complete, got {:?}",
        result.limits_reached
    );
    assert!(!result.proofs.is_empty(), "no proofs were recorded");
    proof_to_n3(&document.prefixes, &result)
}

fn checked(proof: &str) -> Report {
    check_proof(SOURCE, proof, "test.n3").expect("proof document parses")
}

type Outcome = Result<(), String>;

fn generated_proof_checks() -> Outcome {
    let proof = proof_text();
    let report = checked(&proof);
    if !report.valid() {
        return Err(format!("valid proof rejected: {:?}", report.failures));
    }
    if report.verified < 3 {
        return Err(format!(
            "expected at least 3 verified steps, got {}",
            report.verified
        ));
    }
    Ok(())
}

/// `proof_to_n3` emits N3, so the emitted text must parse back with the
/// ordinary N3 parser — no special proof grammar.
fn proof_serialization_parses_back() -> Outcome {
    let proof = proof_text();
    let document = parse_n3_with_source(&proof, None, Some("proof.n3"))
        .map_err(|e| format!("emitted proof does not re-parse as N3: {}", e))?;
    if document.facts.is_empty() {
        return Err("re-parsed proof document carries no triples".into());
    }
    Ok(())
}

/// A forged conclusion (the derived triple swapped for one the reasoner never
/// produced) must fail verification, and the failure must name the step.
fn tampered_conclusion_is_rejected() -> Outcome {
    let proof = proof_text().replace(":Socrates a :Mortal", ":Plato a :Mortal");
    if !proof.contains(":Plato a :Mortal") {
        return Err("tamper anchor `:Socrates a :Mortal` not found in emitted proof".into());
    }
    let report = checked(&proof);
    if report.valid() {
        return Err("tampered proof was admitted".into());
    }
    let rendered = report.verdict();
    if rendered.is_empty() {
        return Err("checker produced an empty verdict for a forged proof".into());
    }
    Ok(())
}

/// F1 (identity substrate): the proof writer dedups rule steps by a key
/// derived from the term's Debug rendering, which drops a formula's
/// existential binder set when no universals are declared. Two derivations
/// of facts that differ only inside such a formula (same predicate, objects
/// with distinct binder sets) are distinct terms the reasoner reports
/// separately — and one distinct step must not be swallowed.
fn dedup_keeps_binder_set_distinct_steps() -> Outcome {
    use purrdf_n3::ast::{Document, Graph, Name, Rule, Term, Triple};
    use std::collections::BTreeSet;

    const X: &str = "http://example.org/x";
    const P: &str = "http://example.org/p";
    const W: &str = "http://example.org/w";
    const HOLDS: &str = "http://example.org/holds";
    const SKOLEM: &str = "http://www.w3.org/2000/10/swap/log#skolem";

    let same_triples = || {
        vec![Triple::new(
            Term::Iri(Name::from(X)),
            Term::Iri(Name::from(P)),
            Term::Blank(Name::from("q")),
        )]
    };
    let f_a = Graph {
        universals: BTreeSet::new(),
        existentials: BTreeSet::new(),
        triples: same_triples(),
    };
    let f_b = Graph {
        universals: BTreeSet::new(),
        existentials: [Name::from("q")].into(),
        triples: same_triples(),
    };
    assert_ne!(
        Term::Formula(f_a.clone()),
        Term::Formula(f_b.clone()),
        "precondition: distinct terms"
    );
    assert_eq!(
        format!("{:?}", Term::Formula(f_a.clone())),
        format!("{:?}", Term::Formula(f_b.clone())),
        "precondition: Debug cannot tell the two facts apart"
    );

    let holds_conclusion = |formula: Graph| {
        vec![Triple::new(
            Term::Iri(Name::from(W)),
            Term::Iri(Name::from(HOLDS)),
            Term::Formula(formula),
        )]
    };
    let skolem_rule = |subject: Term, conclusion: Vec<Triple>| {
        Rule::new(
            Graph::from(vec![Triple::new(
                subject,
                Term::Iri(Name::from(SKOLEM)),
                Term::var("sk"),
            )]),
            conclusion,
            true,
        )
    };
    let mut doc = Document::new();
    doc.rules = vec![
        skolem_rule(Term::Formula(f_a.clone()), holds_conclusion(f_a)),
        skolem_rule(Term::Formula(f_b.clone()), holds_conclusion(f_b)),
    ];

    let result = reason(
        &doc,
        &ReasonerOptions {
            proof: true,
            ..ReasonerOptions::default()
        },
    );
    assert!(
        result.is_complete(),
        "run must complete: {:?}",
        result.incomplete_summary()
    );

    // The reasoner itself keeps both facts: the collision is purely in the
    // writer's key.
    let holds_count = result
        .closure
        .iter()
        .filter(|t| t.p == Term::Iri(Name::from(HOLDS)))
        .count();
    assert_eq!(
        holds_count, 2,
        "the reasoner derives two distinct facts: {:?}",
        result.closure
    );

    let proof = proof_to_n3(&doc.prefixes, &result);
    let rule_steps = proof.matches("pe:rule").count();
    assert_eq!(
        rule_steps, 2,
        "two distinct derivations must both appear as proof steps\nproof:\n{proof}"
    );
    Ok(())
}

fn main() {
    let cases: &[(&str, fn() -> Outcome)] = &[
        ("generated_proof_checks", generated_proof_checks),
        (
            "proof_serialization_parses_back",
            proof_serialization_parses_back,
        ),
        (
            "tampered_conclusion_is_rejected",
            tampered_conclusion_is_rejected,
        ),
        (
            "dedup_keeps_binder_set_distinct_steps",
            dedup_keeps_binder_set_distinct_steps,
        ),
    ];
    let mut passed = 0usize;
    let mut failed = 0usize;
    for (name, case) in cases {
        match case() {
            Ok(()) => {
                passed += 1;
                println!("PASS {}", name);
            }
            Err(reason) => {
                failed += 1;
                println!("FAIL {}: {}", name, reason);
            }
        }
    }
    println!("{} passed, {} failed (of {})", passed, failed, cases.len());
    if failed > 0 {
        std::process::exit(1);
    }
}

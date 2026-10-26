// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Chicago-school courts for the N3 forward reasoner.
//!
//! Real reasoner over real parsed N3 documents, asserting on final derived
//! triple sets — no mocks. Scenarios ported from graphlaw's
//! `tests/chicago_n3_datalog.rs` court: implication chaining, recursive
//! fixpoint (transitive closure), builtin families (math/string/list),
//! termination on cyclic data, and budget/limit behavior.

use std::collections::BTreeSet;

use purrdf_n3::ast::{Name, Term};
use purrdf_n3::parser::parse_n3;
use purrdf_n3::reasoner::{ReasonerOptions, ReasonerResult, ReasonerLimit, reason};

use purrdf_testkit::harness::{self, Trial};

const HEAD: &str = "\
@prefix : <http://example.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix log: <http://www.w3.org/2000/10/swap/log#> .
@prefix math: <http://www.w3.org/2000/10/swap/math#> .
@prefix string: <http://www.w3.org/2000/10/swap/string#> .
@prefix list: <http://www.w3.org/2000/10/swap/list#> .
";

/// Reason over a program; assert the run completed with no semantic errors.
fn reason_ok(program: &str) -> ReasonerResult {
    let doc = parse_n3(program, None).expect("program should parse");
    let result = reason(&doc, &ReasonerOptions::default());
    assert!(
        result.is_complete(),
        "reasoning must complete: {:?}",
        result.incomplete_summary()
    );
    assert!(result.errors.is_empty(), "no semantic errors: {:?}", result.errors);
    result
}

fn iri(name: &str) -> Term {
    Term::Iri(Name::from(name))
}

/// Closure triples with this predicate IRI as (subject, object) term pairs.
fn so_pairs(result: &ReasonerResult, predicate: &str) -> BTreeSet<(Term, Term)> {
    result
        .closure
        .iter()
        .filter(|t| t.p == iri(predicate))
        .map(|t| (t.s.clone(), t.o.clone()))
        .collect()
}

/// Literal values of one named subject's objects under `predicate`.
fn object_values(result: &ReasonerResult, subject: &str, predicate: &str) -> BTreeSet<String> {
    result
        .closure
        .iter()
        .filter(|t| t.s == iri(subject) && t.p == iri(predicate))
        .filter_map(|t| match &t.o {
            Term::Literal(l) => Some(l.value.as_str().to_string()),
            _ => None,
        })
        .collect()
}

/// Literal values inside a list-valued object of `subject predicate`.
fn list_values(result: &ReasonerResult, subject: &str, predicate: &str) -> Vec<Vec<String>> {
    result
        .closure
        .iter()
        .filter(|t| t.s == iri(subject) && t.p == iri(predicate))
        .filter_map(|t| match &t.o {
            Term::List(items) => Some(
                items
                    .iter()
                    .map(|item| match item {
                        Term::Literal(l) => l.value.as_str().to_string(),
                        _ => panic!("expected literal list items, got {item:?}"),
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .collect()
}

fn main() -> std::process::ExitCode {
    let cases = vec![
        // ================================================== chaining
        Trial::test("implication_chaining_across_rules", || {
            let program = format!(
                "{HEAD}
                :alice :parent :bob .
                :bob :parent :carol .
                :bob :parent :dave .
                {{ ?x :parent ?y . ?y :parent ?z }} => {{ ?x :grandparent ?z }} .
                {{ ?x :grandparent ?z }} => {{ ?x :egp ?z }} ."
            );
            let result = reason_ok(&program);
            let egp = so_pairs(&result, "http://example.org/egp");
            let expect: BTreeSet<(Term, Term)> = [
                ("http://example.org/alice", "http://example.org/carol"),
                ("http://example.org/alice", "http://example.org/dave"),
            ]
            .iter()
            .map(|(s, o)| (iri(s), iri(o)))
            .collect();
            assert_eq!(
                egp, expect,
                "the second rule must consume exactly the first rule's output"
            );
            Ok(())
        }),

        // ================================================== fixpoint
        Trial::test("recursive_rules_reach_fixpoint", || {
            // Transitive closure over a 5-node chain (4 edges): 10 pairs.
            let mut program = format!(
                "{HEAD}
                {{ ?x :parent ?y }} => {{ ?x :ancestor ?y }} .
                {{ ?x :ancestor ?y . ?y :ancestor ?z }} => {{ ?x :ancestor ?z }} .\n"
            );
            for i in 0..4 {
                let j = i + 1;
                program.push_str(&format!(":n{i} :parent :n{j} .\n"));
            }
            let result = reason_ok(&program);
            let anc = so_pairs(&result, "http://example.org/ancestor");
            let expect: BTreeSet<(Term, Term)> = (0..5usize)
                .flat_map(|i| (i + 1..5).map(move |j| (i, j)))
                .map(|(i, j)| (iri(&format!("http://example.org/n{i}")), iri(&format!("http://example.org/n{j}"))))
                .collect();
            assert_eq!(anc.len(), 10, "closure of a 5-node chain is 10 ordered pairs: {anc:?}");
            assert_eq!(anc, expect, "fixpoint must be the exact transitive closure");
            Ok(())
        }),

        // ================================================== math builtins
        Trial::test("builtin_math_family", || {
            let program = format!(
                "{HEAD}
                :card :sum ( 2 3 ) .
                {{ ?e :sum ( ?a ?b ) .
                   ( ?a ?b ) math:sum ?t .
                }} => {{ ?e :total ?t }} ."
            );
            let result = reason_ok(&program);
            assert_eq!(
                object_values(&result, "http://example.org/card", "http://example.org/total"),
                BTreeSet::from(["5".to_string()]),
                "math:sum over (2 3) must bind ?t to 5"
            );
            Ok(())
        }),

        // ================================================== string builtins
        Trial::test("builtin_string_family", || {
            let program = format!(
                "{HEAD}
                :greeting :head \"Hello, \" .
                :greeting :tail \"world\" .
                {{ ?g :head ?a .
                   ?g :tail ?b .
                   ( ?a ?b ) string:concatenation ?c .
                }} => {{ ?g :text ?c }} .
                {{ ?g :text ?c .
                   ?c string:startsWith \"Hello\" .
                }} => {{ ?g :greeting \"yes\" }} ."
            );
            let result = reason_ok(&program);
            assert_eq!(
                object_values(&result, "http://example.org/greeting", "http://example.org/text"),
                BTreeSet::from(["Hello, world".to_string()]),
                "string:concatenation must join the two parts in order"
            );
            assert_eq!(
                object_values(&result, "http://example.org/greeting", "http://example.org/greeting"),
                BTreeSet::from(["yes".to_string()]),
                "string:startsWith premise must hold on the derived text"
            );
            Ok(())
        }),

        // ================================================== list builtins
        Trial::test("builtin_list_family", || {
            let program = format!(
                "{HEAD}
                :hand :cards ( 3 1 2 ) .
                {{ ?h :cards ?l .
                   ?l list:reverse ?r .
                }} => {{ ?h :reversed ?r }} .
                {{ ?h :reversed ?r .
                   ?r list:sort ?s .
                }} => {{ ?h :sorted ?s }} ."
            );
            let result = reason_ok(&program);
            let sorted = list_values(&result, "http://example.org/hand", "http://example.org/sorted");
            assert_eq!(sorted, vec![vec!["1", "2", "3"]], "reverse-then-sort must yield ( 1 2 3 )");
            Ok(())
        }),

        // ================================================== cyclic data
        Trial::test("termination_on_cyclic_data", || {
            let program = format!(
                "{HEAD}
                :a :next :b . :b :next :c . :c :next :a .
                {{ ?x :next ?y }} => {{ ?x :reaches ?y }} .
                {{ ?x :reaches ?y . ?y :reaches ?z }} => {{ ?x :reaches ?z }} ."
            );
            let result = reason_ok(&program);
            let r = so_pairs(&result, "http://example.org/reaches");
            // 3-cycle: 3 direct edges + 3 distance-2 pairs + 3 self-pairs = 9.
            assert_eq!(r.len(), 9, "3-cycle closure is 9 ordered pairs: {r:?}");
            assert!(
                r.contains(&(iri("http://example.org/a"), iri("http://example.org/a"))),
                "cycle must derive self-reachability: {r:?}"
            );
            Ok(())
        }),

        // ================================================== budgets
        Trial::test("budget_closure_facts_limit", || {
            let program = format!(
                "{HEAD}
                :a :next :b . :b :next :c . :c :next :d .
                {{ ?x :next ?y }} => {{ ?x :reaches ?y }} .
                {{ ?x :reaches ?y . ?y :reaches ?z }} => {{ ?x :reaches ?z }} ."
            );
            let doc = parse_n3(&program, None).expect("program should parse");
            let mut opts = ReasonerOptions::default();
            opts.max_closure_facts = 3; // chain closure derives 6; budget affords 3
            let result = reason(&doc, &opts);
            assert!(!result.is_complete(), "a three-fact budget cannot close a 4-node chain");
            assert!(
                result.limits_reached.contains(&ReasonerLimit::ClosureFacts),
                "the closure-fact limit must be reported: {:?}",
                result.limits_reached
            );
            assert!(
                !result.derived.is_empty(),
                "facts derived before the cut are still reported"
            );
            Ok(())
        }),

        Trial::test("budget_iterations_limit", || {
            let program = format!(
                "{HEAD}
                :a :next :b . :b :next :c . :c :next :a .
                {{ ?x :next ?y }} => {{ ?x :reaches ?y }} .
                {{ ?x :reaches ?y . ?y :reaches ?z }} => {{ ?x :reaches ?z }} ."
            );
            let doc = parse_n3(&program, None).expect("program should parse");
            let mut opts = ReasonerOptions::default();
            opts.max_iterations = 2;
            let result = reason(&doc, &opts);
            assert!(!result.is_complete(), "a two-step budget cannot close a cycle");
            assert!(
                result.limits_reached.contains(&ReasonerLimit::Iterations),
                "the iteration limit must be reported: {:?}",
                result.limits_reached
            );
            Ok(())
        }),

        // ================================================== F1: formula identity
        Trial::test("formula_binder_sets_mint_distinct_skolems", || {
            use purrdf_n3::ast::{Document, Graph, Rule, Triple};
            use std::collections::BTreeSet;

            // Two formulas with byte-identical triples that differ ONLY in
            // their existential binder sets: A binds nothing in-formula (the
            // blank is some outer binder), B declares `q` existential.
            let same_triples = || {
                vec![Triple::new(
                    iri("http://example.org/x"),
                    iri("http://example.org/p"),
                    Term::Blank(Name::from("q")),
                )]
            };
            let f_a = Graph { universals: BTreeSet::new(), existentials: BTreeSet::new(), triples: same_triples() };
            let f_b = Graph { universals: BTreeSet::new(), existentials: [Name::from("q")].into(), triples: same_triples() };
            // The terms are distinct...
            assert_ne!(
                Term::Formula(f_a.clone()),
                Term::Formula(f_b.clone()),
                "precondition: the two formulas must be distinct terms"
            );
            // ...while the historical identity substrate (Debug) cannot tell
            // them apart when neither declares universals.
            assert_eq!(
                format!("{:?}", Term::Formula(f_a.clone())),
                format!("{:?}", Term::Formula(f_b.clone())),
                "precondition: Debug must be the collision the finding names"
            );

            let log_skolem = iri("http://www.w3.org/2000/10/swap/log#skolem");
            let skolem_rule = |subject: Term, predicate: &'static str| {
                Rule::new(
                    Graph::from(vec![Triple::new(subject, log_skolem.clone(), Term::var("sk"))]),
                    vec![Triple::new(iri("http://example.org/w"), iri(predicate), Term::var("sk"))],
                    true,
                )
            };
            let mut doc = Document::new();
            doc.rules = vec![
                skolem_rule(Term::Formula(f_a), "http://example.org/first_genid"),
                skolem_rule(Term::Formula(f_b), "http://example.org/second_genid"),
            ];
            // Conclusions: `:w :first_genid ?sk` / `:w :second_genid ?sk`.
            let result = reason(&doc, &ReasonerOptions::default());
            assert!(result.is_complete(), "run must complete: {:?}", result.incomplete_summary());

            let genid = |predicate: &str| -> BTreeSet<String> {
                result
                    .closure
                    .iter()
                    .filter(|t| t.p == iri(predicate))
                    .map(|t| match &t.o {
                        Term::Iri(v) => v.as_str().to_string(),
                        other => panic!("expected a skolem IRI, got {other:?}"),
                    })
                    .collect()
            };
            let first = genid("http://example.org/first_genid");
            let second = genid("http://example.org/second_genid");
            assert_eq!(first.len(), 1, "rule over formula A must fire once: {first:?}");
            assert_eq!(second.len(), 1, "rule over formula B must fire once: {second:?}");
            assert!(
                first.is_disjoint(&second),
                "two formulas differing only in existential binder sets must mint \
                 DISTINCT skolem IRIs; both rules saw {first:?}",
            );
            Ok(())
        }),

        // ================================================== F4: unknown builtins
        Trial::test("unknown_builtin_is_typed_error_not_silent_complete", || {
            let program = format!(
                "{HEAD}
                :in :value 5 .
                {{ ?i :value ?v . ?v math:totallyUnknownOp ?y . }} => {{ :out :got ?y }} ."
            );
            let doc = parse_n3(&program, None).expect("program should parse");
            let result = reason(&doc, &ReasonerOptions::default());
            assert!(
                !result.is_complete(),
                "a rule whose builtin cannot be evaluated must not report Complete"
            );
            let named = result.errors.iter().any(|e| {
                matches!(e, purrdf_n3::reasoner::ReasonerError::UnsupportedBuiltin { builtin, .. }
                    if builtin == "http://www.w3.org/2000/10/swap/math#totallyUnknownOp")
            });
            assert!(
                named,
                "the typed error must name the builtin: {:?}",
                result.errors
            );
            Ok(())
        }),

        Trial::test("known_builtins_still_complete_without_errors", || {
            // Guard against the unknown-builtin gate over-firing: rule-as-data
            // (log:implies as a fact predicate) and ordinary rdf:type must keep
            // evaluating.
            let program = format!(
                "{HEAD}
                :s :p :o .
                {{ ?a :p ?b . ?b log:implies ?c . }} => {{ :x :ruleData ?c }} .
                :t rdf:type :C .
                {{ ?t rdf:type ?c . }} => {{ :x :typed ?c }} ."
            );
            let result = reason_ok(&program);
            assert!(
                !result.derived.is_empty(),
                "rule-as-data and rdf:type premises must keep evaluating"
            );
            Ok(())
        }),

        // ================================================== F5: math:equalTo
        Trial::test("math_equalTo_compares_decimals_exactly", || {
            let program = format!(
                "{HEAD}
                {{ 0.30000000000000004 math:equalTo 0.3 . }} => {{ :w :eq \"yes\" }} .
                {{ 0.30000000000000004 math:notEqualTo 0.3 . }} => {{ :w :neq \"yes\" }} ."
            );
            let result = reason_ok(&program);
            let eq = object_values(&result, "http://example.org/w", "http://example.org/eq");
            let neq = object_values(&result, "http://example.org/w", "http://example.org/neq");
            assert!(
                eq.is_empty(),
                "0.30000000000000004 and 0.3 are distinct decimals: math:equalTo must not hold (got {eq:?})"
            );
            assert_eq!(
                neq,
                BTreeSet::from(["yes".to_string()]),
                "math:notEqualTo must hold for the distinct decimals"
            );
            Ok(())
        }),

        Trial::test("math_equalTo_handles_nan_and_big_decimals", || {
            let program = format!(
                "{HEAD}
                {{ \"NaN\"^^xsd:decimal math:notEqualTo 5 . }} => {{ :w :nanneq \"yes\" }} .
                {{ \"NaN\"^^xsd:decimal math:equalTo \"NaN\"^^xsd:decimal . }} => {{ :w :naneq \"yes\" }} .
                {{ \"100000000000000000001\"^^xsd:decimal math:notEqualTo \"100000000000000000000\"^^xsd:decimal . }} => {{ :w :bigdiff \"yes\" }} .
                {{ \"100000000000000000001\"^^xsd:decimal math:equalTo \"100000000000000000000\"^^xsd:decimal . }} => {{ :w :bigeq \"yes\" }} ."
            );
            let result = reason_ok(&program);
            let val = |p: &str| object_values(&result, "http://example.org/w", &format!("http://example.org/{p}"));
            assert_eq!(
                val("nanneq"),
                BTreeSet::from(["yes".to_string()]),
                "NaN differs from 5: math:notEqualTo must hold"
            );
            assert!(
                val("naneq").is_empty(),
                "NaN never equals anything, including itself: math:equalTo must not hold"
            );
            assert_eq!(
                val("bigdiff"),
                BTreeSet::from(["yes".to_string()]),
                "the two 21-digit decimals are distinct values"
            );
            assert!(
                val("bigeq").is_empty(),
                "the two 21-digit decimals are distinct values: math:equalTo must not hold"
            );
            Ok(())
        }),

        Trial::test("math_equalTo_still_equates_equal_decimals", || {
            // The fix must not lose ordinary equality: equal lexical values,
            // equivalent forms, and integer/decimal crossings stay equal.
            let program = format!(
                "{HEAD}
                {{ 0.3 math:equalTo 0.30 . }} => {{ :w :equiv \"yes\" }} .
                {{ 3 math:equalTo 3.0 . }} => {{ :w :cross \"yes\" }} ."
            );
            let result = reason_ok(&program);
            let val = |p: &str| object_values(&result, "http://example.org/w", &format!("http://example.org/{p}"));
            assert_eq!(
                val("equiv"),
                BTreeSet::from(["yes".to_string()]),
                "0.3 and 0.30 are the same decimal value"
            );
            assert_eq!(
                val("cross"),
                BTreeSet::from(["yes".to_string()]),
                "integer 3 equals decimal 3.0"
            );
            Ok(())
        }),

        // ================================================== F6: exponentiation gates
        Trial::test("exponentiation_never_mints_non_finite_literals", || {
            let program = format!(
                "{HEAD}
                {{ ( 2 10000000 ) math:exponentiation ?x . }} => {{ :w :overflow \"yes\" }} .
                {{ ( 1 ?e ) math:exponentiation 10 . }} => {{ :w :base1 \"yes\" }} .
                {{ ( -8 0.5 ) math:exponentiation ?x . }} => {{ :w :negbase \"yes\" }} ."
            );
            let result = reason_ok(&program);
            for p in ["overflow", "base1", "negbase"] {
                assert!(
                    object_values(&result, "http://example.org/w", &format!("http://example.org/{p}")).is_empty(),
                    "non-finite exponentiation results must fail the premise, not mint \
                     INF/NaN as xsd:decimal ({p} fired: {:?})",
                    object_values(&result, "http://example.org/w", &format!("http://example.org/{p}"))
                );
            }
            Ok(())
        }),

        Trial::test("exponentiation_still_computes_finite_results", || {
            // Guard: the finiteness gates must not take real arithmetic down.
            let program = format!(
                "{HEAD}
                {{ ( 2 10 ) math:exponentiation ?x . }} => {{ :w :pow ?x }} .
                {{ ( 2 ?e ) math:exponentiation 1024 . }} => {{ :w :loge ?e }} .
                {{ ( 9 0.5 ) math:exponentiation ?x . }} => {{ :w :sqrt ?x }} ."
            );
            let result = reason_ok(&program);
            assert_eq!(
                object_values(&result, "http://example.org/w", "http://example.org/pow"),
                BTreeSet::from(["1024".to_string()]),
                "(2 10) must still exponentiate exactly"
            );
            assert_eq!(
                object_values(&result, "http://example.org/w", "http://example.org/loge"),
                BTreeSet::from(["10".to_string()]),
                "the base-2 logarithm of 1024 must still solve exactly"
            );
            assert_eq!(
                object_values(&result, "http://example.org/w", "http://example.org/sqrt"),
                BTreeSet::from(["3".to_string()]),
                "9^0.5 = 3"
            );
            Ok(())
        }),
    ];
    harness::main(cases)
}

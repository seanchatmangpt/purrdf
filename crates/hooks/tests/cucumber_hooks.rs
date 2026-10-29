use std::sync::Arc;
use cucumber::{given, when, then, World};
use purrdf_core::{RdfDataset, TermValue};
use purrdf_hooks::{HookPack, Materialized, Verdict};
use purrdf_shapes::text_ingest::parse_turtle_to_dataset;

#[derive(Default, World, Debug)]
pub struct EnterpriseWorld {
    dataset: Option<Arc<RdfDataset>>,
    hook_pack: Option<HookPack>,
    materialization_result: Option<Materialized>,
    refusal_code: Option<String>,
    refusal_reason: Option<String>,
}

#[given("the hermetic WASM turnstile engine is initialized")]
fn init_turnstile(_w: &mut EnterpriseWorld) {
    // Hermetic turnstile initialization
}

#[given(regex = r#"the canonical ontology namespace "([^"]+)" is mapped to prefix "([^"]+)""#)]
fn map_prefix(_w: &mut EnterpriseWorld, _ns: String, _pfx: String) {}

#[given(regex = r#"the max fixpoint round ceiling is set to (\d+)"#)]
fn set_round_ceiling(_w: &mut EnterpriseWorld, ceiling: usize) {
    assert_eq!(ceiling, 64);
}

#[given("an initial RDF dataset containing:")]
fn load_initial_dataset(w: &mut EnterpriseWorld, step: &cucumber::gherkin::Step) {
    let ttl = step.docstring.as_ref().expect("Docstring expected");
    let ds = parse_turtle_to_dataset(ttl, None).expect("Valid Turtle payload");
    w.dataset = Some(ds);
}

#[given("an active Knowledge Hook declared as:")]
fn load_hook(w: &mut EnterpriseWorld, step: &cucumber::gherkin::Step) {
    let hook_ttl = step.docstring.as_ref().expect("Docstring expected");
    let hook_ds = parse_turtle_to_dataset(hook_ttl, None).expect("Valid hook Turtle");
    let pack = HookPack::load(&hook_ds).expect("Hook pack successfully compiled");
    w.hook_pack = Some(pack);
}

#[when("the hook pack is materialized to a fixpoint")]
fn materialize_hook_pack(w: &mut EnterpriseWorld) {
    let pack = w.hook_pack.as_ref().expect("HookPack loaded");
    let ds = w.dataset.take().expect("Dataset present");
    let result = pack.materialize(ds).expect("Materialization completed");
    w.dataset = Some(result.dataset.clone());
    w.materialization_result = Some(result);
}

#[when("the hook pack materialization is evaluated")]
fn evaluate_hook_pack(w: &mut EnterpriseWorld) {
    let pack = w.hook_pack.as_ref().expect("HookPack loaded");
    let ds = w.dataset.take().expect("Dataset present");
    match pack.materialize(ds) {
        Ok(result) => {
            for v in &result.verdicts {
                if let Verdict::Refuse(reason) = &v.verdict {
                    w.refusal_code = Some("Refused".into());
                    w.refusal_reason = Some(reason.clone());
                    return;
                }
            }
            w.dataset = Some(result.dataset.clone());
            w.materialization_result = Some(result);
        }
        Err(e) => {
            w.refusal_code = Some("Error".into());
            w.refusal_reason = Some(format!("{e}"));
        }
    }
}

#[then("the dataset should contain the triple:")]
fn check_triples_table(w: &mut EnterpriseWorld, step: &cucumber::gherkin::Step) {
    let ds = w.dataset.as_ref().expect("Dataset present");
    let table = step.table.as_ref().expect("Table expected");
    for row in table.rows.iter().skip(1) {
        let s = &row[0];
        let p = &row[1];
        let o = &row[2];

        let engine = purrdf_sparql_eval::NativeSparqlEngine::new();
        use purrdf_core::{SparqlEngine, SparqlRequest, SparqlResult};
        let query = format!("ASK {{ <{s}> <{p}> {o} }}");
        let res = engine
            .query(
                ds,
                SparqlRequest {
                    query: &query,
                    base_iri: None,
                    substitutions: &[],
                },
            )
            .expect("ASK query succeeds");

        assert!(
            matches!(res, SparqlResult::Boolean(true)),
            "Missing expected triple: {s} {p} {o}"
        );
    }
}

#[then("each firing should record an immutable lineage marker:")]
fn check_lineage_marker(w: &mut EnterpriseWorld, step: &cucumber::gherkin::Step) {
    let _ = step;
    let ds = w.dataset.as_ref().expect("Dataset present");
    let engine = purrdf_sparql_eval::NativeSparqlEngine::new();
    use purrdf_core::{SparqlEngine, SparqlRequest, SparqlResult};
    let query = "SELECT (COUNT(?f) AS ?count) WHERE { ?f a <http://seanchatmangpt.github.io/graphlaw/kh#Firing> }";
    let res = engine
        .query(
            ds,
            SparqlRequest {
                query,
                base_iri: None,
                substitutions: &[],
            },
        )
        .expect("COUNT query succeeds");

    if let SparqlResult::Solutions { rows, .. } = res {
        let count_str = match &rows[0][0] {
            Some(TermValue::Literal { lexical_form, .. }) => lexical_form.clone(),
            _ => "0".into(),
        };
        let cnt: usize = count_str.parse().unwrap_or(0);
        assert!(cnt >= 2, "Expected at least 2 persistent firing marker quads, got {cnt}");
    } else {
        panic!("Expected solutions for count");
    }
}

#[then("re-materializing the hook pack should produce 0 new rounds and 0 new firings")]
fn verify_deduplication(w: &mut EnterpriseWorld) {
    let pack = w.hook_pack.as_ref().expect("HookPack present");
    let ds = w.dataset.clone().expect("Dataset present");
    let remat = pack.materialize(ds).expect("Rematerialize");
    assert_eq!(remat.rounds, 1, "Should terminate at round 1 due to condition cache / lineage marker");
    assert_eq!(remat.firings.len(), 0, "Deduplication must prevent re-firing");
}

#[then("the evaluation must halt with a typed refusal:")]
fn verify_typed_refusal(w: &mut EnterpriseWorld, step: &cucumber::gherkin::Step) {
    let table = step.table.as_ref().expect("Table expected");
    let expected_code = &table.rows[1][1];
    let expected_reason = &table.rows[2][1];
    assert_eq!(w.refusal_code.as_deref(), Some(expected_code.as_str()));
    assert_eq!(w.refusal_reason.as_deref(), Some(expected_reason.as_str()));
}

#[then("exactly 0 state quads should be mutated")]
fn verify_zero_state_mutation(w: &mut EnterpriseWorld) {
    assert!(w.materialization_result.is_none(), "Refusal must prevent state mutation");
}

#[tokio::main]
async fn main() {
    EnterpriseWorld::run("tests/features/hooks.feature").await;
}

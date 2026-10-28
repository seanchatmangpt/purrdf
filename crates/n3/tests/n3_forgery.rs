//! Forgery court (WP P01): three forge classes against the proof checker,
//! each with a flip twin, so the hardening is witnessed both ways
//! (harness = false, same binary runs through the workspace wasm runner).
//!
//! F1 — impure laundering: a `log:content` step whose conclusion a rule
//!       step then resolves to. Hardened, the impure step keeps its
//!       obligation (`impure built-in`) but loses all resolution power: the
//!       laundered premise is C4-unresolved and the claim dies. `legacy()`
//!       restores the pre-hardening acceptance (the flip).
//! F2 — cross-document digest splice: a fact step citing a digest lifted
//!       from a different document. Hardened, C1 kills it; the honest twin
//!       (the source's own digest, the same label) verifies. `legacy()`
//!       skips the digest rows (the flip).
//! F3 — variadic head: a rule whose head carries a universal the recorded
//!       application never pins; the step then concludes every instance of
//!       itself. Hardened, C1 rejects the unpinned head variable; the
//!       honest twin pins it with `pe:binding`. `legacy()` restores the
//!       pre-hardening acceptance (the flip).
//!
//! Every assertion is on the checker's real report; no doubles.

use purrdf_n3::ast::Document;
use purrdf_n3::proof_check::{CheckPolicy, Failure, Report};
use purrdf_n3::proof_check_n3::{
    check_proof, check_proof_document_with_policy, check_proof_with_policy,
};

const PE: &str = "https://eyereasoner.github.io/pe#";
const NS: &str = "http://example.org/p01/";

/// BLAKE3 through the crate's own digest site: parse the text under a label
/// and read the digest the parser minted (the same value a fact step
/// records as `pe:sourceDigest`).
fn digest_of(text: &str) -> Result<String, String> {
    let document = purrdf_n3::parser::parse_n3_with_source(text, None, Some("digest-site.n3"))
        .map_err(|e| format!("digest fixture parses: {e}"))?;
    document
        .fact_sources
        .values()
        .find_map(|source| source.digest.clone())
        .ok_or_else(|| "a labeled parse mints digest-carrying source refs".to_string())
}

fn hardened_report(source: &str, proof: &str, label: &str) -> Result<Report, String> {
    check_proof(source, proof, label).map_err(|e| format!("forge document parses: {e}"))
}

fn legacy_report(source: &str, proof: &str, label: &str) -> Result<Report, String> {
    check_proof_with_policy(source, proof, label, CheckPolicy::legacy())
        .map_err(|e| format!("forge document parses: {e}"))
}

fn failure_under<'r>(report: &'r Report, condition: &str) -> Option<&'r Failure> {
    report
        .failures
        .iter()
        .find(|failure| failure.condition == condition)
}

fn expect(condition: bool, what: String) -> Result<(), String> {
    if condition { Ok(()) } else { Err(what) }
}

// ---------------------------------------------------------------- F1 -----

const F1_SOURCE: &str = "\
@prefix : <http://example.org/p01/> .
@prefix log: <http://www.w3.org/2000/10/swap/log#> .
:doc :meta \"x\" .
{ :doc log:content \"stolen\" } => { :a :p :b } .
";

const F1_FORGE: &str = "\
@prefix : <http://example.org/p01/> .
@prefix log: <http://www.w3.org/2000/10/swap/log#> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:a :p :b .

{ :doc log:content \"stolen\" } pe:builtin log:content .

{ :a :p :b }
    pe:rule 1 ;
    pe:uses { :doc log:content \"stolen\" } .
";

fn f1_impure_laundering_splice_is_killed_with_the_obligation_left_visible() -> Result<(), String> {
    let report = hardened_report(F1_SOURCE, F1_FORGE, "f1.n3")?;
    let c4 = failure_under(&report, "C4");
    expect(
        !report.valid(),
        format!("the laundered premise must not resolve; report: {report:?}"),
    )?;
    let c4 = c4.ok_or_else(|| format!("the laundered premise must fail C4; report: {report:?}"))?;
    expect(
        c4.detail
            .contains("neither an instance of a step's conclusion nor given by the source"),
        format!("unexpected C4 detail: {}", c4.detail),
    )?;
    // The obligation stays visible: the impure read is still recorded as an
    // obligation, its resolution power is what died.
    expect(
        report
            .obligations
            .iter()
            .any(|obligation| obligation.kind == "impure built-in"),
        format!("the impure read must remain a visible obligation; report: {report:?}"),
    )
    // And the claim `:a :p :b` must not be waved through on the strength of
    // the laundering step: with the premise unresolved the report is invalid.
}

fn f1_flip_legacy_restores_the_pre_hardening_acceptance() -> Result<(), String> {
    let report = legacy_report(F1_SOURCE, F1_FORGE, "f1.n3")?;
    expect(
        report.valid(),
        format!("legacy() must restore the pre-hardening acceptance; failures: {report:?}"),
    )
}

// ---------------------------------------------------------------- F2 -----

const F2_SOURCE: &str = "\
@prefix : <http://example.org/p01/> .
:s :p :o .
";

/// A different document, so its digest is the splice's wrong value.
const F2_OTHER: &str = "\
@prefix : <http://example.org/other/> .
:t :q :u .
";

fn f2_cross_document_digest_splice_is_killed_by_c1() -> Result<(), String> {
    let other = digest_of(F2_OTHER)?;
    let forge = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :p :o .

{{ :s :p :o }} pe:fact \"other.n3\" ; pe:sourceDigest \"{other}\" .
"
    );
    let report = hardened_report(F2_SOURCE, &forge, "f2.n3")?;
    expect(
        !report.valid(),
        format!("a spliced digest must not pass; report: {report:?}"),
    )?;
    let c1 = failure_under(&report, "C1")
        .ok_or_else(|| format!("the splice must fail C1; report: {report:?}"))?;
    expect(
        c1.detail.contains("cross-document splice"),
        format!("unexpected C1 detail: {}", c1.detail),
    )?;

    // The honest twin: the source's own digest and label verifies.
    let source_digest = digest_of(F2_SOURCE)?;
    let honest = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :p :o .

{{ :s :p :o }} pe:fact \"f2.n3\" ; pe:sourceDigest \"{source_digest}\" .
"
    );
    let report = hardened_report(F2_SOURCE, &honest, "f2.n3")?;
    expect(
        report.valid(),
        format!("an honest digest-anchored fact step must verify; report: {report:?}"),
    )?;
    expect(
        report.verified == 1,
        format!("the fact step must count as verified, not an obligation; report: {report:?}"),
    )
}

fn f2_flip_legacy_skips_the_digest_rows() -> Result<(), String> {
    let other = digest_of(F2_OTHER)?;
    let forge = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :p :o .

{{ :s :p :o }} pe:fact \"other.n3\" ; pe:sourceDigest \"{other}\" .
"
    );
    let report = legacy_report(F2_SOURCE, &forge, "f2.n3")?;
    expect(
        report.valid(),
        format!("legacy() must restore the pre-hardening acceptance; failures: {report:?}"),
    )
}

fn f2_expect_without_a_source_digest_is_c1() -> Result<(), String> {
    // The source document here is parsed WITHOUT a label, so no fact in it
    // carries a digest — yet the step records one.
    let unlabeled: Document =
        purrdf_n3::parser::parse_n3(F2_SOURCE, None).map_err(|e| format!("fixture parses: {e}"))?;
    let source_digest = digest_of(F2_SOURCE)?;
    let forge = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :p :o .

{{ :s :p :o }} pe:fact \"f2.n3\" ; pe:sourceDigest \"{source_digest}\" .
"
    );
    let report = check_proof_document_with_policy(&unlabeled, &forge, CheckPolicy::default())
        .map_err(|e| format!("forge parses: {e}"))?;
    let c1 = failure_under(&report, "C1").ok_or_else(|| {
        format!("a recorded digest with no source-side digest must fail C1; report: {report:?}")
    })?;
    expect(
        c1.detail
            .contains("no digest-carrying source document gives this fact"),
        format!("unexpected C1 detail: {}", c1.detail),
    )?;

    // The same document under legacy() does not consult digests.
    let report = check_proof_document_with_policy(&unlabeled, &forge, CheckPolicy::legacy())
        .map_err(|e| format!("forge parses: {e}"))?;
    expect(
        report.valid(),
        format!("legacy() must skip the digest rows; failures: {report:?}"),
    )
}

fn f2_unrecorded_digest_stays_a_visible_obligation() -> Result<(), String> {
    // The source carries a digest; the step records none: the step is an
    // obligation ("unverified source digest"), never verified.
    let proof = "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :p :o .

{ :s :p :o } pe:fact \"f2.n3\" .
";
    let report = hardened_report(F2_SOURCE, proof, "f2.n3")?;
    expect(
        report.valid(),
        format!("an unrecorded digest is an obligation, not a failure; report: {report:?}"),
    )?;
    expect(
        report.verified == 0,
        format!("the fact step must not count as verified; report: {report:?}"),
    )?;
    expect(
        report
            .obligations
            .iter()
            .any(|obligation| obligation.kind == "unverified source digest"),
        format!("the unverified digest must remain a visible obligation; report: {report:?}"),
    )
}

fn f2_label_mismatch_is_c1() -> Result<(), String> {
    // The right digest under the wrong document name: the step names a
    // document this fact is not anchored in.
    let source_digest = digest_of(F2_SOURCE)?;
    let forge = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :p :o .

{{ :s :p :o }} pe:fact \"other.n3\" ; pe:sourceDigest \"{source_digest}\" .
"
    );
    let report = hardened_report(F2_SOURCE, &forge, "f2.n3")?;
    let c1 = failure_under(&report, "C1")
        .ok_or_else(|| format!("a mislabeled fact step must fail C1; report: {report:?}"))?;
    expect(
        c1.detail.contains("names source"),
        format!("unexpected C1 detail: {}", c1.detail),
    )
}

// ---------------------------------------------------------------- F3 -----

const F3_SOURCE: &str = "\
@prefix : <http://example.org/p01/> .
:s :p :o .
{ ?a :p ?b } => { ?a :t ?x } .
";

fn f3_variadic_head_is_killed_by_c1() -> Result<(), String> {
    // The forged step leaves the rule's head-only universal ?x unpinned, so
    // the head match mints :evil from the step's own conclusion: the step
    // would conclude every instance of its head.
    let forge = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :t :evil .

{{ :s :t :evil }} pe:rule 1 ; pe:uses {{ :s :p :o }} .
"
    );
    let report = hardened_report(F3_SOURCE, &forge, "f3.n3")?;
    expect(
        !report.valid(),
        format!("an unpinned head variable must not pass; report: {report:?}"),
    )?;
    let c1 = failure_under(&report, "C1")
        .ok_or_else(|| format!("the variadic head must fail C1; report: {report:?}"))?;
    expect(
        c1.detail
            .contains("would conclude every instance of its head"),
        format!("unexpected C1 detail: {}", c1.detail),
    )
}

fn f3_flip_legacy_restores_the_pre_hardening_acceptance() -> Result<(), String> {
    let forge = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :t :evil .

{{ :s :t :evil }} pe:rule 1 ; pe:uses {{ :s :p :o }} .
"
    );
    let report = legacy_report(F3_SOURCE, &forge, "f3.n3")?;
    expect(
        report.valid(),
        format!("legacy() must restore the pre-hardening acceptance; failures: {report:?}"),
    )
}

fn f3_honest_head_only_universal_pins_itself_and_survives() -> Result<(), String> {
    // Head-only universals survive when the step pins them with an explicit
    // `pe:binding` — one firing, one complete recorded assignment.
    let honest = "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:s :t :c .

{ :s :t :c } pe:rule 1 ; pe:uses { :s :p :o } ;
    pe:binding [ pe:var \"x\" ; pe:value :c ] .
";
    let report = hardened_report(F3_SOURCE, honest, "f3.n3")?;
    expect(
        report.valid(),
        format!("a pinned head-only universal must survive; failures: {report:?}"),
    )?;
    expect(
        report.verified >= 1,
        format!("the pinned application must count as verified; report: {report:?}"),
    )
}

fn source_anchored_general_facts_survive_the_hardening() -> Result<(), String> {
    // A general (non-ground) source fact stays a valid anchor: a fact step
    // citing one of its instances verifies, and the digest rows hold it to
    // the anchoring document.
    let source = "\
@prefix : <http://example.org/p01/> .
:generic :holds ?anything .
";
    let source_digest = digest_of(source)?;
    let honest = format!(
        "\
@prefix : <http://example.org/p01/> .
@prefix pe: <https://eyereasoner.github.io/pe#> .

:generic :holds :value .

{{ :generic :holds :value }} pe:fact \"gen.n3\" ; pe:sourceDigest \"{source_digest}\" .
"
    );
    let report = hardened_report(source, &honest, "gen.n3")?;
    expect(
        report.valid(),
        format!("an instance of a general source fact must survive; failures: {report:?}"),
    )?;
    expect(
        report.verified == 1,
        format!("the instance must count as verified; report: {report:?}"),
    )
}

// ------------------------------------------------- positive control -----

/// The honest pipeline: a generated proof for a labeled document checks
/// under the hardened default, with the digest round-trip exercised.
fn honest_generated_proof_checks_under_the_hardened_default() -> Result<(), String> {
    let source = "\
@prefix : <http://example.org/p01/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
:Socrates a :Human .
:Human rdfs:subClassOf :Mortal .
{ ?s a ?a . ?a rdfs:subClassOf ?b } => { ?s a ?b } .
";
    let document = purrdf_n3::parser::parse_n3_with_source(source, None, Some("socrates.n3"))
        .map_err(|e| format!("source parses: {e}"))?;
    let result = purrdf_n3::reasoner::reason(
        &document,
        &purrdf_n3::reasoner::ReasonerOptions {
            proof: true,
            ..Default::default()
        },
    );
    let proof = purrdf_n3::proof_writer::proof_to_n3(&document.prefixes, &result);
    expect(
        proof.contains("pe:sourceDigest"),
        format!("the writer must anchor fact steps with pe:sourceDigest; proof:\n{proof}"),
    )?;
    let report = check_proof(source, &proof, "socrates.n3")
        .map_err(|e| format!("the checker reads its own emission: {e}"))?;
    expect(
        report.valid(),
        format!("an honest generated proof must check; failures: {report:?}"),
    )?;
    expect(
        report.verified == 3,
        format!("two facts + one rule application, all verified; report: {report:?}"),
    )?;
    expect(
        report.obligations.is_empty(),
        format!("no obligations expected; report: {report:?}"),
    )
}

/// The namespace constants are pinned so a rename cannot silently sever
/// the court documents from the vocabulary they attack.
fn court_vocabulary_is_pinned() -> Result<(), String> {
    expect(
        PE == "https://eyereasoner.github.io/pe#",
        "PE spelling moved".to_string(),
    )?;
    expect(
        NS == "http://example.org/p01/",
        "NS spelling moved".to_string(),
    )
}

fn main() {
    let cases: Vec<(&str, fn() -> Result<(), String>)> = vec![
        (
            "f1_impure_laundering_splice_is_killed_with_the_obligation_left_visible",
            f1_impure_laundering_splice_is_killed_with_the_obligation_left_visible,
        ),
        (
            "f1_flip_legacy_restores_the_pre_hardening_acceptance",
            f1_flip_legacy_restores_the_pre_hardening_acceptance,
        ),
        (
            "f2_cross_document_digest_splice_is_killed_by_c1",
            f2_cross_document_digest_splice_is_killed_by_c1,
        ),
        (
            "f2_flip_legacy_skips_the_digest_rows",
            f2_flip_legacy_skips_the_digest_rows,
        ),
        (
            "f2_expect_without_a_source_digest_is_c1",
            f2_expect_without_a_source_digest_is_c1,
        ),
        (
            "f2_unrecorded_digest_stays_a_visible_obligation",
            f2_unrecorded_digest_stays_a_visible_obligation,
        ),
        ("f2_label_mismatch_is_c1", f2_label_mismatch_is_c1),
        (
            "f3_variadic_head_is_killed_by_c1",
            f3_variadic_head_is_killed_by_c1,
        ),
        (
            "f3_flip_legacy_restores_the_pre_hardening_acceptance",
            f3_flip_legacy_restores_the_pre_hardening_acceptance,
        ),
        (
            "f3_honest_head_only_universal_pins_itself_and_survives",
            f3_honest_head_only_universal_pins_itself_and_survives,
        ),
        (
            "source_anchored_general_facts_survive_the_hardening",
            source_anchored_general_facts_survive_the_hardening,
        ),
        (
            "honest_generated_proof_checks_under_the_hardened_default",
            honest_generated_proof_checks_under_the_hardened_default,
        ),
        ("court_vocabulary_is_pinned", court_vocabulary_is_pinned),
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
    println!("{} passed, {} failed (of 13)", passed, failed);
    if failed > 0 {
        std::process::exit(1);
    }
}

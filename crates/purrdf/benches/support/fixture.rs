// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Shared fixture for the facade's pipeline benches: one deterministic
//! N-Triples document, the one SELECT the pipeline runs, and the small helpers
//! both benches share.
//!
//! Included with `#[path = "support/fixture.rs"]` by `pipeline_e2e.rs` (the
//! timed bench) and `pipeline_alloc.rs` (the allocation probe), so there is
//! exactly ONE definition of the document every facade bench measures — the
//! same one-home rule the workspace's RNG job follows.

/// The fixture's media type: the canonical N-Triples spelling the codec table
/// routes on.
pub(crate) const NTRIPLES: &str = "application/n-triples";

/// The document size the fixture is generated until it crosses, in bytes.
pub(crate) const TARGET_BYTES: usize = 1_000_000;

/// The generator seed. Nonzero — an LCG state of zero is a fixed point under
/// any increment of 0, and the one-home generator below keeps the state as the
/// accumulator itself, so the nonzero seed is the only requirement.
pub(crate) const SEED: u64 = 0x0DDB_1A5E_5BAD_5EED_u64;

/// The `foaf:name` predicate, spoken in full — an N-Triples document has no
/// prefix directives to shorten it.
const NAME_IRI: &str = "http://xmlns.com/foaf/0.1/name";

/// The `rdfs:label` predicate.
const LABEL_IRI: &str = "http://www.w3.org/2000/01/rdf-schema#label";

/// The fixture's one object-to-object edge predicate.
const KNOWS_IRI: &str = "http://example.org/vocab/knows";

/// The one SPARQL SELECT the pipeline runs, named by the bench's own constant.
///
/// A bound, full-corpus read of every `foaf:name`: one solution per subject,
/// which prices the storage read, the solution materialization and the
/// projection together — the query half of what a consumer ships.
pub(crate) const SELECT_NAME: &str =
    "SELECT ?s ?name WHERE { ?s <http://xmlns.com/foaf/0.1/name> ?name }";

/// The word pool the seeded literals are drawn from.
const WORDS: [&str; 8] = [
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
];

/// The one-home seeded generator: [`purrdf_hash::mix::lcg64_next`] (Knuth's
/// MMIX constants, the helpers-ledger home for this job — `n3`'s and
/// `branchless`'s benches draw from the same function, never a private
/// re-derivation). `state` must be nonzero, which [`SEED`] guarantees.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        purrdf_hash::mix::lcg64_next(&mut self.0, 1)
    }

    /// One index below `bound`.
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// One seeded literal: three pool words and the row number, so literal length
/// varies but is a pure function of the seed.
fn seeded_name(rng: &mut Lcg, row: usize) -> String {
    let a = WORDS[rng.below(WORDS.len())];
    let b = WORDS[rng.below(WORDS.len())];
    let c = WORDS[rng.below(WORDS.len())];
    format!("{a} {b} {c} {row}")
}

/// The deterministic fixture: N-Triples text, three statements per subject,
/// generated until the document crosses [`TARGET_BYTES`]. Every byte is a pure
/// function of [`SEED`], so every run measures the same document.
pub(crate) fn build_fixture() -> Vec<u8> {
    let mut rng = Lcg(SEED);
    let mut document = String::new();
    let mut row = 0usize;
    while document.len() < TARGET_BYTES {
        let subject = format!("<http://example.org/item/{row}>");
        let name = seeded_name(&mut rng, row);
        let label = seeded_name(&mut rng, row);
        let knows = format!(
            "{subject} <{KNOWS_IRI}> <http://example.org/item/{}> .\n",
            rng.below(row.max(1))
        );
        document.push_str(&format!("{subject} <{NAME_IRI}> \"{name}\" .\n"));
        document.push_str(&format!("{subject} <{LABEL_IRI}> \"{label}\" .\n"));
        document.push_str(&knows);
        row += 1;
    }
    document.into_bytes()
}

/// The request every stage runs: [`SELECT_NAME`] against the fixture's store.
pub(crate) fn select_request() -> SparqlRequest<'static> {
    SparqlRequest {
        query: SELECT_NAME,
        base_iri: None,
        substitutions: &[],
    }
}

/// Run the SELECT over `dataset` and return the solution count, so the
/// optimizer cannot drop the evaluation.
pub(crate) fn solution_rows(result: SparqlResult) -> usize {
    match result {
        SparqlResult::Solutions { rows, .. } => rows.len(),
        other => panic!("a SELECT answers with solutions, got {other:?}"),
    }
}

/// Serialize `result` to SPARQL Results JSON and return the document bytes.
pub(crate) fn serialize_json(result: &SparqlResult) -> Vec<u8> {
    serialize(
        result,
        SparqlResultsFormat::Json,
        &ResultProvenance::default(),
        None,
    )
    .expect("the fixture answer serializes")
    .bytes
}

use purrdf::sparql::{ResultProvenance, SparqlResult, SparqlResultsFormat, serialize};
use purrdf::{SparqlRequest, parse_dataset};

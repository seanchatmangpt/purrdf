// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

#![forbid(unsafe_code)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/Blackcat-Informatics/purrdf/main/docs/purrdf-logo.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/Blackcat-Informatics/purrdf/main/docs/purrdf-logo.svg"
)]

//! PurRDF native Notation3: parsing, reasoning and proofs over RDF 1.2.
//!
//! The crate exposes the N3 parser, the forward/backward reasoner and the
//! proof support, along with the RDF syntaxes N3 reasoning reads: Turtle,
//! TriG, N-Triples, N-Quads and RDF Message Logs.
//!
//! # Facade contract
//!
//! This file is the crate facade: it declares the module tree and re-exports
//! the public API at the root so callers depend on one stable surface, not on
//! module paths. Identical input yields byte-identical output on every target
//! — no map iteration order reaches an output path.

/// The N3 abstract syntax tree: terms, graphs, rules, documents, and the builtin vocabulary constants.
pub mod ast;
/// The error and result types shared by parsing and reasoning.
pub mod error;
/// The N3/Turtle lexer.
pub mod lexer;
/// The N3 parser (plus Turtle, TriG, N-Quads and RDF Message Log inputs) producing `Document`s.
pub mod parser;
/// Serializing documents, triples and reasoner output back to N3/TriG text.
pub mod printing;
/// Checking a proof document: the reference implementation, and its N3 reader.
pub mod proof_check;
pub mod proof_check_n3;
/// Writing an N3 proof document for what the reasoner derived.
pub mod proof_writer;
pub mod rdf_compat;
/// Compatibility helpers for the RDF syntaxes the N3 reasoner consumes.
pub mod reasoner;
pub mod sudoku;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use ast::{Document, Graph, Literal, Name, Rule, SourceRef, Term, Triple};
pub use error::{EyeronError, Result};
pub use parser::{is_rdf_message_log, parse_n3, parse_n3_with_source, parse_rdf_message_log};
pub use printing::{
    document_debug, fuse_report, rdf_result_to_string, result_to_string, triples_to_n3,
    triples_to_trig,
};
pub use proof_writer::proof_to_n3;
pub use reasoner::{
    CompletionStatus, FiredFuse, PreparedReasoner, ReasonerError, ReasonerLimit, ReasonerOptions,
    ReasonerResult, ReasonerStatistics, reason as reason_document,
};

/// Parse an N3 string, run the forward reasoner, and return the N3 output for
/// newly-derived triples.
pub fn reason(input: &str) -> Result<String> {
    let doc = if is_rdf_message_log(input) {
        parse_rdf_message_log(input, None)?
    } else {
        parse_n3(input, None)?
    };
    // Only `.derived` is read below: skip the `.explicit`/`.explicit_sources`
    // clone of every fact in `doc` that this function has no use for.
    let options = ReasonerOptions {
        include_explicit: false,
        ..ReasonerOptions::default()
    };
    let result = reason_document(&doc, &options);
    if let Some(summary) = result.incomplete_summary() {
        return Err(EyeronError::new(summary));
    }
    Ok(result_to_string(&doc.prefixes, &result.derived))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socrates() {
        let input = r#"
            @prefix : <http://example.org/> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
            :Socrates a :Human .
            :Human rdfs:subClassOf :Mortal .
            { ?s a ?a . ?a rdfs:subClassOf ?b . } => { ?s a ?b . } .
        "#;
        let out = reason(input).unwrap();
        assert!(out.contains(":Socrates a :Mortal"), "{}", out);
    }

    #[test]
    fn prepared_reasoner_keeps_data_batches_independent() {
        let program = parse_n3(
            r#"
                @prefix : <http://example.org/> .
                { ?x :input ?value } => { ?x :output ?value } .
            "#,
            None,
        )
        .unwrap();
        let prepared = PreparedReasoner::new(program);
        let first = parse_n3("@prefix : <http://example.org/> . :first :input 1 .", None).unwrap();
        let second =
            parse_n3("@prefix : <http://example.org/> . :second :input 2 .", None).unwrap();

        let first_result = prepared.reason(&first, &ReasonerOptions::default());
        let second_result = prepared.reason(&second, &ReasonerOptions::default());

        assert!(
            result_to_string(&first.prefixes, &first_result.derived).contains(":first :output 1")
        );
        let second_output = result_to_string(&second.prefixes, &second_result.derived);
        assert!(second_output.contains(":second :output 2"));
        assert!(!second_output.contains(":first"));
    }
}

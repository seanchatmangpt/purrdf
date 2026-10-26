// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! N3 reasoner benchmarks.
//!
//! `purrdf-n3` had no bench of its own: the nearest was the datalog evaluator's
//! `seminaive` bench, which measures the join, not the parser that feeds it or
//! the proof machinery that audits its output. These three give the N3 stack
//! its own numbers:
//!
//! * `parse` — throughput of `parse_n3` over a generated flat document of
//!   simple triples at two sizes (1k and 10k triples), so a parser change has
//!   a per-triple number that is not mixed with reasoning.
//! * `fixpoint` — forward chaining to convergence: the two rules that close
//!   `p` into `anc` transitively over a chain, so the run takes many rounds
//!   and a semi-naive-style change has a number that moves.
//! * `proof` — proof generation (`proof: true`) plus `proof_to_n3` and a full
//!   `check_proof` verification of the written proof, so the audit path is
//!   measured as one operation and cannot be optimized by weakening either
//!   half.
//!
//! Report-only, per this repository's rule: benches exist so a later change
//! has a number to move, never so a speedup can be asserted. Nothing here
//! fails a build.

use purrdf_n3::parser::parse_n3;
use purrdf_n3::proof_check_n3::check_proof;
use purrdf_n3::proof_writer::proof_to_n3;
use purrdf_n3::reasoner::{CompletionStatus, ReasonerOptions, ReasonerResult, reason};
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};

/// Build the flat document `ex:s{i} ex:p ex:o{j}` of `n` triples, as text, the
/// way a caller actually feeds the parser.
fn flat_document(n: usize) -> String {
    let mut text = String::with_capacity(n * 32 + 64);
    text.push_str("@prefix ex: <https://example.org/> .\n");
    for i in 0..n {
        text.push_str(&format!("ex:s{i} ex:p ex:o{i} .\n"));
    }
    text
}

/// The `p` chain `ex:n0 -> ex:n1 -> … -> ex:n{n}` of `n` links, written in
/// both directions so the text carries `2n` facts, plus the two rules whose
/// fixpoint closes `p` into `anc` transitively: every node reaches every
/// other, so the closure is `n * (n + 1)` derived facts and the run takes
/// many rounds.
fn chain_program(n: usize) -> String {
    let mut text = String::with_capacity(n * 32 + 256);
    text.push_str("@prefix ex: <https://example.org/> .\n");
    text.push_str("{ ?s ex:p ?o } => { ?s ex:anc ?o } .\n");
    text.push_str("{ ?s ex:p ?m . ?m ex:anc ?o } => { ?s ex:anc ?o } .\n");
    for i in 0..n {
        text.push_str(&format!("ex:n{i} ex:p ex:n{} .\n", i + 1));
        text.push_str(&format!("ex:n{} ex:p ex:n{i} .\n", i + 1));
    }
    text
}

/// Run one program to fixpoint, panicking rather than reporting a partial
/// answer — a bench that silently measured a limit refusal would be measuring
/// nothing.
fn reason_fixture(program: &str) -> ReasonerResult {
    let doc = parse_n3(program, None).expect("the fixture program parses");
    let result = reason(&doc, &ReasonerOptions::default());
    assert_eq!(
        result.status,
        CompletionStatus::Complete,
        "the fixture fixpoint converges inside every ceiling"
    );
    result
}

/// Parse the fixture, run the fixpoint with proofs on, write the proof to N3,
/// and verify it — panicking on any partial result. The proof bench measures
/// this whole operation: generation, serialization, verification.
fn proof_fixture(program: &str) {
    let doc = parse_n3(program, None).expect("the fixture program parses");
    let options = ReasonerOptions { proof: true, ..ReasonerOptions::default() };
    let result = reason(&doc, &options);
    assert_eq!(
        result.status,
        CompletionStatus::Complete,
        "the fixture fixpoint converges inside every ceiling"
    );
    let proof = proof_to_n3(&doc.prefixes, &result);
    assert!(!proof.is_empty(), "the fixture derives facts, so a proof exists");
    let report = check_proof(program, &proof, "bench-reasoner").expect("the proof reads back");
    assert!(
        report.valid(),
        "the fixture's proof verifies: {}",
        report.verdict()
    );
}

/// `parse_n3` over the flat fixture at two sizes: the per-triple parser cost
/// un-mixed from reasoning.
fn parse(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_parse");
    for n in [1_000_usize, 10_000] {
        let text = flat_document(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &text, |bch, text| {
            bch.iter(|| black_box(parse_n3(black_box(text), None)).is_ok());
        });
    }
    group.finish();
}

/// Forward chaining to convergence over the transitive-closure program: the
/// many-round fixpoint the reasoner exists for.
fn fixpoint(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_fixpoint");
    for n in [64_usize, 256] {
        let program = chain_program(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &program, |bch, program| {
            bch.iter(|| black_box(reason_fixture(black_box(program))));
        });
        // Hold one result so the closure size is visible in the report, not
        // inferred: 64 links close to 2080 derived facts, 256 to 32896.
        let result = reason_fixture(&program);
        let _ = black_box(result.closure.len());
    }
    group.finish();
}

/// Proof generation + `proof_to_n3` + `check_proof` verification as one
/// operation over the same fixpoint fixture.
fn proof(c: &mut Bench) {
    let mut group = c.benchmark_group("n3_proof");
    for n in [64_usize, 256] {
        let program = chain_program(n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &program, |bch, program| {
            bch.iter(|| proof_fixture(black_box(program)));
        });
    }
    group.finish();
}

bench_group!(benches, parse, fixpoint, proof);
bench_main!(benches);

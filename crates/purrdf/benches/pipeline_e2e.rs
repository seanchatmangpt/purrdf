// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! The buyer's number: one end-to-end pipeline over the facade's public API —
//! parse N-Triples text into the store, run one SPARQL SELECT, serialize the
//! results — timed as a single unit, then priced stage by stage.
//!
//! `multimodal_on_demand_read.rs` prices the retrieval composition; this file
//! prices the document pipeline a consumer actually ships: bytes in, answers
//! out. The group `pipeline_e2e` holds four rows:
//!
//! * **`end_to_end`** — the whole pipeline as ONE iteration:
//!   [`parse_dataset`] → one [`NativeSparqlEngine`] SELECT → SPARQL Results
//!   JSON `serialize` of the answer, with [`Throughput::Bytes`] set to the
//!   fixture size, so the row reads as MB/s of input document. This is the
//!   row a buyer asks for first.
//! * **`parse`** — [`parse_dataset`] alone. The facade's parse+store entry:
//!   parsing and freezing into the interned store are one call, so "store"
//!   has no separate row — it is priced inside `parse` and inside
//!   `end_to_end`.
//! * **`query`** — the SELECT alone over the already-stored dataset, with
//!   [`Throughput::Elements`] of one per iteration, so the row reads as
//!   queries per second.
//! * **`serialize`** — the SPARQL Results JSON writer alone over the
//!   already-computed answer, MB/s of serialized output bytes.
//!
//! The `query` and `serialize` stages run over fixtures their predecessors
//! produced ONCE, before the group starts, so each stage prices its own work
//! and never its predecessors'. `end_to_end` pays every stage every
//! iteration, which is why the four rows are not additive.
//!
//! The fixture — a seeded ~1 MB N-Triples document, the SELECT, and the
//! shared helpers — lives in `support/fixture.rs`, one definition shared with
//! `pipeline_alloc.rs`, the allocation probe that prices the same stages in
//! bytes instead of nanoseconds. That probe is a SEPARATE bench target
//! on purpose: its process installs the counting global allocator, whose
//! per-allocation bookkeeping must not sit under this bench's timed rows.
//!
//! Report-only, per this repository's rule: benches exist so a later change
//! has a number to move, never so a speedup can be asserted.

#[path = "support/fixture.rs"]
mod fixture;

use std::hint::black_box;
use std::sync::Arc;

use purrdf_testkit::bench::{Bench, Throughput, bench_group, bench_main};

use purrdf::sparql::{NativeSparqlEngine, QueryOptions};
use purrdf::{RdfDataset, parse_dataset};

use fixture::{
    NTRIPLES, build_fixture, select_request as fixture_select_request, serialize_json,
    solution_rows,
};

/// The request every stage runs: the fixture module's SELECT, imported so the
/// call sites below stay at one line.
fn select_request() -> purrdf::SparqlRequest<'static> {
    fixture_select_request()
}

fn pipeline(c: &mut Bench) {
    let fixture = build_fixture();
    let fixture_len = u64::try_from(fixture.len()).expect("the fixture size fits");
    let engine = NativeSparqlEngine::new();

    // The successors' fixtures, built ONCE so each stage prices only itself:
    // the stored dataset the `query` stage reads, and the answer the
    // `serialize` stage writes.
    let stored: Arc<RdfDataset> =
        parse_dataset(&fixture, NTRIPLES, None).expect("the fixture parses");
    let answer = engine
        .query_with_options_view(&stored, select_request(), QueryOptions::EMPTY)
        .expect("the fixture query evaluates");
    let answer_len = u64::try_from(serialize_json(&answer).len()).expect("the answer size fits");

    let mut group = c.benchmark_group("pipeline_e2e");

    // The buyer row: the whole pipeline as ONE iteration, MB/s of input
    // document.
    group.throughput(Throughput::Bytes(fixture_len));
    group.bench_function("end_to_end", |b| {
        b.iter(|| {
            let dataset = parse_dataset(&fixture, NTRIPLES, None).expect("the fixture parses");
            let result = engine
                .query_with_options_view(&dataset, select_request(), QueryOptions::EMPTY)
                .expect("the fixture query evaluates");
            let document = serialize_json(&result);
            black_box(document.len())
        });
    });

    // Stage one: parse + store, MB/s of input document.
    group.bench_function("parse", |b| {
        b.iter(|| black_box(parse_dataset(&fixture, NTRIPLES, None).expect("the fixture parses")));
    });

    // Stage two: the query alone, queries per second.
    group.throughput(Throughput::Elements(1));
    group.bench_function("query", |b| {
        b.iter(|| {
            black_box(solution_rows(
                engine
                    .query_with_options_view(&stored, select_request(), QueryOptions::EMPTY)
                    .expect("the fixture query evaluates"),
            ))
        });
    });

    // Stage three: SPARQL Results JSON out, MB/s of output document.
    group.throughput(Throughput::Bytes(answer_len));
    group.bench_function("serialize", |b| {
        b.iter(|| black_box(serialize_json(&answer).len()));
    });

    group.finish();
}

bench_group!(benches, pipeline);
bench_main!(benches);

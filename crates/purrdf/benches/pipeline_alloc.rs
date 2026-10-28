// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// This bench is a plain `main`, but the workspace `missing_docs` lint applies to
// its items just the same; the reporting helpers below are internal probes, not
// API.
#![allow(missing_docs)]

//! Peak-allocation evidence for the facade's document pipeline — the same
//! parse/query/serialize stages `pipeline_e2e` prices in nanoseconds, priced
//! here in bytes — in a separate process from the timed bench so the tracking
//! allocator's atomics do not skew its latency (mirrors `pack_input_alloc`).
//!
//! For each stage it reports two DISTINCT numbers, on purpose:
//!
//! * **`peak_allocated_bytes`** — the high-water mark of live bytes during ONE
//!   operation, read from a whole-process window of the workspace's shared
//!   instrument. Memory-mapped pages never pass through the allocator, so this
//!   does NOT include an `mmap`.
//! * **`rss_delta_kb`** — the change in the process resident set
//!   (`/proc/self/statm`), which DOES include memory-mapped pages the allocator
//!   never sees, and which is read here because it is an OS figure no allocator
//!   ledger can supply. On non-Linux hosts `resident_kib` is Linux-only
//!   and returns a constant 0, so `rss_delta_kb=0` rows are platform
//!   absences, not measurements.
//!
//! The two together tell the real story: one parse into the interned store has
//! a large *allocated* peak, while the serialized answer's peak is the writer's
//! buffer, not the dataset. Conflating the two numbers would hide exactly the
//! trade-off this benchmark exists to quantify.
//!
//! Each stage is measured over ONE operation, not an iteration loop: the
//! question is how much memory one pipeline costs, which a loop's allocator
//! recycling would hide behind the first iteration. The `query` and `serialize`
//! windows run over fixtures their predecessors produced ONCE, before any
//! window opens, so each stage's peak is its own work and never its
//! predecessors'. The `end_to_end` window pays every stage, which is why the
//! four rows are not additive.
//!
//! The fixture is the `support/fixture.rs` module `pipeline_e2e.rs` uses — one
//! definition of the document, never a second body.
//!
//! Report-only, per this repository's rule.

#[path = "support/fixture.rs"]
mod fixture;

use std::hint::black_box;
use std::sync::Arc;

use purrdf::parse_dataset;
use purrdf::sparql::{NativeSparqlEngine, QueryOptions};
use purrdf_alloc_probe::{
    CountingAllocator, WholeProcessWindow, report_peak, resident_kib as rss_kb,
};

use fixture::{NTRIPLES, build_fixture, select_request, serialize_json, solution_rows};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn report(label: &str, peak_allocated_bytes: i64, rss_before_kb: u64, rss_after_kb: u64) {
    report_peak(
        "pipeline_alloc",
        label,
        peak_allocated_bytes,
        rss_before_kb,
        rss_after_kb,
    );
}

fn main() {
    let fixture = build_fixture();
    println!(
        "[pipeline_alloc] fixture document is {} bytes",
        fixture.len()
    );
    let engine = NativeSparqlEngine::new();

    // The successors' fixtures, built ONCE so each stage's window prices only
    // itself: the stored dataset the `query` stage reads, and the answer the
    // `serialize` stage writes. Built OUTSIDE any window, so their cost lands
    // in no stage's ledger.
    let stored: Arc<purrdf::RdfDataset> =
        parse_dataset(&fixture, NTRIPLES, None).expect("the fixture parses");
    let answer = engine
        .query_with_options_view(&stored, select_request(), QueryOptions::EMPTY)
        .expect("the fixture query evaluates");
    let answer_len = serialize_json(&answer).len();

    // Stage one: parse + store. Parsing and freezing into the interned store
    // are one call, so "store" has no separate row — it is priced inside
    // `parse` and inside `end_to_end`.
    {
        let rss_before = rss_kb();
        let window = WholeProcessWindow::open();
        black_box(parse_dataset(&fixture, NTRIPLES, None).expect("the fixture parses"));
        let peak = window.close().peak_working_bytes;
        report("parse", peak, rss_before, rss_kb());
    }

    // Stage two: the query alone, over the already-stored dataset.
    {
        let rss_before = rss_kb();
        let window = WholeProcessWindow::open();
        black_box(solution_rows(
            engine
                .query_with_options_view(&stored, select_request(), QueryOptions::EMPTY)
                .expect("the fixture query evaluates"),
        ));
        let peak = window.close().peak_working_bytes;
        report("query", peak, rss_before, rss_kb());
    }

    // Stage three: SPARQL Results JSON out, over the already-computed answer.
    {
        let rss_before = rss_kb();
        let window = WholeProcessWindow::open();
        black_box(serialize_json(&answer).len());
        let peak = window.close().peak_working_bytes;
        report("serialize", peak, rss_before, rss_kb());
    }

    // The buyer row: the whole pipeline as ONE operation, over the original
    // bytes. Not additive with the stage rows above — those ran over prepared
    // fixtures, this one pays every stage.
    {
        let rss_before = rss_kb();
        let window = WholeProcessWindow::open();
        let dataset = parse_dataset(&fixture, NTRIPLES, None).expect("the fixture parses");
        let result = engine
            .query_with_options_view(&dataset, select_request(), QueryOptions::EMPTY)
            .expect("the fixture query evaluates");
        let document = serialize_json(&result);
        black_box(document.len());
        let peak = window.close().peak_working_bytes;
        report("end_to_end", peak, rss_before, rss_kb());
    }

    println!(
        "[pipeline_alloc] fixture {fixture_bytes} bytes, answer {answer_len} bytes",
        fixture_bytes = fixture.len(),
    );
}

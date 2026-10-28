// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The canon-arrow egress arm is residency-bounded: it never holds the answer
//! it is writing (OPT-17).
//!
//! Mirrors `results_streaming_is_bounded.rs`'s
//! `the_construct_arm_does_not_hold_the_document_it_is_writing`: a write-watching
//! sink cannot see residency, because an intermediate `String` drains through the
//! staging window in exactly the bounded pieces a streamed document does. Only the
//! allocator can tell them apart, so the oracle is `CurrentThreadWindow` — the
//! correct window in a test binary, where the harness runs tests concurrently over
//! one global allocator and a whole-process window would absorb sibling traffic.
//!
//! The writer module (`src/canon_arrow.rs`) is landed by a sibling lane against the
//! public surface already wired in `lib.rs` (`SparqlResultsFormat::CanonArrow` ->
//! `canon_arrow::write_canon_arrow`). This test routes through that public surface
//! only.
//!
//! EXPECTED-TO-COMPILE-WITH-WRITER: at the time this file was written,
//! `src/canon_arrow.rs` was not yet on disk and the crate did not compile. No
//! assertion here depends on the writer's internals — only on
//! `serialize_into` + `SparqlResultsFormat::CanonArrow` + `purrdf_arrow`'s
//! header decode, all of which are on disk.

use purrdf_alloc_probe::{CountingAllocator, CurrentThreadWindow};
use purrdf_arrow::ArrowEgressHeader;
use purrdf_core::sink::DRAIN_BUFFER_BYTES;
use purrdf_core::{SparqlResult, TermValue};
use purrdf_sparql_results::{ResultProvenance, SparqlResultsFormat, serialize, serialize_into};
use std::io::Write;

/// Registered here, not by the crate: `#[global_allocator]` is one per binary.
#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// A SELECT answer far larger than one batch and one drain window. Same fixture
/// shape as the streaming court's `solutions_result`, at 4096 rows.
fn solutions_result(rows: usize) -> SparqlResult {
    SparqlResult::Solutions {
        variables: vec!["subject".to_owned(), "label".to_owned()],
        rows: (0..rows)
            .map(|i| {
                vec![
                    Some(TermValue::Iri(format!(
                        "https://example.org/subject/{i}/with/a/deliberately/long/path"
                    ))),
                    Some(TermValue::Literal {
                        lexical_form: format!(
                            "row {i} with \"quoted\" text, padded out so the document is \
                             far larger than one batch slab"
                        ),
                        datatype: "http://www.w3.org/2001/XMLSchema#string".to_owned(),
                        language: None,
                        direction: None,
                    }),
                ]
            })
            .collect(),
        aux: purrdf_core::RdfDatasetBuilder::new()
            .freeze()
            .expect("empty aux dataset")
            .into(),
    }
}

/// How many source rows one canon-arrow batch slab carries.
///
/// The plan (OPT-17) writes "one or more zero-copy record-batch slabs"; the
/// residency claim is per-slab, so the bound is stated against a batch of this
/// many rows whatever the writer's actual batching. 64 rows of this fixture at
/// ~150 rendered bytes/row is roughly 10 KiB — far below one drain window, so
/// the sink buffer dominates the allowance and the assertion separates the
/// regimes (one batch vs. the whole 4096-row answer) by orders of magnitude.
const BATCH_ROWS: usize = 64;

#[test]
fn canon_arrow_streaming_holds_only_one_batch() {
    let ROWS: usize = 4_096;
    let result = solutions_result(ROWS);
    let provenance = ResultProvenance::default();
    let format = SparqlResultsFormat::CanonArrow;

    // How big the answer actually is, measured outside the window. `serialize`
    // is the eager spelling; its length is the regime separator, not the oracle.
    let document = serialize(&result, format, &provenance, None)
        .expect("canon-arrow carries a SELECT answer")
        .bytes
        .len();
    assert!(
        document > 8 * DRAIN_BUFFER_BYTES,
        "the fixture must be many drain windows long or this proves nothing; \
         {document} bytes"
    );

    // --- The output is a real canon-arrow frame (parses independently). ---
    let mut output = Vec::new();
    struct VecSink<'a>(&'a mut Vec<u8>);
    impl Write for VecSink<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = VecSink(&mut output);
    let outcome = serialize_into(&result, format, &provenance, None, &mut sink)
        .expect("canon-arrow streams a SELECT answer");
    assert_eq!(
        outcome.bytes_written as usize,
        output.len(),
        "the reported count must agree with what reached the sink"
    );

    assert_eq!(
        &output[0..8],
        &ArrowEgressHeader::MAGIC,
        "the frame must open with the canon-arrow magic"
    );
    let header_bytes: [u8; ArrowEgressHeader::SIZE] = output[0..ArrowEgressHeader::SIZE]
        .try_into()
        .expect("the output carries at least one full 128-byte header");
    let header = ArrowEgressHeader::from_bytes(&header_bytes)
        .expect("the streamed frame's header must decode");
    assert!(
        header.fits_in(output.len() as u64),
        "the header claims {} total bytes against a {}-byte frame: the streamed \
         output does not satisfy its own envelope",
        header.total_byte_len,
        output.len()
    );

    // --- Residency: the window never holds more than one batch's slab above
    // --- the sink buffer.
    //
    // A write-watching sink is blind here (see the module comment), so the
    // allocator is the oracle. `serialize_into` already wraps its writer in the
    // crate's `TextSink` staging window, and the window's allocator sees that
    // buffer too — hence the drain-window term in the allowance.
    //
    // One batch's slab, estimated from the fixture itself (measured, not
    // assumed): the eager document carries `ROWS` rows, so one batch's share of
    // it is `document / ROWS * BATCH_ROWS`. Generous by design — the claim is
    // that peak residency does not TRACK the answer, not that it hits a number.
    // Holding the whole document puts the peak at or above `document`, so the
    // gap between one batch's share (plus one staging window) and the document
    // is orders of magnitude wide.
    let one_batch_estimate = document / ROWS * BATCH_ROWS;
    let allowance = (one_batch_estimate + DRAIN_BUFFER_BYTES) * 2;

    let window = CurrentThreadWindow::open();
    let streamed = serialize_into(&result, format, &provenance, None, &mut std::io::sink())
        .expect("canon-arrow streams a SELECT answer");
    let measured = window.close();

    assert_eq!(
        streamed.bytes_written as usize, document,
        "the streamed and eager spellings must agree on length"
    );
    assert!(
        measured.peak_working_bytes < allowance as i64,
        "canon-arrow egress peaked at {} live bytes writing a {document}-byte \
         document (one batch's share is ~{one_batch_estimate} bytes + one \
         {DRAIN_BUFFER_BYTES}-byte staging window, allowance {allowance}): \
         residency is tracking the answer, so something is holding the whole \
         frame it is writing",
        measured.peak_working_bytes
    );
}

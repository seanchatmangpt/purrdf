// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Native round-trip court for the `queryCanonArrow` egress payload (OPT-17).
//!
//! The JS surface `queryCanonArrow` is the wasm-bindgen shim over
//! [`purrdf_sparql_results::serialize`] with `SparqlResultsFormat::CanonArrow`
//! — the same `serialize_query_result` path the binding's native unit tests
//! exercise. wasm-bindgen tests would need the Node/wasm-bindgen harness that
//! the repo runs as the dedicated CI wasm lane
//! (`crates/rdf-wasm/js/tests`, run against the wasm-bindgen output); this
//! court pins the Rust-side byte contract that shim hands back, so the JS
//! boundary on CI exercises an already-pinned frame, not a hope.
//!
//! Contract under test (OPT-17 §3/§4):
//!
//! - the frame's first 8 bytes are the wire magic `b"GLAWARRW"`;
//! - the leading 128 bytes decode through
//!   [`purrdf_arrow::ArrowEgressHeader::from_bytes`];
//! - the decoded header validates the frame length with
//!   [`purrdf_arrow::ArrowEgressHeader::fits_in`];
//! - a non-tabular result (ASK) emits the fixed 192-byte canon-empty frame
//!   (128-byte header + 64-byte trailing padding), both digests set to the
//!   domain digest [`purrdf_arrow::canon_arrow_empty_v1`].

// NOTE (OPT-17 status, 2026-10-27): the wasm-bindgen shim `queryCanonArrow`
// is wired in `crates/rdf-wasm/src/query.rs` (exposed through
// js/index.mjs + index.d.ts). This court pins the underlying Rust path the
// shim routes through —
// `purrdf_sparql_results::serialize(.., SparqlResultsFormat::CanonArrow, ..)`
// — which is exactly the `serialize_query_result` arm the shim routes
// through. When the JS boundary lands, the Node lane should assert the same
// three byte facts on the ArrayBuffer it receives.

use purrdf_arrow::ArrowEgressHeader;
use purrdf_sparql_results::{
    ResultProvenance, SparqlResult, SparqlResultsFormat, serialize,
};

/// The wire magic every canon-arrow frame starts with (`GLAWARRW`).
const MAGIC: &[u8; 8] = b"GLAWARRW";

/// Fixed 192-byte canon-empty frame: 128-byte header + 64-byte padding.
const CANON_EMPTY_FRAME_LEN: u64 = 192;

fn canon_arrow_bytes(result: &SparqlResult) -> Vec<u8> {
    let outcome = serialize(
        result,
        SparqlResultsFormat::CanonArrow,
        &ResultProvenance::default(),
        None,
    )
    .expect("canon-arrow egress of a tabular/boolean result succeeds");
    assert!(
        !outcome.provenance_dropped,
        "empty default provenance is carried by the frame, not dropped"
    );
    outcome.bytes
}

/// The `queryCanonArrow` byte contract: magic prefix, header decode, `fits_in`.
#[test]
fn canon_arrow_frame_opens_with_glawarrw_and_header_fits_in() {
    let ask = SparqlResult::Boolean(true);
    let frame = canon_arrow_bytes(&ask);

    // 1. First 8 bytes are the wire magic.
    assert_eq!(
        &frame[0..8],
        MAGIC,
        "canon-arrow frame must open with the GLAWARRW wire magic"
    );

    // 2. The leading 128 bytes decode as the egress header.
    let header_bytes: [u8; 128] = frame[0..128]
        .try_into()
        .expect("frame is at least one header long");
    let header = ArrowEgressHeader::from_bytes(&header_bytes)
        .expect("the frame's own header validates");

    // 3. `fits_in` accepts exactly the frame the header describes.
    assert_eq!(
        header.total_byte_len,
        CANON_EMPTY_FRAME_LEN,
        "ASK emits the fixed canon-empty frame: 128-byte header + 64-byte padding"
    );
    assert!(
        header.fits_in(frame.len() as u64),
        "fits_in(frame.len()) must be true: the header describes exactly this frame"
    );
    assert!(
        !header.fits_in(CANON_EMPTY_FRAME_LEN - 1),
        "fits_in is a real bound: 191 bytes cannot hold the frame it describes"
    );

    // The non-tabular frame carries the canon-empty domain digest in BOTH
    // digest fields (triples_digest [36..68], columnar_digest [68..100]).
    assert_eq!(
        header.triples_digest,
        *purrdf_arrow::canon_arrow_empty_v1(),
        "ASK's triples_digest is the canon-empty domain digest"
    );
}

/// The exact ASK case the OPT-17 plan names: a 192-byte frame end to end.
#[test]
fn ask_result_emits_the_192_byte_canon_empty_frame() {
    let frame = canon_arrow_bytes(&SparqlResult::Boolean(false));

    assert_eq!(
        frame.len(),
        CANON_EMPTY_FRAME_LEN as usize,
        "ASK frame is exactly 192 bytes: 128-byte header + 64-byte trailing padding"
    );
    assert_eq!(&frame[0..8], MAGIC);
    assert!(
        frame[128..].iter().all(|&byte| byte == 0),
        "the 64-byte trailing region is zero padding"
    );

    let header_bytes: [u8; 128] = frame[0..128]
        .try_into()
        .expect("128 of 192 bytes is exactly one header");
    let header =
        ArrowEgressHeader::from_bytes(&header_bytes).expect("ASK header round-trips");
    assert!(header.fits_in(frame.len() as u64));
    assert_eq!(
        header.triples_digest,
        *purrdf_arrow::canon_arrow_empty_v1(),
        "both digests are the domain digest for a non-tabular result"
    );
    assert_eq!(
        header.columnar_digest,
        *purrdf_arrow::canon_arrow_empty_v1(),
        "the columnar digest carries the same 32-byte domain digest"
    );
}

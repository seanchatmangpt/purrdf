// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Golden bytes for the canon-arrow egress frame (OPT-17), following the
//! `crates/columnar/tests/golden_bytes.rs` house pattern: golden functions with
//! explicit expected values, not snapshot files.
//!
//! The values are derived, not frozen digest literals: the canon-empty frame's
//! two digests are the domain digest, and the solutions frame's digests are
//! recomputed here from their documented inputs — the triples witness stream
//! and the emitted payload region — so a change that moves the bytes moves an
//! EXPECTED value, which the byte-determinism contract forbids without a
//! golden update and its reason.
//!
//! The slab vocabulary pinned here is the writer's (`canon_arrow.rs`,
//! `mod kind` + `emit_solutions`): per column in projection order —
//! `rows` term-kind bytes (`IRI=0`, `BLANK_NODE=1`, `LITERAL=2`, `TRIPLE=3`,
//! `UNBOUND=0xFF`), then `(rows + 1)` little-endian `u32` offsets into the
//! column's lexical bytes, then the concatenated canonical N-Triples
//! spellings. Batches sit back-to-back in the payload region, which ends with
//! 64 zero bytes of trailing padding. The header field offsets are
//! `crates/arrow/src/header.rs`'s wire order.

use purrdf_arrow::{ArrowEgressHeader, canon_arrow_empty_v1};
use purrdf_core::{BlankScope, RdfDatasetBuilder, TermValue};
use purrdf_hash::blake3;
use purrdf_sparql_results::{
    ResultProvenance, SparqlResult, SparqlResultsFormat, serialize, serialize_into,
};

/// Header wire offsets, named once so every assertion reads the same map.
mod off {
    pub const MAGIC: std::ops::Range<usize> = 0..8;
    pub const VERSION: std::ops::Range<usize> = 8..10;
    pub const FLAGS: std::ops::Range<usize> = 10..12;
    pub const BATCH_COUNT: std::ops::Range<usize> = 12..16;
    pub const PAYLOAD_OFFSET: std::ops::Range<usize> = 16..20;
    pub const PAYLOAD_LEN: std::ops::Range<usize> = 20..28;
    pub const TOTAL_LEN: std::ops::Range<usize> = 28..36;
    pub const TRIPLES_DIGEST: std::ops::Range<usize> = 36..68;
    pub const COLUMNAR_DIGEST: std::ops::Range<usize> = 68..100;
    pub const RESERVED: std::ops::Range<usize> = 100..128;
    pub const PAYLOAD_REGION: usize = 128;
    /// The trailing frame padding.
    pub const PADDING: usize = 64;
}

/// `SparqlResult::Solutions` with the given variables and rows over an empty
/// aux dataset.
fn solutions(variables: Vec<String>, rows: Vec<Vec<Option<TermValue>>>) -> SparqlResult {
    SparqlResult::Solutions {
        variables,
        rows,
        aux: RdfDatasetBuilder::new()
            .freeze()
            .expect("empty aux dataset")
            .into(),
    }
}

fn iri(value: &str) -> TermValue {
    TermValue::Iri(value.to_string())
}

/// A simple (`xsd:string`) literal, which the term writer emits bare.
fn plain(lex: &str) -> TermValue {
    TermValue::Literal {
        lexical_form: lex.to_string(),
        datatype: "http://www.w3.org/2001/XMLSchema#string".to_string(),
        language: None,
        direction: None,
    }
}

fn blank(label: &str) -> TermValue {
    TermValue::Blank {
        label: label.to_string(),
        scope: BlankScope(0),
    }
}

/// A 2x2 SELECT result whose every cell's canonical N-Triples spelling is a
/// known constant:
/// `s` = `<http://example.org/alice>` (26 bytes), `_:b0` (4 bytes);
/// `o` = `"Alice"` (7 bytes), `"Bob"` (5 bytes).
fn two_by_two() -> SparqlResult {
    solutions(
        vec!["s".to_string(), "o".to_string()],
        vec![
            vec![Some(iri("http://example.org/alice")), Some(plain("Alice"))],
            vec![Some(blank("b0")), Some(plain("Bob"))],
        ],
    )
}

/// The little-endian `u32` offset table, flattened.
fn offsets(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// The canon-empty frame: 192 bytes, both digests the domain digest, ONE
/// empty batch (the writer's Default contract), empty payload, 64 zero bytes
/// of trailing padding.
fn the_ask_frame_pins_the_exact_192_byte_envelope() {
    for boolean in [true, false] {
        let frame = serialize(
            &SparqlResult::Boolean(boolean),
            SparqlResultsFormat::CanonArrow,
            &ResultProvenance::default(),
            None,
        )
        .expect("ASK serializes to the canon-arrow frame")
        .bytes;
        assert_eq!(frame.len(), 192, "header 128 + payload 0 + padding 64");

        // Header, byte for byte at the wire offsets.
        assert_eq!(&frame[off::MAGIC], b"GLAWARRW");
        assert_eq!(&frame[off::VERSION], &0x0002_u16.to_le_bytes());
        assert_eq!(&frame[off::FLAGS], &0_u16.to_le_bytes());
        assert_eq!(&frame[off::BATCH_COUNT], &1_u32.to_le_bytes());
        assert_eq!(&frame[off::PAYLOAD_OFFSET], &128_u32.to_le_bytes());
        assert_eq!(&frame[off::PAYLOAD_LEN], &0_u64.to_le_bytes());
        assert_eq!(&frame[off::TOTAL_LEN], &192_u64.to_le_bytes());

        // Both digests are the domain digest, and therefore each other.
        let empty = *canon_arrow_empty_v1();
        assert_eq!(&frame[off::TRIPLES_DIGEST], &empty[..]);
        assert_eq!(&frame[off::COLUMNAR_DIGEST], &empty[..]);
        assert_eq!(empty, *blake3::hash(b"CANON_ARROW_EMPTY_V1").as_bytes());

        // Reserved region and the empty payload region are zero-filled.
        assert!(frame[off::RESERVED].iter().all(|&b| b == 0));
        assert!(frame[off::PAYLOAD_REGION..].iter().all(|&b| b == 0));

        // The frame decodes through the shared wire header...
        let header = ArrowEgressHeader::from_bytes(
            frame[..ArrowEgressHeader::SIZE]
                .try_into()
                .expect("header slice"),
        )
        .expect("the canon-empty frame round-trips");
        assert_eq!(header.batch_count, 1);
        assert_eq!(header.payload_byte_len, 0);
        assert_eq!(header.total_byte_len, 192);
        assert_eq!(header.triples_digest, empty);
        assert_eq!(header.columnar_digest, empty);

        // ...and `fits_in` gates the payload slice at exactly the total.
        assert!(header.fits_in(192));
        assert!(!header.fits_in(191));
    }
}

/// A zero-row SELECT is non-tabular: identical bytes to the ASK frame.
fn a_zero_row_select_emits_the_canon_empty_frame() {
    let frame = serialize(
        &solutions(vec!["s".to_string()], Vec::new()),
        SparqlResultsFormat::CanonArrow,
        &ResultProvenance::default(),
        None,
    )
    .expect("a zero-row SELECT serializes")
    .bytes;
    let ask = serialize(
        &SparqlResult::Boolean(true),
        SparqlResultsFormat::CanonArrow,
        &ResultProvenance::default(),
        None,
    )
    .expect("ASK serializes")
    .bytes;
    assert_eq!(frame, ask);
}

/// The 2x2 solutions frame: header fields, first-slab byte offsets, the exact
/// term-kind bytes and offset tables, decode, and digest recompute.
///
/// Layout arithmetic (slab offsets, single batch of 2 rows, 2 columns):
///
/// - column `s`: kinds `[IRI=0, BLANK_NODE=1]` at 0..2; offsets `[0, 26, 30]`
///   at 2..14; lexical `<http://example.org/alice>` + `_:b0` at 14..44;
/// - column `o`: kinds `[LITERAL=2, LITERAL=2]` at 44..46; offsets
///   `[0, 7, 12]` at 46..58; lexical `"Alice"` + `"Bob"` at 58..70.
///
/// Payload = slab 68 + padding 64 = 132; total = 128 + 132 = 260.
fn the_two_by_two_frame_pins_the_slab_bytes() {
    let frame = serialize(
        &two_by_two(),
        SparqlResultsFormat::CanonArrow,
        &ResultProvenance::default(),
        None,
    )
    .expect("SELECT serializes to the canon-arrow frame")
    .bytes;
    assert_eq!(frame.len(), 260);

    // Header: one batch, payload 132 (slab 68 + padding 64), total 260.
    assert_eq!(&frame[off::MAGIC], b"GLAWARRW");
    assert_eq!(&frame[off::VERSION], &0x0002_u16.to_le_bytes());
    assert_eq!(&frame[off::FLAGS], &0_u16.to_le_bytes());
    assert_eq!(&frame[off::BATCH_COUNT], &1_u32.to_le_bytes());
    assert_eq!(&frame[off::PAYLOAD_OFFSET], &128_u32.to_le_bytes());
    assert_eq!(&frame[off::PAYLOAD_LEN], &132_u64.to_le_bytes());
    assert_eq!(&frame[off::TOTAL_LEN], &260_u64.to_le_bytes());

    // First slab, column `s`: the term-kind bytes and the offset table.
    let slab = off::PAYLOAD_REGION;
    assert_eq!(&frame[slab..slab + 2], &[0x00, 0x01], "s kinds: IRI, BLANK_NODE");
    assert_eq!(&frame[slab + 2..slab + 14], &offsets(&[0, 26, 30])[..]);
    assert_eq!(
        &frame[slab + 14..slab + 44],
        b"<http://example.org/alice>_:b0"
    );

    // First slab, column `o`.
    assert_eq!(
        &frame[slab + 44..slab + 46],
        &[0x02, 0x02],
        "o kinds: LITERAL, LITERAL"
    );
    assert_eq!(&frame[slab + 46..slab + 58], &offsets(&[0, 7, 12])[..]);
    assert_eq!(&frame[slab + 58..slab + 70], b"\"Alice\"\"Bob\"");

    // The 64-byte trailing padding closes the frame.
    assert!(frame[260 - off::PADDING..].iter().all(|&b| b == 0));

    // Decode through the shared wire header.
    let header = ArrowEgressHeader::from_bytes(
        frame[..ArrowEgressHeader::SIZE]
            .try_into()
            .expect("header slice"),
    )
    .expect("the solutions frame round-trips");
    assert_eq!(header.batch_count, 1);
    assert_eq!(header.payload_offset, ArrowEgressHeader::PAYLOAD_OFFSET);
    assert_eq!(header.payload_byte_len, 132);
    assert_eq!(header.total_byte_len, 260);
    assert!(header.fits_in(frame.len() as u64));
    assert!(!header.fits_in(frame.len() as u64 - 1));

    // Digest recompute on decode: the columnar digest is BLAKE3 over the
    // whole emitted payload region (slab + padding), and the triples digest
    // is BLAKE3 over the bound cells' N-Triples spellings in row-major order,
    // each followed by a 0x00 separator.
    let mut columnar = blake3::Hasher::new();
    columnar.update(&frame[off::PAYLOAD_REGION..]);
    assert_eq!(header.columnar_digest, *columnar.finalize().as_bytes());

    let mut triples = blake3::Hasher::new();
    for token in [
        "<http://example.org/alice>",
        "_:b0",
        "\"Alice\"",
        "\"Bob\"",
    ] {
        triples.update(token.as_bytes());
        triples.update(&[0]);
    }
    assert_eq!(header.triples_digest, *triples.finalize().as_bytes());
}

/// The streaming lane writes the same bytes as the eager convenience: the
/// frame is a pure function of the result, whichever entry point emits it.
fn the_streaming_lane_writes_the_eager_bytes() {
    let eager = serialize(
        &two_by_two(),
        SparqlResultsFormat::CanonArrow,
        &ResultProvenance::default(),
        None,
    )
    .expect("eager serialization")
    .bytes;
    let mut streamed = Vec::new();
    serialize_into(
        &two_by_two(),
        SparqlResultsFormat::CanonArrow,
        &ResultProvenance::default(),
        None,
        &mut streamed,
    )
    .expect("streamed serialization");
    assert_eq!(streamed, eager);
}

purrdf_testkit::harness_main!(
    the_ask_frame_pins_the_exact_192_byte_envelope,
    a_zero_row_select_emits_the_canon_empty_frame,
    the_two_by_two_frame_pins_the_slab_bytes,
    the_streaming_lane_writes_the_eager_bytes,
);

// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The canon-arrow egress arm: a [`SparqlResult`] as a framed CanonArrow
//! columnar payload (OPT-17), written through the shared 128-byte
//! [`ArrowEgressHeader`] — no second framing authority.
//!
//! Shape routing:
//!
//! - `Solutions` encodes its rows as record-batch slabs under the slab-layout
//!   vocabulary below — one slab per [`ROWS_PER_BATCH`] rows.
//! - `Boolean`, `Graph`, and a `Solutions` with no rows or no projected
//!   variables are non-tabular: they emit
//!   the fixed 192-byte canon-empty frame (header + 64 bytes of trailing
//!   padding) with both digests set to the domain digest
//!   [`purrdf_arrow::canon_arrow_empty_v1`].
//!
//! # Allocation law (OPT-17)
//!
//! The streaming lane ([`write_canon_arrow`], the `serialize`/`serialize_into`
//! arm) holds ZERO document-sized allocations: the header is a fixed
//! `[u8; 128]`, and each batch slab is staged into a buffer bounded by
//! [`ROWS_PER_BATCH`] rows before it is handed to the sink, whose own staging
//! window is the fixed `purrdf_core::sink::DRAIN_BUFFER_BYTES`.
//!
//! The ONE deliberate exception is the eager convenience [`to_canon_arrow`]:
//! it returns a single contiguous `Vec<u8>` — a document-sized allocation by
//! construction, for FFI contiguity (a C/JS caller needs one pointer, not a
//! header plus N slab pointers). It is exempt from the zero-document-allocation
//! invariant, which binds only the streaming lane.
//!
//! The fixed 128-byte payload offset forces the header onto the wire FIRST,
//! but the header carries the payload's BLAKE3 digest. The lane therefore
//! walks the rows in three bounded passes — measure, digest, emit — each
//! staging at most one slab, so no pass ever holds two, let alone the payload.
//!
//! # Determinism
//!
//! The frame is a pure function of the result: variable names, row order and
//! canonical N-Triples term spellings. No timestamps, no environment, no
//! iteration-order-dependent data.
//!
//! # Examples
//!
//! ```
//! use purrdf_sparql_results::{SparqlResult, to_canon_arrow};
//!
//! let frame = to_canon_arrow(&SparqlResult::Boolean(true))
//!     .expect("ASK emits the canon-empty frame");
//! assert_eq!(frame.len(), 128 + 64);
//!
//! let header = purrdf_arrow::ArrowEgressHeader::from_bytes(
//!     frame[..128].try_into().expect("header slice"),
//! )
//! .expect("the canon-empty frame round-trips through the wire header");
//! assert_eq!(
//!     header.triples_digest,
//!     *purrdf_arrow::canon_arrow_empty_v1()
//! );
//! ```

use crate::error::Error;
use crate::term::ntriples_token;
use purrdf_arrow::ArrowEgressHeader;
use purrdf_core::sink::TextSink;
use purrdf_core::{SparqlResult, TermValue};
use purrdf_hash::blake3;

/// Rows per record-batch slab. The streaming lane's slab staging buffer is
/// bounded by this constant times the per-row lexical size — never by the
/// result.
pub(crate) const ROWS_PER_BATCH: usize = 64;

/// Trailing frame padding in bytes.
const TRAILING_PADDING: u64 = 64;

/// The slab column vocabulary: the per-batch encoding a
/// `Solutions` slab uses, laid out per projected variable in projection order.
///
/// This is the WIRE term-kind encoding, owned here by its only consumer; it
/// is deliberately not a `purrdf_arrow` storage-class vocabulary: that
/// described layout algebra no code uses, and the two axes (logical term
/// kind vs physical storage class) do not correspond.
///
/// For a batch of `r` rows and `c` columns, the slab is the concatenation, for
/// each column in order, of:
///
/// 1. `r` term-kind bytes — one [`kind::IRI`]/[`kind::BLANK_NODE`]/[`kind::LITERAL`]/[`kind::TRIPLE`]
///    discriminant per row, or [`kind::UNBOUND`] (`0xFF`) for a `None` cell;
/// 2. `r + 1` little-endian `u32` offsets into the column's lexical bytes —
///    monotone; for an unbound row the entry repeats its predecessor, so the
///    empty range `[off, off)` marks the missing cell;
/// 3. the column's lexical bytes — the canonical N-Triples spelling of each
///    bound cell, concatenated.
///
/// Batches are laid back-to-back inside the payload region; the payload ends
/// with [`TRAILING_PADDING`] zero bytes.
mod kind {
    /// An IRI cell.
    pub const IRI: u8 = 0;
    /// A blank-node cell.
    pub const BLANK_NODE: u8 = 1;
    /// A literal cell.
    pub const LITERAL: u8 = 2;
    /// A nested triple-term cell.
    pub const TRIPLE: u8 = 3;
    /// The `None` sentinel: the cell is unbound.
    pub const UNBOUND: u8 = 0xFF;
}

/// The term-kind discriminant of a bound cell.
fn term_kind(value: &TermValue) -> u8 {
    match value {
        TermValue::Iri(_) => kind::IRI,
        TermValue::Blank { .. } => kind::BLANK_NODE,
        TermValue::Literal { .. } => kind::LITERAL,
        TermValue::Triple { .. } => kind::TRIPLE,
    }
}

/// The canonical N-Triples spelling of a bound cell, through the crate's one
/// term writer — the same spelling every W3C results document carries, so the
/// columnar witness and the text formats cannot disagree about a term.
fn lexical(value: &TermValue) -> Result<String, Error> {
    ntriples_token(value)
}

/// The typed refusal for a count the slab vocabulary cannot address.
fn storage_overflow() -> Error {
    Error::Storage(purrdf_lex::allocation::StorageError::SizeOverflow)
}

/// `payload + 128 header + 64 trailing padding` — the wire contract
/// `ArrowEgressHeader::from_bytes` validates by subtraction.
fn frame_total(payload_byte_len: u64) -> u64 {
    (ArrowEgressHeader::SIZE as u64)
        .checked_add(payload_byte_len)
        .and_then(|sum| sum.checked_add(TRAILING_PADDING))
        .expect("a measured payload plus the fixed frame overhead cannot overflow u64")
}

/// The non-tabular frame: a default header whose two digests are the domain
/// digest, then the trailing padding — 192 bytes total.
fn canon_empty_frame() -> [u8; 192] {
    let mut frame = [0_u8; ArrowEgressHeader::SIZE + TRAILING_PADDING as usize];
    let header = ArrowEgressHeader {
        triples_digest: *purrdf_arrow::canon_arrow_empty_v1(),
        columnar_digest: *purrdf_arrow::canon_arrow_empty_v1(),
        ..Default::default()
    };
    frame[..ArrowEgressHeader::SIZE].copy_from_slice(&header.to_bytes());
    frame
}

/// Encode ONE batch slab under the slab-layout vocabulary.
///
/// Bounded by [`ROWS_PER_BATCH`] rows by construction: the callers chunk with
/// `chunks(ROWS_PER_BATCH)` and never hand a batch larger than that.
fn encode_batch(variables: &[String], batch: &[Vec<Option<TermValue>>]) -> Result<Vec<u8>, Error> {
    debug_assert!(
        batch.len() <= ROWS_PER_BATCH,
        "encode_batch callers chunk; an oversized batch is a caller bug"
    );
    let mut slab = Vec::new();
    for (column, _name) in variables.iter().enumerate() {
        // Kinds and the offset table are sized up front; the column's lexical
        // bytes are appended after the offsets that address them.
        let kinds_at = slab.len();
        let offsets_at = kinds_at + batch.len();
        slab.resize(offsets_at + (batch.len() + 1) * 4, 0);
        let mut offset: u32 = 0;
        for (row_index, row) in batch.iter().enumerate() {
            debug_assert_eq!(
                row.len(),
                variables.len(),
                "rows must be rectangular: measure pass indexes cells by column, so a ragged row silently drifts measure/emit"
            );
            // Kinds and offset bytes are written by index, never through a
            // held sub-slice: a held `&mut slab[...]` would overlap the other
            // region's write (E0499) and split the slab into two live borrows.
            let Some(value) = row.get(column).and_then(Option::as_ref) else {
                slab[kinds_at + row_index] = kind::UNBOUND;
                // The offset repeats: an empty lexical range marks the cell.
                slab[offsets_at + row_index * 4..offsets_at + (row_index + 1) * 4]
                    .copy_from_slice(&offset.to_le_bytes());
                continue;
            };
            slab[kinds_at + row_index] = term_kind(value);
            let token = lexical(value)?;
            slab[offsets_at + row_index * 4..offsets_at + (row_index + 1) * 4]
                .copy_from_slice(&offset.to_le_bytes());
            let end = u32::try_from(token.len())
                .ok()
                .and_then(|len| offset.checked_add(len))
                .ok_or_else(storage_overflow)?;
            offset = end;
            slab.extend_from_slice(token.as_bytes());
        }
        // The column's final offset closes the last cell's range.
        let tail = offsets_at + batch.len() * 4;
        slab[tail..tail + 4].copy_from_slice(&offset.to_le_bytes());
    }
    Ok(slab)
}

/// What the measuring pass learned about a `Solutions` result before anything
/// is emitted.
struct Measured {
    /// BLAKE3 over the canonical term spellings — the triples-side witness.
    triples_digest: [u8; 32],
    /// Payload region length: all slabs plus the trailing padding.
    payload_byte_len: u64,
    /// Whether at least one cell was unbound
    /// ([`ArrowEgressHeader::FLAG_HAS_NULLS`]).
    has_nulls: bool,
    /// Each batch's lexical total, in batch order — the per-slab expected size
    /// the emit pass asserts its slabs against.
    batch_lexical: Vec<u64>,
}

/// Pass 1: hash and measure the rows without materializing any slab. Each
/// lexical token is computed and dropped immediately; only its length and
/// digest contribution are retained.
fn measure_solutions(
    variables: &[String],
    rows: &[Vec<Option<TermValue>>],
) -> Result<Measured, Error> {
    let mut triples = blake3::Hasher::new();
    let mut payload = TRAILING_PADDING;
    let mut has_nulls = false;
    let mut batch_lexical = Vec::new();
    for batch in rows.chunks(ROWS_PER_BATCH) {
        let mut lexical_total = 0_u64;
        for row in batch {
            for cell in row {
                let Some(value) = cell else {
                    has_nulls = true;
                    continue;
                };
                let token = lexical(value)?;
                triples.update(token.as_bytes());
                triples.update(&[0]);
                lexical_total = lexical_total
                    .checked_add(token.len() as u64)
                    .ok_or_else(storage_overflow)?;
            }
        }
        // Fixed slab region: kinds (`rows * cols` bytes) plus offset tables
        // (`(rows + 1)` little-endian u32s per column).
        let rows = u64::try_from(batch.len()).map_err(|_| storage_overflow())?;
        let columns = u64::try_from(variables.len()).map_err(|_| storage_overflow())?;
        let fixed = rows
            .checked_mul(columns)
            .and_then(|term| term.checked_mul(5))
            .and_then(|term| term.checked_add(columns.checked_mul(4)?))
            .ok_or_else(storage_overflow)?;
        payload = payload
            .checked_add(fixed)
            .and_then(|sum| sum.checked_add(lexical_total))
            .ok_or_else(storage_overflow)?;
        // An offsets entry must address the column's lexical bytes in a u32.
        if lexical_total > u32::MAX as u64 {
            return Err(storage_overflow());
        }
        batch_lexical.push(lexical_total);
    }
    Ok(Measured {
        triples_digest: *triples.finalize().as_bytes(),
        payload_byte_len: payload,
        has_nulls,
        batch_lexical,
    })
}

/// Pass 2: hash the payload region — every batch slab re-encoded and fed to
/// BLAKE3, plus the trailing padding — without writing anything. Bounded the
/// same way pass 1 is: one slab staged at a time.
fn digest_payload(
    variables: &[String],
    rows: &[Vec<Option<TermValue>>],
) -> Result<[u8; 32], Error> {
    let mut payload = blake3::Hasher::new();
    for batch in rows.chunks(ROWS_PER_BATCH) {
        payload.update(&encode_batch(variables, batch)?);
    }
    payload.update(&[0_u8; TRAILING_PADDING as usize]);
    Ok(*payload.finalize().as_bytes())
}

/// Write a [`SparqlResult`] as a framed CanonArrow payload INTO `out`: the
/// same bytes [`to_canon_arrow`] returns, written incrementally. `out`'s
/// staging (and therefore the frame's residency here) is the sink's fixed
/// window; this lane stages no more than one batch slab at a time.
///
/// `provenance` is accepted for signature parity with the other format arms
/// but never consulted: the v1 frame carries no provenance extension (the
/// header reserved region is zero-checked on the wire), so the exit gate
/// `provenance_dropped` is computed by the dispatcher, not here.
///
/// # Errors
///
/// [`Error::MalformedTerm`] when a triple term at any nesting depth carries a
/// non-IRI predicate (the term writer's refusal), and [`Error::Storage`] when
/// a batch's lexical bytes cannot be addressed with `u32` offsets. Write
/// failures surface at the caller's `finish()`; a failure may leave a PREFIX
/// of the frame already written, and the caller must discard the target.
pub(crate) fn write_canon_arrow(
    result: &SparqlResult,
    provenance: &crate::ResultProvenance,
    out: &mut TextSink<'_>,
) -> Result<(), Error> {
    let _ = provenance;
    emit_into(result, out)
}

/// Serialize a [`SparqlResult`] to one contiguous CanonArrow frame.
///
/// This is the eager convenience: the returned `Vec<u8>` is the ONE deliberate
/// document-sized allocation in the canon-arrow lane (see the module
/// documentation), and it exists for FFI contiguity — a C/JS caller needs one
/// pointer, not a header plus N slab pointers. The streaming lane
/// ([`write_canon_arrow`]) holds zero document-sized allocations; prefer it
/// wherever the consumer reads incrementally.
///
/// # Examples
///
/// ```
/// use purrdf_core::{RdfDatasetBuilder, TermValue};
/// use purrdf_sparql_results::{SparqlResult, to_canon_arrow};
///
/// let result = SparqlResult::Solutions {
///     variables: vec!["s".to_string()],
///     rows: vec![vec![Some(TermValue::Iri("http://example.org/s".to_string()))]],
///     aux: RdfDatasetBuilder::new().freeze().expect("empty aux dataset").into(),
/// };
/// let frame = to_canon_arrow(&result).expect("SELECT emits a framed payload");
/// let header = purrdf_arrow::ArrowEgressHeader::from_bytes(
///     frame[..128].try_into().expect("header slice"),
/// )
/// .expect("the frame carries a valid wire header");
/// assert!(header.fits_in(frame.len() as u64));
/// assert_eq!(header.batch_count, 1);
/// ```
///
/// # Errors
///
/// Every [`write_canon_arrow`] refusal.
///
/// # Panics
///
/// Only on an internal invariant break: the frame it built must carry a valid
/// 128-byte wire header whose envelope fits the frame. No input can reach
/// these asserts — the frame is constructed, not parsed.
pub fn to_canon_arrow(result: &SparqlResult) -> Result<Vec<u8>, Error> {
    let mut out = TextSink::with_capacity(192);
    emit_into(result, &mut out)?;
    let finished = out.finish().map_err(|error| Error::Write {
        kind: error.kind(),
        cause: error,
    })?;
    let frame = finished.bytes;
    // The frame's own header must validate against the frame it was written
    // into: an envelope that does not fit its own frame is broken by
    // construction, so this is an invariant gate, not an input refusal.
    let wire: [u8; ArrowEgressHeader::SIZE] = frame[..ArrowEgressHeader::SIZE]
        .try_into()
        .expect("frame carries a full 128-byte header");
    let header = ArrowEgressHeader::from_bytes(&wire).expect("frame header round-trips");
    assert!(
        header.fits_in(frame.len() as u64),
        "frame header must fit the frame it was emitted in"
    );
    // Release-compiled: the debug measure/emit asserts are no-ops in
    // release, so the eager lane pins the payload length exactly — a
    // measure/emit drift cannot ship a mis-describing frame here.
    assert_eq!(
        u64::try_from(frame.len() - ArrowEgressHeader::SIZE).expect("frame below u64"),
        header.payload_byte_len + TRAILING_PADDING,
        "frame payload region must equal the header's declared length"
    );
    Ok(frame)
}

/// Dispatch over the result shapes: non-tabular shapes emit the canon-empty
/// frame; `Solutions` walks its rows in three bounded passes.
fn emit_into(result: &SparqlResult, out: &mut TextSink<'_>) -> Result<(), Error> {
    match result {
        SparqlResult::Solutions {
            variables, rows, ..
        } if !rows.is_empty() && !variables.is_empty() => {
            emit_solutions(variables, rows, out)
        }
        SparqlResult::Solutions { .. } | SparqlResult::Boolean(_) | SparqlResult::Graph(_) => {
            out.push_bytes(&canon_empty_frame());
            Ok(())
        }
    }
}

/// The three bounded passes over a non-empty `Solutions` result.
fn emit_solutions(
    variables: &[String],
    rows: &[Vec<Option<TermValue>>],
    out: &mut TextSink<'_>,
) -> Result<(), Error> {
    // Pass 1: measure and take the triples-side witness.
    let measured = measure_solutions(variables, rows)?;
    let batch_count =
        u32::try_from(rows.len().div_ceil(ROWS_PER_BATCH)).map_err(|_| storage_overflow())?;
    // Pass 2: take the payload-side witness.
    let columnar_digest = digest_payload(variables, rows)?;
    let header = ArrowEgressHeader {
        batch_count,
        flags: if measured.has_nulls {
            ArrowEgressHeader::FLAG_HAS_NULLS
        } else {
            0
        },
        payload_byte_len: measured.payload_byte_len,
        total_byte_len: frame_total(measured.payload_byte_len),
        triples_digest: measured.triples_digest,
        columnar_digest,
        ..Default::default()
    };
    debug_assert!(
        header.fits_in(frame_total(measured.payload_byte_len)),
        "the frame must fit the envelope it was measured for"
    );
    // Pass 3: emit — header first at the fixed 128-byte payload offset, then
    // one slab per batch, then the trailing padding.
    out.push_bytes(&header.to_bytes());
    for (batch_index, batch) in rows.chunks(ROWS_PER_BATCH).enumerate() {
        let slab = encode_batch(variables, batch)?;
        // The passes must agree exactly: what pass 1 measured is what pass 3
        // emits, slab for slab.
        debug_assert_eq!(
            slab.len() as u64,
            slab_fixed_len(variables, batch) + measured.batch_lexical[batch_index],
            "measure/emit disagreement"
        );
        out.push_bytes(&slab);
        if out.failed() {
            // The sink reports the failure at finish(); stop formatting early
            // so a dead drain costs one slab, not the document.
            return Ok(());
        }
    }
    out.push_bytes(&[0_u8; TRAILING_PADDING as usize]);
    Ok(())
}

/// The fixed (lexical-free) size of one batch slab — the pass-1 arithmetic for
/// exactly one batch, which pass 3's emitted slab must equal once its lexical
/// total is added back.
fn slab_fixed_len(variables: &[String], batch: &[Vec<Option<TermValue>>]) -> u64 {
    let rows = batch.len() as u64;
    let columns = variables.len() as u64;
    rows.checked_mul(columns)
        .and_then(|term| term.checked_mul(5))
        .and_then(|term| term.checked_add(columns * 4))
        .expect("a bounded batch's fixed slab region cannot overflow u64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use purrdf_core::RdfDatasetBuilder;

    /// Decode the 128-byte wire header out of a frame.
    fn header_of(frame: &[u8]) -> ArrowEgressHeader {
        let wire: [u8; ArrowEgressHeader::SIZE] = frame[..ArrowEgressHeader::SIZE]
            .try_into()
            .expect("full header");
        ArrowEgressHeader::from_bytes(&wire).expect("valid wire header")
    }

    fn select_result(variables: Vec<String>, rows: Vec<Vec<Option<TermValue>>>) -> SparqlResult {
        SparqlResult::Solutions {
            variables,
            rows,
            aux: RdfDatasetBuilder::new()
                .freeze()
                .expect("empty aux dataset")
                .into(),
        }
    }

    fn iri(suffix: &str) -> Option<TermValue> {
        Some(TermValue::Iri(format!("http://example.org/{suffix}")))
    }

    #[test]
    fn non_tabular_shapes_emit_the_canon_empty_frame() {
        let empty_solutions = select_result(vec!["s".to_string()], Vec::new());
        for result in [
            SparqlResult::Boolean(true),
            SparqlResult::Boolean(false),
            empty_solutions,
        ] {
            let frame = to_canon_arrow(&result).expect("non-tabular frame");
            assert_eq!(frame.len(), 192, "header plus trailing padding");
            let header = header_of(&frame);
            assert_eq!(header.batch_count, 1);
            assert_eq!(header.payload_byte_len, 0);
            assert_eq!(header.total_byte_len as usize, 192);
            assert!(frame[128..].iter().all(|&byte| byte == 0));
            assert_eq!(header.triples_digest, *purrdf_arrow::canon_arrow_empty_v1());
            assert_eq!(
                header.columnar_digest,
                *purrdf_arrow::canon_arrow_empty_v1()
            );
        }
    }

    #[test]
    fn a_zero_column_solutions_emits_the_canon_empty_frame() {
        // Reachable via the crate's own readers: {"head":{"vars":[]},
        // "results":{"bindings":[{},{},{}]}} parses to variables=[] with rows.
        let result = select_result(Vec::new(), vec![Vec::new(); 3]);
        let frame = to_canon_arrow(&result).expect("zero-column frame");
        assert_eq!(frame.len(), 192);
        let header = header_of(&frame);
        assert_eq!(header.batch_count, 1);
        assert_eq!(header.payload_byte_len, 0);
        assert_eq!(header.triples_digest, *purrdf_arrow::canon_arrow_empty_v1());
    }

    #[test]
    fn a_one_row_frame_carries_the_slab_vocabulary() {
        let value = TermValue::Iri("http://example.org/s".to_string());
        let result = select_result(vec!["s".to_string()], vec![vec![Some(value.clone())]]);
        let frame = to_canon_arrow(&result).expect("SELECT frame");
        let header = header_of(&frame);
        assert_eq!(header.batch_count, 1);
        assert!(header.fits_in(frame.len() as u64));
        assert_eq!(
            header.flags & ArrowEgressHeader::FLAG_HAS_NULLS,
            0,
            "no unbound cells"
        );

        // Slab: 1 kind byte, 2 u32 offsets, then the lexical bytes.
        let token = lexical(&value).expect("well-formed term");
        let slab = &frame[128..128 + 1 + 8 + token.len()];
        assert_eq!(slab[0], kind::IRI);
        let offsets: [u32; 2] = [
            u32::from_le_bytes(slab[1..5].try_into().expect("offset")),
            u32::from_le_bytes(slab[5..9].try_into().expect("offset")),
        ];
        assert_eq!(offsets, [0, token.len() as u32]);
        assert_eq!(&slab[9..], token.as_bytes());
        // Slab ends exactly where the padding begins.
        assert_eq!(128 + slab.len() + 64, frame.len());
        // The payload digest covers exactly the slab plus the padding.
        let mut payload = blake3::Hasher::new();
        payload.update(slab);
        payload.update(&frame[frame.len() - 64..]);
        assert_eq!(*payload.finalize().as_bytes(), header.columnar_digest);
        // And the triples witness is the token stream.
        let mut triples = blake3::Hasher::new();
        triples.update(token.as_bytes());
        triples.update(&[0]);
        assert_eq!(*triples.finalize().as_bytes(), header.triples_digest);
    }

    #[test]
    fn an_unbound_cell_carries_the_sentinel_kind_and_repeated_offset() {
        let result = select_result(vec!["s".to_string()], vec![vec![None]]);
        let frame = to_canon_arrow(&result).expect("unbound frame");
        let header = header_of(&frame);
        assert_eq!(
            header.flags & ArrowEgressHeader::FLAG_HAS_NULLS,
            ArrowEgressHeader::FLAG_HAS_NULLS,
            "the unbound cell declares itself"
        );
        let slab = &frame[128..128 + 1 + 8];
        assert_eq!(slab[0], kind::UNBOUND);
        assert_eq!(
            slab[1..9],
            [0_u8; 8],
            "empty lexical range, repeated offset"
        );
        assert_eq!(frame.len(), 128 + 9 + 64);
    }

    #[test]
    fn a_columnar_mixed_row_lays_out_per_column_in_projection_order() {
        let literal = TermValue::Literal {
            lexical_form: "5".to_string(),
            datatype: "http://www.w3.org/2001/XMLSchema#integer".to_string(),
            language: None,
            direction: None,
        };
        let blank = TermValue::Blank {
            label: "b0".to_string(),
            scope: purrdf_core::BlankScope(0),
        };
        let result = select_result(
            vec!["a".to_string(), "b".to_string()],
            vec![vec![Some(literal.clone()), Some(blank.clone())]],
        );
        let frame = to_canon_arrow(&result).expect("two-column frame");
        let lit_token = lexical(&literal).expect("well-formed literal");
        let blank_token = lexical(&blank).expect("well-formed blank");
        let slab = &frame[128..frame.len() - 64];
        // Column "a": kinds[1], offsets[2], data.
        assert_eq!(slab[0], kind::LITERAL);
        let a_len = lit_token.len();
        assert_eq!(&slab[9..9 + a_len], lit_token.as_bytes());
        // Column "b" starts after column a's slab.
        let b_at = 1 + 8 + a_len;
        assert_eq!(slab[b_at], kind::BLANK_NODE);
        let b_len = blank_token.len();
        assert_eq!(&slab[b_at + 9..b_at + 9 + b_len], blank_token.as_bytes());
        assert_eq!(slab.len(), b_at + 9 + b_len);
    }

    #[test]
    fn rows_beyond_the_batch_size_split_into_more_slabs() {
        let rows: Vec<Vec<Option<TermValue>>> = (0..ROWS_PER_BATCH + 1)
            .map(|index| vec![iri(&index.to_string())])
            .collect();
        let result = select_result(vec!["s".to_string()], rows);
        let frame = to_canon_arrow(&result).expect("multi-batch frame");
        let header = header_of(&frame);
        assert_eq!(header.batch_count, 2);
        // Each slab is fixed-region plus its own lexical bytes; row i's token
        // length grows by one digit per decade, so measure instead of assert.
        let tokens: Vec<String> = (0..ROWS_PER_BATCH + 1)
            .map(|index| {
                lexical(&iri(&index.to_string()).expect("bound")).expect("well-formed term")
            })
            .collect();
        let expected_payload: u64 =
            2 * (1 + 8) + 64 + tokens.iter().map(|token| token.len() as u64).sum::<u64>();
        assert_eq!(header.payload_byte_len, expected_payload);
        assert_eq!(frame.len() as u64, 128 + expected_payload);
    }

    #[test]
    fn the_frame_is_a_pure_function_of_the_result() {
        let result = select_result(
            vec!["s".to_string(), "p".to_string()],
            vec![vec![iri("s"), None], vec![iri("s2"), iri("p")]],
        );
        let first = to_canon_arrow(&result).expect("first frame");
        let second = to_canon_arrow(&result).expect("second frame");
        assert_eq!(first, second);
    }

    #[test]
    fn the_streaming_lane_emits_the_same_bytes_as_the_eager_frame() {
        let result = select_result(vec!["s".to_string()], vec![vec![iri("s")], vec![None]]);
        let eager = to_canon_arrow(&result).expect("eager frame");
        let mut bytes = Vec::new();
        let mut drain = purrdf_core::sink::WriterDrain(&mut bytes);
        let mut out = TextSink::to_drain(&mut drain);
        write_canon_arrow(&result, &crate::ResultProvenance::default(), &mut out)
            .expect("streaming frame");
        out.finish().expect("drain completes");
        assert_eq!(eager, bytes);
    }

    #[test]
    fn a_malformed_triple_term_predicate_is_refused_before_any_header_is_trusted() {
        let bad = TermValue::Triple {
            s: purrdf_core::TermBox::new(TermValue::Iri("http://example.org/s".to_string())),
            p: purrdf_core::TermBox::new(TermValue::Literal {
                lexical_form: "not-a-predicate".to_string(),
                datatype: "http://www.w3.org/2001/XMLSchema#string".to_string(),
                language: None,
                direction: None,
            }),
            o: purrdf_core::TermBox::new(TermValue::Iri("http://example.org/o".to_string())),
        };
        let result = select_result(vec!["s".to_string()], vec![vec![Some(bad)]]);
        let error = to_canon_arrow(&result).expect_err("malformed term refused");
        assert!(matches!(error, Error::MalformedTerm(_)));
    }

    #[test]
    fn a_nested_triple_term_non_iri_predicate_is_refused_through_the_eager_frame() {
        let bad = TermValue::Triple {
            s: purrdf_core::TermBox::new(TermValue::Iri("http://example.org/s".to_string())),
            p: purrdf_core::TermBox::new(TermValue::Triple {
                s: purrdf_core::TermBox::new(TermValue::Iri(
                    "http://example.org/is".to_string(),
                )),
                p: purrdf_core::TermBox::new(TermValue::Blank {
                    label: "bp".to_string(),
                    scope: purrdf_core::BlankScope(0),
                }),
                o: purrdf_core::TermBox::new(TermValue::Iri(
                    "http://example.org/io".to_string(),
                )),
            }),
            o: purrdf_core::TermBox::new(TermValue::Iri("http://example.org/o".to_string())),
        };
        let result = select_result(vec!["s".to_string()], vec![vec![Some(bad)]]);
        let error = to_canon_arrow(&result).expect_err("nested non-IRI predicate refused");
        assert!(matches!(error, Error::MalformedTerm(_)));
    }

    #[test]
    fn a_nested_triple_term_non_iri_predicate_is_refused_through_the_streaming_lane() {
        let bad = TermValue::Triple {
            s: purrdf_core::TermBox::new(TermValue::Iri("http://example.org/s".to_string())),
            p: purrdf_core::TermBox::new(TermValue::Triple {
                s: purrdf_core::TermBox::new(TermValue::Iri(
                    "http://example.org/is".to_string(),
                )),
                p: purrdf_core::TermBox::new(TermValue::Blank {
                    label: "bp".to_string(),
                    scope: purrdf_core::BlankScope(0),
                }),
                o: purrdf_core::TermBox::new(TermValue::Iri(
                    "http://example.org/io".to_string(),
                )),
            }),
            o: purrdf_core::TermBox::new(TermValue::Iri("http://example.org/o".to_string())),
        };
        let result = select_result(vec!["s".to_string()], vec![vec![Some(bad)]]);
        let mut bytes = Vec::new();
        let mut drain = purrdf_core::sink::WriterDrain(&mut bytes);
        let mut out = TextSink::to_drain(&mut drain);
        let error = write_canon_arrow(&result, &crate::ResultProvenance::default(), &mut out)
            .expect_err("nested non-IRI predicate refused while streaming");
        assert!(matches!(error, Error::MalformedTerm(_)));
    }
}

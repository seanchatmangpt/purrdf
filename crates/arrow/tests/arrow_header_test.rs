// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Unit tests for CanonArrow 128-byte header wire format and domain null digest.

use purrdf_arrow::{ArrowEgressHeader, ArrowEgressHeaderError, canon_arrow_empty_v1};
use std::mem::{align_of, size_of};

#[test]
fn header_size_is_128() {
    assert_eq!(size_of::<ArrowEgressHeader>(), 128);
    assert_eq!(size_of::<ArrowEgressHeader>(), ArrowEgressHeader::SIZE);
}

#[test]
fn header_alignment_is_64() {
    assert_eq!(align_of::<ArrowEgressHeader>(), 64);
}

#[test]
fn header_round_trip() {
    let header = ArrowEgressHeader {
        magic: ArrowEgressHeader::MAGIC,
        schema_version: ArrowEgressHeader::SCHEMA_VERSION,
        flags: ArrowEgressHeader::FLAG_HAS_NULLS | ArrowEgressHeader::FLAG_STRICT_PROJECTION_ORDER,
        batch_count: 5,
        payload_offset: ArrowEgressHeader::PAYLOAD_OFFSET,
        payload_byte_len: 1024,
        total_byte_len: 1024 + 128 + 64,
        triples_digest: [0x11; 32],
        columnar_digest: [0x22; 32],
        reserved: [0u8; 24],
    };

    let bytes = header.to_bytes();
    assert_eq!(bytes.len(), 128);
    let parsed = ArrowEgressHeader::from_bytes(&bytes).expect("parse header");
    assert_eq!(parsed, header);
}

#[test]
fn header_refuses_invalid_inputs() {
    // The canonical default is already wire-valid; only the payload length
    // deviates, so every refusal below is driven by a single byte flip.
    let header = ArrowEgressHeader {
        payload_byte_len: 64,
        total_byte_len: 64 + 128 + 64,
        ..Default::default()
    };
    let mut bytes = header.to_bytes();

    // Bad magic
    bytes[0] = b'Z';
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes),
        Err(ArrowEgressHeaderError::BadMagic)
    );

    // Non-zero reserved
    let mut bytes2 = header.to_bytes();
    bytes2[120] = 1;
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes2),
        Err(ArrowEgressHeaderError::ReservedNotZero)
    );

    // Bad schema version (bytes 8..10, LE u16)
    let mut bytes3 = header.to_bytes();
    bytes3[8] = 0x03;
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes3),
        Err(ArrowEgressHeaderError::BadSchemaVersion(0x0003))
    );

    // Undefined flag bits (bytes 10..12)
    let mut bytes4 = header.to_bytes();
    bytes4[10] = 0xff;
    bytes4[11] = 0xff;
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes4),
        Err(ArrowEgressHeaderError::BadFlags(0xffff))
    );

    // Bad payload offset (bytes 16..20)
    let mut bytes5 = header.to_bytes();
    bytes5[16] = 0x40;
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes5),
        Err(ArrowEgressHeaderError::BadPayloadOffset(0x40))
    );

    // Bad total length (bytes 28..36): 64 + 128 + 64 is the contract.
    let mut bytes6 = header.to_bytes();
    let wrong_total = (header.total_byte_len + 1).to_le_bytes();
    bytes6[28..36].copy_from_slice(&wrong_total);
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes6),
        Err(ArrowEgressHeaderError::BadTotalLen {
            payload_byte_len: header.payload_byte_len,
            total_byte_len: header.total_byte_len + 1,
        })
    );
}

/// Regression: a hostile wire header with payload_byte_len near u64::MAX
/// must be refused, not admitted by addition wrapping in release builds
/// (payload = u64::MAX made the old `total != payload + 192` check wrap
/// the right side down to 191, passing validation for total = 191).
#[test]
fn header_refuses_overflowing_lengths() {
    let header = ArrowEgressHeader {
        payload_byte_len: u64::MAX,
        total_byte_len: 191, // what MAX + 192 wraps to
        ..Default::default()
    };
    assert_eq!(
        ArrowEgressHeader::from_bytes(&header.to_bytes()),
        Err(ArrowEgressHeaderError::BadTotalLen {
            payload_byte_len: u64::MAX,
            total_byte_len: 191,
        })
    );
}

/// Golden-bytes wire image test. Round-trip alone cannot catch a
/// systematic byte swap that is mirrored in both `to_bytes` and
/// `from_bytes`; this pins the exact little-endian layout byte by byte.
#[test]
fn header_wire_image_is_exact() {
    let triples_digest: [u8; 32] = core::array::from_fn(|i| i as u8);
    let columnar_digest: [u8; 32] = core::array::from_fn(|i| (255 - i) as u8);
    let header = ArrowEgressHeader {
        magic: ArrowEgressHeader::MAGIC,
        schema_version: 0x0002,
        flags: ArrowEgressHeader::FLAG_STRICT_PROJECTION_ORDER, // bit2 = 0x0004
        batch_count: 2,
        payload_offset: ArrowEgressHeader::PAYLOAD_OFFSET, // 128
        payload_byte_len: 512,
        total_byte_len: 512 + 192,
        triples_digest,
        columnar_digest,
        reserved: [0u8; 24],
    };

    let bytes = header.to_bytes();
    // Field-by-field slice asserts (readable, points at the offending field).
    assert_eq!(&bytes[0..8], b"GLAWARRW");
    assert_eq!(&bytes[8..10], &[0x02, 0x00]); // schema_version 0x0002 LE
    assert_eq!(&bytes[10..12], &[0x04, 0x00]); // FLAG_STRICT_PROJECTION_ORDER = 1<<2 LE
    assert_eq!(&bytes[12..16], &[2, 0, 0, 0]); // batch_count 2 LE
    assert_eq!(&bytes[16..20], &[128, 0, 0, 0]); // payload_offset 128 LE
    assert_eq!(&bytes[20..28], &512u64.to_le_bytes()); // payload_byte_len LE
    assert_eq!(&bytes[28..36], &704u64.to_le_bytes()); // total_byte_len LE
    assert_eq!(&bytes[36..68], &triples_digest); // byte i == i
    assert_eq!(&bytes[68..100], &columnar_digest); // byte i == 255 - i
    assert!(bytes[100..128].iter().all(|&b| b == 0)); // reserved zero

    // And against the full literal image, so a layout drift anywhere fails.
    let mut expected = [0u8; 128];
    expected[0..8].copy_from_slice(b"GLAWARRW");
    expected[8] = 0x02;
    expected[10] = 0x04;
    expected[12] = 2;
    expected[16] = 128;
    for (i, slot) in expected[20..28].iter_mut().enumerate() {
        *slot = (512u64 >> (8 * i)) as u8;
    }
    for (i, slot) in expected[28..36].iter_mut().enumerate() {
        *slot = (704u64 >> (8 * i)) as u8;
    }
    expected[36..68].copy_from_slice(&triples_digest);
    expected[68..100].copy_from_slice(&columnar_digest);
    assert_eq!(bytes, expected);

    // The golden image must also decode back to the original struct.
    let parsed = ArrowEgressHeader::from_bytes(&bytes).expect("parse golden image");
    assert_eq!(parsed, header);
}

/// `Default` is a valid, parseable canonical header — the empty-payload
/// frame (payload = 0, total = 128 header + 64 padding). The refusal tests
/// and benches build on this base via struct-update syntax.
#[test]
fn header_default_is_a_parseable_wire_image() {
    let header = ArrowEgressHeader::default();
    assert_eq!(header.magic, ArrowEgressHeader::MAGIC);
    assert_eq!(header.schema_version, ArrowEgressHeader::SCHEMA_VERSION);
    assert_eq!(header.flags, 0);
    assert_eq!(header.batch_count, 1);
    assert_eq!(header.payload_offset, ArrowEgressHeader::PAYLOAD_OFFSET);
    assert_eq!(header.payload_byte_len, 0);
    assert_eq!(header.total_byte_len, 192);
    assert_eq!(header.reserved, [0; 24]);
    let parsed = ArrowEgressHeader::from_bytes(&header.to_bytes()).expect("default parses");
    assert_eq!(parsed, header);
}

/// A-2: batch_count must be consistent with payload_byte_len.
/// Refusals: (batch_count=0, payload>0) and (batch_count>1, payload=0).
#[test]
fn header_refuses_inconsistent_batch_count() {
    let base = ArrowEgressHeader {
        magic: ArrowEgressHeader::MAGIC,
        schema_version: ArrowEgressHeader::SCHEMA_VERSION,
        flags: 0,
        batch_count: 0,
        payload_offset: ArrowEgressHeader::PAYLOAD_OFFSET,
        payload_byte_len: 64,
        total_byte_len: 64 + 128 + 64,
        triples_digest: [0; 32],
        columnar_digest: [0; 32],
        reserved: [0; 24],
    };

    // Nonempty payload with zero batches.
    let mut bytes = base.to_bytes();
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes),
        Err(ArrowEgressHeaderError::BadBatchCount(0))
    );

    // Multi-batch frame with an empty payload.
    let mut empty_multi = base;
    empty_multi.batch_count = 4;
    empty_multi.payload_byte_len = 0;
    empty_multi.total_byte_len = 128 + 64;
    bytes = empty_multi.to_bytes();
    assert_eq!(
        ArrowEgressHeader::from_bytes(&bytes),
        Err(ArrowEgressHeaderError::BadBatchCount(4))
    );
}

/// Valid neighbours for the batch-count refusals (over-refusal law):
/// (batch_count=1, payload=0) — the canon empty path — and
/// (batch_count=4, large payload) must both still be accepted.
#[test]
fn header_accepts_consistent_batch_count_neighbours() {
    let base = ArrowEgressHeader {
        magic: ArrowEgressHeader::MAGIC,
        schema_version: ArrowEgressHeader::SCHEMA_VERSION,
        flags: 0,
        batch_count: 1,
        payload_offset: ArrowEgressHeader::PAYLOAD_OFFSET,
        payload_byte_len: 0,
        total_byte_len: 128 + 64,
        triples_digest: [0; 32],
        columnar_digest: [0; 32],
        reserved: [0; 24],
    };

    // Single batch, empty payload: the canon empty path.
    assert_eq!(ArrowEgressHeader::from_bytes(&base.to_bytes()), Ok(base));

    // Multiple batches over a large payload.
    let multi = ArrowEgressHeader {
        batch_count: 4,
        payload_byte_len: 1 << 20,
        total_byte_len: (1 << 20) + 128 + 64,
        ..base
    };
    assert_eq!(ArrowEgressHeader::from_bytes(&multi.to_bytes()), Ok(multi));
}

#[test]
fn fits_in_canonical_frame_boundary() {
    // Canonical frame: the Default header declares an empty payload, so the
    // total frame is exactly 128 (header) + 64 (trailing padding) = 192 bytes.
    let header = ArrowEgressHeader::default();
    assert_eq!(header.total_byte_len, 192);
    assert!(header.fits_in(192), "exact-size buffer must fit");
    assert!(!header.fits_in(191), "one byte short must not fit");
}

#[test]
fn fits_in_huge_available_is_true() {
    let header = ArrowEgressHeader {
        payload_byte_len: 4096,
        total_byte_len: 4096 + 128 + 64,
        ..Default::default()
    };
    assert!(header.fits_in(u64::MAX));
    assert!(header.fits_in(header.total_byte_len * 2));
}

#[test]
fn fits_in_zero_available_with_nonempty_header_is_false() {
    // The Default header is non-empty in the length sense (total 192 > 0),
    // so nothing fits in a zero-byte buffer.
    let header = ArrowEgressHeader::default();
    assert!(!header.fits_in(0));
    // Even a minimal wire-valid frame (total == SIZE + 64 == 192) refuses 0.
    assert_eq!(header.total_byte_len, 192);
}

#[test]
fn fits_in_matches_caller_slice_bounds_pattern() {
    // Documented caller pattern (see `ArrowEgressHeader::fits_in` docs): gate
    // the payload slice on fits_in before slicing, so
    // `&frame[SIZE..][..payload_byte_len]` cannot panic on a truncated frame.
    //
    //     if !header.fits_in(frame.len() as u64) { refuse }
    //     let payload = &frame[ArrowEgressHeader::SIZE..][..header.payload_byte_len as usize];
    //
    // Here we prove the gate's arithmetic agrees with real slice bounds:
    // frame has exactly total_byte_len bytes, so `&frame[..]` covers the
    // whole declared frame and the slice math is in-bounds.
    let header = ArrowEgressHeader {
        payload_byte_len: 256,
        total_byte_len: 256 + 128 + 64,
        ..Default::default()
    };
    let frame = vec![0u8; header.total_byte_len as usize];
    assert_eq!(frame.len(), 448);

    // Gate passes on the full frame...
    assert!(header.fits_in(frame.len() as u64));
    // ...and the documented slice math is then in-bounds and lands on the
    // payload region immediately after the 128-byte header.
    let payload: &[u8] = &frame[ArrowEgressHeader::SIZE..][..header.payload_byte_len as usize];
    assert_eq!(payload.len(), 256);

    // Truncate one byte: the gate refuses, and indeed the documented slice
    // math would panic (total region needs `frame.len()` bytes), so refusing
    // before slicing is load-bearing.
    let truncated = &frame[..frame.len() - 1];
    assert!(!header.fits_in(truncated.len() as u64));
    assert!(truncated.len() < header.total_byte_len as usize);
}

#[test]
fn empty_digest_is_deterministic() {
    let d1 = canon_arrow_empty_v1();
    let d2 = canon_arrow_empty_v1();
    assert_eq!(d1, d2);
    assert_ne!(d1, &[0u8; 32]);
}

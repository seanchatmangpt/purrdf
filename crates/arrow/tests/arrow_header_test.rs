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
    let header = ArrowEgressHeader {
        magic: ArrowEgressHeader::MAGIC,
        schema_version: ArrowEgressHeader::SCHEMA_VERSION,
        flags: 0,
        batch_count: 1,
        payload_offset: 128,
        payload_byte_len: 64,
        total_byte_len: 64 + 128 + 64,
        triples_digest: [0; 32],
        columnar_digest: [0; 32],
        reserved: [0; 24],
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
        magic: ArrowEgressHeader::MAGIC,
        schema_version: ArrowEgressHeader::SCHEMA_VERSION,
        flags: 0,
        batch_count: 1,
        payload_offset: 128,
        payload_byte_len: u64::MAX,
        total_byte_len: 191, // what MAX + 192 wraps to
        triples_digest: [0; 32],
        columnar_digest: [0; 32],
        reserved: [0; 24],
    };
    assert_eq!(
        ArrowEgressHeader::from_bytes(&header.to_bytes()),
        Err(ArrowEgressHeaderError::BadTotalLen {
            payload_byte_len: u64::MAX,
            total_byte_len: 191,
        })
    );
}

#[test]
fn empty_digest_is_deterministic() {
    let d1 = canon_arrow_empty_v1();
    let d2 = canon_arrow_empty_v1();
    assert_eq!(d1, d2);
    assert_ne!(d1, &[0u8; 32]);
}

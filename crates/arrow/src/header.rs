// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! CanonArrow egress wire protocol — 128-byte framed CanonArrow columnar-egress header (not Arrow IPC; designed for downstream Arrow IPC encapsulation).
//!
//! Layout is fixed: the struct is `#[repr(C, align(64))]` and the field
//! order is the wire order. `total_byte_len` covers payload + 128 header + 64-byte trailing padding.

use purrdf_hash::blake3;

/// Wire header for the CanonArrow columnar egress payload. 128 bytes total, 64-byte aligned.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrowEgressHeader {
    /// Wire magic, `b"GLAWARRW"`.
    pub magic: [u8; 8],
    /// Wire schema version (`0x0002`).
    pub schema_version: u16,
    /// Flag bits (see `FLAG_*` constants).
    pub flags: u16,
    /// Number of record batches in the payload.
    pub batch_count: u32,
    /// Byte offset of the payload region (always 128).
    pub payload_offset: u32,
    /// Length of the payload region in bytes.
    pub payload_byte_len: u64,
    /// Payload + 128 header + 64-byte trailing padding.
    pub total_byte_len: u64,
    /// BLAKE3 over the triples-side witness.
    pub triples_digest: [u8; 32],
    /// BLAKE3 over the payload region.
    pub columnar_digest: [u8; 32],
    /// Reserved; zero-filled on the wire (offsets 100..128).
    pub reserved: [u8; 24],
}

// Compile-time layout pin: the repr(C) struct must stay identical to the
// 128-byte wire image `to_bytes`/`from_bytes` slice by explicit offsets.
// A field add/remove/reorder that desynchronizes the two fails here.
const _: () = assert!(
    core::mem::size_of::<ArrowEgressHeader>() == ArrowEgressHeader::SIZE,
    "struct layout must equal the 128-byte wire image"
);
const _: () = assert!(
    core::mem::align_of::<ArrowEgressHeader>() == 64,
    "struct alignment must equal the 64-byte wire alignment"
);

impl ArrowEgressHeader {
    /// Total header size in bytes.
    pub const SIZE: usize = 128;
    /// Wire magic bytes.
    pub const MAGIC: [u8; 8] = *b"GLAWARRW";
    /// Wire schema version.
    ///
    /// Version history: `0x0002` is the first and only defined version;
    /// no `0x0001` archive format and no `0x0003` extension format exist.
    ///
    /// Version policy: readers fail closed on any version other than
    /// `SCHEMA_VERSION` ([`ArrowEgressHeaderError::BadSchemaVersion`]).
    /// This is deliberate — there is no best-effort decode of unknown
    /// versions and no migration shim, because a version bump may change
    /// field widths, offsets, or digest coverage, and guessing would
    /// silently corrupt reads.
    ///
    /// Upgrade path: bump `SCHEMA_VERSION` only when the wire layout
    /// changes incompatibly. `BadSchemaVersion` carries the observed
    /// value so migration tooling can classify historical and future
    /// streams by version without decoding them.
    pub const SCHEMA_VERSION: u16 = 0x0002;
    /// Byte offset of the payload region.
    pub const PAYLOAD_OFFSET: u32 = 128; A `SCHEMA_VERSION` bump is a BREAKING change — historical
/// streams fail closed on the new reader and new streams fail
/// on old readers — so it must ship in a semver major release
/// of this crate (and therefore the workspace); minor and
/// patch releases keep `SCHEMA_VERSION` unchanged.

    /// Flags bit0: payload contains null values.
    /// Advisory only: `from_bytes` mask-checks flags but never cross-checks
    /// them against `batch_count` or `payload_byte_len`, so a frame may
    /// declare this bit with an empty payload; readers must treat it as a
    /// producer hint, not an invariant.
    pub const FLAG_HAS_NULLS: u16 = 1 << 0;
    /// Flags bit1: payload encoded big-endian.
    ///
    /// Declared on the wire but not interpreted by this reader or writer:
    /// the writer always emits little-endian payloads with this bit clear,
    /// and `from_bytes` neither requires nor rejects it. A future writer
    /// that sets it takes on endianness handling itself; readers that
    /// cannot honor it must refuse before decoding the payload.
    pub const FLAG_BIG_ENDIAN: u16 = 1 << 1;
    /// Flags bit2: columns follow declared projection order strictly.
    pub const FLAG_STRICT_PROJECTION_ORDER: u16 = 1 << 2;

    /// Mask of defined flag bits.
    pub const FLAGS_MASK: u16 =
        Self::FLAG_HAS_NULLS | Self::FLAG_BIG_ENDIAN | Self::FLAG_STRICT_PROJECTION_ORDER;

    /// Serialize to the 128-byte little-endian wire form.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[0..8].copy_from_slice(&self.magic);
        out[8..10].copy_from_slice(&self.schema_version.to_le_bytes());
        out[10..12].copy_from_slice(&self.flags.to_le_bytes());
        out[12..16].copy_from_slice(&self.batch_count.to_le_bytes());
        out[16..20].copy_from_slice(&self.payload_offset.to_le_bytes());
        out[20..28].copy_from_slice(&self.payload_byte_len.to_le_bytes());
        out[28..36].copy_from_slice(&self.total_byte_len.to_le_bytes());
        out[36..68].copy_from_slice(&self.triples_digest);
        out[68..100].copy_from_slice(&self.columnar_digest);
        out
    }

    /// Deserialize from the 128-byte wire form.
    ///
    /// Version policy: only [`ArrowEgressHeader::SCHEMA_VERSION`]
    /// (`0x0002`, the first and only defined version) is accepted; any
    /// other version fails closed with
    /// [`ArrowEgressHeaderError::BadSchemaVersion`], which carries the
    /// observed value so migration tooling can identify the stream
    /// without decoding it. See that constant for the upgrade path.
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Result<Self, ArrowEgressHeaderError> {
        if bytes[0..8] != Self::MAGIC {
            return Err(ArrowEgressHeaderError::BadMagic);
        }
        let schema_version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        if schema_version != Self::SCHEMA_VERSION {
            return Err(ArrowEgressHeaderError::BadSchemaVersion(schema_version));
        }
        let flags = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        if flags & !Self::FLAGS_MASK != 0 {
            return Err(ArrowEgressHeaderError::BadFlags(flags));
        }
        let batch_count = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let payload_offset = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        if payload_offset != Self::PAYLOAD_OFFSET {
            return Err(ArrowEgressHeaderError::BadPayloadOffset(payload_offset));
        }
        let payload_byte_len = u64::from_le_bytes(bytes[20..28].try_into().unwrap());
        // Batch-count consistency: a nonempty payload must belong to at least
        // one batch; a multi-batch frame cannot carry an empty payload.
        if batch_count == 0 && payload_byte_len > 0 {
            return Err(ArrowEgressHeaderError::BadBatchCount(batch_count));
        }
        if batch_count > 1 && payload_byte_len == 0 {
            return Err(ArrowEgressHeaderError::BadBatchCount(batch_count));
        }
        let total_byte_len = u64::from_le_bytes(bytes[28..36].try_into().unwrap());
        // Subtraction order, not addition: a hostile wire header with
        // payload_byte_len near u64::MAX must overflow-refuse here, not
        // wrap total_byte_len + SIZE + 64 to a small passing value.
        let expected = match total_byte_len
            .checked_sub(Self::SIZE as u64)
            .and_then(|rest| rest.checked_sub(64))
        {
            Some(rest) => rest,
            None => {
                return Err(ArrowEgressHeaderError::BadTotalLen {
                    payload_byte_len,
                    total_byte_len,
                });
            }
        };
        if payload_byte_len != expected {
            return Err(ArrowEgressHeaderError::BadTotalLen {
                payload_byte_len,
                total_byte_len,
            });
        }
        let mut triples_digest = [0u8; 32];
        triples_digest.copy_from_slice(&bytes[36..68]);
        let mut columnar_digest = [0u8; 32];
        columnar_digest.copy_from_slice(&bytes[68..100]);
        if bytes[100..128].iter().any(|&b| b != 0) {
            return Err(ArrowEgressHeaderError::ReservedNotZero);
        }
        Ok(Self {
            magic: Self::MAGIC,
            schema_version,
            flags,
            batch_count,
            payload_offset,
            payload_byte_len,
            total_byte_len,
            triples_digest,
            columnar_digest,
            reserved: [0u8; 24],
        })
    }

    /// Whether a frame declaring this header fits in `available` bytes.
    ///
    /// `from_bytes` validates the header's internal length consistency, but
    /// the wire cannot prove the payload bytes actually follow — slicing
    /// `&frame[Self::SIZE..][..self.payload_byte_len]` without this check is
    /// the caller-side panic surface. Gate the slice on this.
    #[must_use]
    pub fn fits_in(&self, available: u64) -> bool {
        available >= self.total_byte_len
    }
}

/// Typed errors for header validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowEgressHeaderError {
    /// Wire magic did not match `GLAWARRW`.
    BadMagic,
    /// Unknown schema version.
    BadSchemaVersion(u16),
    /// Undefined flag bits set.
    BadFlags(u16),
    /// Payload offset was not 128.
    BadPayloadOffset(u32),
    /// `batch_count` inconsistent with `payload_byte_len`.
    BadBatchCount(u32),
    /// `total_byte_len` inconsistent with `payload_byte_len`.
    BadTotalLen {
        /// Payload region length.
        payload_byte_len: u64,
        /// Declared total length.
        total_byte_len: u64,
    },
    /// Reserved region was not zero-filled.
    ReservedNotZero,
}

impl std::fmt::Display for ArrowEgressHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "canonarrow: bad wire magic"),
            Self::BadSchemaVersion(v) => write!(f, "canonarrow: unsupported schema version {v}"),
            Self::BadFlags(fl) => write!(f, "canonarrow: undefined flag bits 0x{fl:04x}"),
            Self::BadPayloadOffset(off) => {
                write!(f, "canonarrow: expected payload offset 128, got {off}")
            }
            Self::BadBatchCount(n) => {
                write!(f, "canonarrow: batch count {n} inconsistent with payload")
            }
            Self::BadTotalLen {
                payload_byte_len,
                total_byte_len,
            } => write!(
                f,
                "canonarrow: inconsistent lengths: payload={payload_byte_len}, total={total_byte_len}"
            ),
            Self::ReservedNotZero => {
                write!(f, "canonarrow: reserved header region not zero-filled")
            }
        }
    }
}

impl std::error::Error for ArrowEgressHeaderError {}

impl purrdf_hash::error::StructuredError for ArrowEgressHeaderError {
    purrdf_hash::error_visit! {
        BadMagic => "canonarrow-bad-magic" {},
        BadSchemaVersion(v) => "canonarrow-bad-schema-version" { "version" => Unsigned(*v as u128) },
        BadFlags(f) => "canonarrow-bad-flags" { "flags" => Unsigned(*f as u128) },
        BadPayloadOffset(o) => "canonarrow-bad-payload-offset" { "offset" => Unsigned(*o as u128) },
        BadBatchCount(n) => "canonarrow-bad-batch-count" { "batch_count" => Unsigned(*n as u128) },
        BadTotalLen { payload_byte_len, total_byte_len } => "canonarrow-bad-total-len" {
            "payload_len" => Unsigned(*payload_byte_len as u128),
            "total_len" => Unsigned(*total_byte_len as u128),
        },
        ReservedNotZero => "canonarrow-reserved-not-zero" {},
    }
}

impl Default for ArrowEgressHeader {
    /// A valid, parseable canonical header: wire magic, current schema
    /// version, no flags, one record batch, payload at
    /// [`ArrowEgressHeader::PAYLOAD_OFFSET`], empty payload (so
    /// `total_byte_len` is exactly header + 64-byte trailing padding),
    /// zero digests, zero reserved. `from_bytes(&default.to_bytes())`
    /// succeeds.
    fn default() -> Self {
        Self {
            magic: Self::MAGIC,
            schema_version: Self::SCHEMA_VERSION,
            flags: 0,
            batch_count: 1,
            payload_offset: Self::PAYLOAD_OFFSET,
            payload_byte_len: 0,
            total_byte_len: (Self::SIZE as u64) + 64,
            triples_digest: [0; 32],
            columnar_digest: [0; 32],
            reserved: [0; 24],
        }
    }
}

/// Domain null digest: `H_columnar_empty = BLAKE3(b"CANON_ARROW_EMPTY_V1")` for non-tabular results.
pub fn canon_arrow_empty_v1() -> &'static [u8; 32] {
    static DIGEST: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    DIGEST.get_or_init(|| *blake3::hash(b"CANON_ARROW_EMPTY_V1").as_bytes())
}

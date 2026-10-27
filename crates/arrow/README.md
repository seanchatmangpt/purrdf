<!--
SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
SPDX-License-Identifier: CC-BY-4.0
-->

# purrdf-arrow

`purrdf-arrow` implements **CanonArrow**, the PurRDF canonical columnar egress
wire protocol: a single-dependency (purrdf-hash), cache-line-aligned framing header for
streaming query results into Arrow IPC buffers. The protocol is one fixed
128-byte header (`ArrowEgressHeader`) plus a payload region.

## The wire header

`ArrowEgressHeader` is `#[repr(C, align(64))]`, exactly 128 bytes on the wire
(both pinned by compile-time layout assertions in `crates/arrow/src/header.rs`)
with the field order equal to the wire order:

| bytes   | field              | notes |
|---------|--------------------|-------|
| 0..8    | `magic`            | `b"GLAWARRW"` |
| 8..10   | `schema_version`   | `0x0002` |
| 10..12  | `flags`            | see flag bits below; undefined bits refuse |
| 12..16  | `batch_count`      | record batches in the payload |
| 16..20  | `payload_offset`   | always `128` |
| 20..28  | `payload_byte_len` | payload region length |
| 28..36  | `total_byte_len`   | payload + 128 header + 64-byte trailing padding |
| 36..68  | `triples_digest`   | BLAKE3 over the triples-side witness |
| 68..100 | `columnar_digest`  | BLAKE3 over the payload region |
| 100..128| `reserved`         | zero-filled on the wire; nonzero refuses |

Serialization is little-endian. The flag bits are bit0 `FLAG_HAS_NULLS`, bit1
`FLAG_BIG_ENDIAN`, and bit2 `FLAG_STRICT_PROJECTION_ORDER`. Two BLAKE3 digests
travel with every frame —
a triples-side witness digest and a payload-region digest — so a consumer can
verify both provenance and payload integrity independently. For non-tabular
(empty) results the domain digest is
`canon_arrow_empty_v1() = BLAKE3(b"CANON_ARROW_EMPTY_V1")`.

## What CanonArrow is NOT

CanonArrow is **not Arrow IPC**. It uses no flatbuffers, no Arrow IPC
container formats, and no Arrow runtime of any kind. The framing is designed
for *downstream* Arrow IPC encapsulation: a consumer slices the payload region
out of the frame and hands it to a real Arrow IPC reader. The framing bytes
themselves are first-party, like the rest of PurRDF's codecs.

## Validation (`from_bytes`)

`ArrowEgressHeader::from_bytes` refuses, with a typed
`ArrowEgressHeaderError`, when any of the following fails:

- the magic is not `GLAWARRW` (`BadMagic`);
- the schema version is not `0x0002` (`BadSchemaVersion`);
- any flag bit outside `FLAGS_MASK` is set (`BadFlags`);
- the payload offset is not `128` (`BadPayloadOffset`);
- batch count is inconsistent with the payload length — zero batches with a
  nonempty payload, or multiple batches with an empty payload
  (`BadBatchCount`);
- `total_byte_len` is inconsistent with `payload_byte_len`, computed by
  **subtraction** (`total - 128 - 64`) rather than addition, so a hostile
  header with a near-`u64::MAX` payload length overflow-refuses instead of
  wrapping (`BadTotalLen`);
- the reserved region is not zero-filled (`ReservedNotZero`).

`Default::default()` is a valid, parseable wire image:
`from_bytes(&default.to_bytes())` succeeds.

## `fits_in`: gating the payload slice

`from_bytes` proves the header's internal consistency, but the wire cannot
prove the payload bytes actually follow. Slicing without a length gate is the
caller-side panic surface:

```rust
use purrdf_arrow::ArrowEgressHeader;

let frame: &[u8] = /* 128-byte header + payload + padding */;
let header = ArrowEgressHeader::from_bytes(
    frame[..ArrowEgressHeader::SIZE].try_into()?,
)?;
if header.fits_in(frame.len() as u64) {
    let payload = &frame[ArrowEgressHeader::PAYLOAD_OFFSET as usize..]
        [..header.payload_byte_len as usize];
    // hand `payload` to an Arrow IPC reader
}
```

## Tests and benches

- `crates/arrow/tests/arrow_header_test.rs` — layout (128 bytes, 64-byte
  alignment), round-trip, the full refusal matrix (including the overflow
  refusal), exact wire image, default-is-parseable, digest determinism.
- `crates/arrow/benches/ipc_egress.rs` — 11 benches across 4 groups:
  `ipc_egress_100k_rows/{encode,decode}` (BLAKE3 payload digest + frame
  assembly / header parse + digest verification, bytes per second over the
  full wire frame), `ipc_egress_reject/decode_bad_magic` (rejection path),
  `ipc_egress_header_flags/{has_nulls,big_endian,strict_projection_order}`
  (header-only round-trips per flag bit), and
  `ipc_egress_size_sweep/{encode,decode}@{1000,10000}`.

## Status

The header, its validation, and its tests are complete and frozen, but the
crate has **no production consumer today**: nothing in the workspace wires
`ArrowEgressHeader` into the query egress path yet. That wiring is tracked as
[OPT-17](../../docs/jira/v26.10.26/optimization/OPT-17-arrow-egress-wiring.md)
("Wire ArrowEgressHeader Into Production Egress"). Until OPT-17 lands, treat
this crate as test-only.

// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! Report-only CanonArrow egress frame measurements over a seeded batch of
//! 100,000 rows. Encode is the wire-side cost — the BLAKE3 digest of the
//! payload region, the 128-byte header wire form, and the frame assembly
//! (header + payload + 64-byte zero padding). Decode is the receipt-side
//! cost — the header parse and the payload digest verification. Both rows
//! report bytes per second over the full wire frame. Report-only; the
//! estimates a run prints are the numbers, and no GB/s claim is asserted
//! in-tree.

use std::mem::size_of;

use purrdf_arrow::{ArrowEgressHeader, canon_arrow_empty_v1};
use purrdf_hash::blake3;
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main};
use purrdf_testkit::rng::splitmix64_bytes;

/// Rows in the seeded batch.
const ROWS: usize = 100_000;
/// `u64` columns per row: a 64-byte row, one cache line.
const COLUMNS: usize = 8;
/// Payload bytes: one little-endian `u64` per column, columns contiguous.
const PAYLOAD_LEN: usize = ROWS * COLUMNS * size_of::<u64>();
/// Wire frame bytes: payload + 128-byte header + 64-byte trailing padding.
const FRAME_LEN: usize = PAYLOAD_LEN + ArrowEgressHeader::SIZE + 64;
/// The fixture seed: splitmix64 from a constant, no clock, no `rand`.
const SEED: u64 = 0xCA4A_C0DE;

/// The seeded fixture: payload bytes, the header that will frame them, and
/// the assembled frame. The batch doubles as the digest subject, so the
/// header carries the domain null digest on the triples side (this fixture
/// has no triples side) and the payload digest on the columnar side.
///
/// Asserts the fixture round-trips before anything is timed: the header
/// survives `to_bytes`/`from_bytes`, the frame length matches the declared
/// `total_byte_len`, and the payload digest verifies.
fn fixture() -> (ArrowEgressHeader, Vec<u8>, Vec<u8>) {
    let payload = splitmix64_bytes(PAYLOAD_LEN, SEED);
    assert_eq!(payload.len(), PAYLOAD_LEN);
    let digest = *blake3::hash(&payload).as_bytes();
    let header = ArrowEgressHeader {
        flags: ArrowEgressHeader::FLAG_STRICT_PROJECTION_ORDER,
        payload_byte_len: PAYLOAD_LEN as u64,
        total_byte_len: FRAME_LEN as u64,
        triples_digest: *canon_arrow_empty_v1(),
        columnar_digest: digest,
        ..Default::default()
    };
    let frame = frame_from(&header, &payload, FRAME_LEN);
    assert_eq!(frame.len(), FRAME_LEN);
    assert_eq!(
        ArrowEgressHeader::from_bytes(
            frame[0..ArrowEgressHeader::SIZE]
                .try_into()
                .expect("a frame carries a 128-byte header"),
        )
        .expect("the fixture header parses"),
        header,
        "the fixture header round-trips",
    );
    println!(
        "ipc-egress-geometry: {ROWS} rows x {COLUMNS} u64 columns -> payload {PAYLOAD_LEN} B, frame {FRAME_LEN} B"
    );
    (header, payload, frame)
}

/// The full wire frame: 128-byte header, payload region, 64-byte zero
/// padding to the 64-byte alignment the layout promises. `frame_len` is the
/// declared `total_byte_len` of `header` (`FRAME_LEN` for the 100k fixture,
/// smaller for the size-sweep fixtures).
fn frame_from(header: &ArrowEgressHeader, payload: &[u8], frame_len: usize) -> Vec<u8> {
    let mut frame = Vec::with_capacity(frame_len);
    frame.extend_from_slice(&header.to_bytes());
    frame.extend_from_slice(payload);
    frame.resize(frame_len, 0);
    frame
}

/// Encode and decode bytes per second over the 100k-row egress frame.
fn bench_ipc_egress(c: &mut Bench) {
    let (header, payload, frame) = fixture();
    let mut group = c.benchmark_group("ipc_egress_100k_rows");
    group.throughput(Throughput::Bytes(FRAME_LEN as u64));
    group.bench_function("encode", |bencher| {
        bencher.iter(|| {
            let digest = *blake3::hash(&payload).as_bytes();
            let header = ArrowEgressHeader {
                triples_digest: *canon_arrow_empty_v1(),
                columnar_digest: digest,
                ..header
            };
            std::hint::black_box(frame_from(&header, &payload, FRAME_LEN))
        });
    });
    group.bench_function("decode", |bencher| {
        bencher.iter(|| {
            let header = ArrowEgressHeader::from_bytes(
                frame[0..ArrowEgressHeader::SIZE]
                    .try_into()
                    .expect("a frame carries a 128-byte header"),
            )
            .expect("the fixture frame parses");
            let payload_region =
                &frame[ArrowEgressHeader::PAYLOAD_OFFSET as usize..][..PAYLOAD_LEN];
            let verified = blake3::hash(payload_region).as_bytes() == &header.columnar_digest;
            std::hint::black_box((header, verified))
        });
    });
    group.finish();
}

/// A header-only adversarial-flag fixture: the canonical `Default` header
/// (a valid, parseable no-flags header) with exactly one flag bit set. The
/// frame is header-plus-padding only — no payload — so the timed body is the
/// pure header wire round-trip.
fn flagged_header(flags: u16) -> ArrowEgressHeader {
    ArrowEgressHeader {
        flags,
        ..ArrowEgressHeader::default()
    }
}

/// Round-trip one adversarial flag combo through the 128-byte wire form:
/// `to_bytes` then `from_bytes`, asserting the parse succeeds before timing.
fn bench_ipc_egress_header_flags(c: &mut Bench) {
    let mut group = c.benchmark_group("ipc_egress_header_flags");
    group.throughput(Throughput::Bytes(ArrowEgressHeader::SIZE as u64));
    for (id, flags) in [
        ("has_nulls", ArrowEgressHeader::FLAG_HAS_NULLS),
        ("big_endian", ArrowEgressHeader::FLAG_BIG_ENDIAN),
        (
            "strict_projection_order",
            ArrowEgressHeader::FLAG_STRICT_PROJECTION_ORDER,
        ),
    ] {
        let header = flagged_header(flags);
        let wire = header.to_bytes();
        assert_eq!(
            ArrowEgressHeader::from_bytes(&wire).expect("the flagged header parses"),
            header,
            "the flagged header round-trips",
        );
        group.bench_function(id, |bencher| {
            bencher.iter(|| {
                let wire = header.to_bytes();
                let parsed = ArrowEgressHeader::from_bytes(&wire)
                    .expect("the flagged header parses");
                std::hint::black_box(parsed)
            });
        });
    }
    group.finish();
}

/// The rejection fixture: a frame whose header carries a deliberately wrong
/// magic (everything else canonical via `Default`). `from_bytes` must refuse
/// it on the error path; asserted once before timing.
fn bad_magic_frame() -> [u8; ArrowEgressHeader::SIZE] {
    let mut header = ArrowEgressHeader::default();
    header.magic = *b"BADARRW!";
    let wire = header.to_bytes();
    assert!(
        ArrowEgressHeader::from_bytes(&wire).is_err(),
        "the bad-magic header must be refused"
    );
    wire
}

/// The timed cost of the `from_bytes` rejection path on a BadMagic frame.
/// The error path is the load-bearing branch on the receipt side for frames
/// that are not CanonArrow at all.
fn bench_ipc_egress_reject(c: &mut Bench) {
    let wire = bad_magic_frame();
    let mut group = c.benchmark_group("ipc_egress_reject");
    group.throughput(Throughput::Bytes(ArrowEgressHeader::SIZE as u64));
    group.bench_function("decode_bad_magic", |bencher| {
        bencher.iter(|| {
            let refused = ArrowEgressHeader::from_bytes(&wire).is_err();
            std::hint::black_box(refused)
        });
    });
    group.finish();
}

/// Encode and decode at 1,000 and 10,000 rows — the small end of the same
/// geometry the 100k row measures, so the per-row cost curve has points at
/// both ends. Payloads stay in the kilobyte range.
fn bench_ipc_egress_size_sweep(c: &mut Bench) {
    let mut group = c.benchmark_group("ipc_egress_size_sweep");
    for rows in [1_000usize, 10_000] {
        let payload_len = rows * COLUMNS * size_of::<u64>();
        let frame_len = payload_len + ArrowEgressHeader::SIZE + 64;
        let payload = splitmix64_bytes(payload_len, SEED);
        let digest = *blake3::hash(&payload).as_bytes();
        let header = ArrowEgressHeader {
            triples_digest: *canon_arrow_empty_v1(),
            columnar_digest: digest,
            payload_byte_len: payload_len as u64,
            total_byte_len: frame_len as u64,
            ..ArrowEgressHeader::default()
        };
        let frame = {
            let mut frame = Vec::with_capacity(frame_len);
            frame.extend_from_slice(&header.to_bytes());
            frame.extend_from_slice(&payload);
            frame.resize(frame_len, 0);
            frame
        };
        assert_eq!(frame.len(), frame_len);
        assert_eq!(
            ArrowEgressHeader::from_bytes(
                frame[0..ArrowEgressHeader::SIZE]
                    .try_into()
                    .expect("a frame carries a 128-byte header"),
            )
            .expect("the sweep fixture header parses"),
            header,
            "the sweep fixture header round-trips",
        );
        group.throughput(Throughput::Bytes(frame_len as u64));
        group.bench_with_input(
            BenchmarkId::new("encode", rows),
            &(&header, &payload),
            |bencher, &(header, payload)| {
                bencher.iter(|| {
                    let digest = *blake3::hash(payload).as_bytes();
                    let header = ArrowEgressHeader {
                        triples_digest: *canon_arrow_empty_v1(),
                        columnar_digest: digest,
                        ..*header
                    };
                    std::hint::black_box(frame_from(&header, payload, frame_len))
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("decode", rows),
            &frame,
            |bencher, frame| {
                bencher.iter(|| {
                    let header = ArrowEgressHeader::from_bytes(
                        frame[0..ArrowEgressHeader::SIZE]
                            .try_into()
                            .expect("a frame carries a 128-byte header"),
                    )
                    .expect("the sweep frame parses");
                    let payload_region = &frame[ArrowEgressHeader::PAYLOAD_OFFSET as usize..]
                        [..payload_len];
                    let verified =
                        blake3::hash(payload_region).as_bytes() == &header.columnar_digest;
                    std::hint::black_box((header, verified))
                });
            },
        );
    }
    group.finish();
}

bench_group!(
    benches,
    bench_ipc_egress,
    bench_ipc_egress_header_flags,
    bench_ipc_egress_reject,
    bench_ipc_egress_size_sweep
);
bench_main!(benches);

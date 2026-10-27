// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! MIME transfer-decode hot path: the original-byte parse (`analyze`) as GB/s
//! over a seeded multipart message, and the two content transfer decoders
//! (base64, quoted-printable) as GB/s over their decoded payloads, through
//! `Document::decoded_attachment`.
//!
//! Report-only, `cargo bench -p purrdf-mime --bench decode`. The decoded
//! rows measure the public path, which re-verifies the model against the
//! source under the profile before decoding — subtract the `analyze` row to
//! isolate the transfer decode. Fixtures are deterministic xorshift64 streams
//! at const seeds — no RNG dependency, no clock.

use purrdf_mime::{Document, Limits, Profile, SourceDocument, Vocabulary, analyze};
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};
use purrdf_testkit::rng::xorshift64_next;
use purrdf_xsd::{canonical_base64, canonical_hex};

/// Original payload octets carried by each transfer-encoded part.
const PAYLOAD_BYTES: usize = 24 << 10;

/// Seeds of the two payloads, `mime_b64` and `mime_qp` in hex.
const BASE64_SEED: u64 = 0x6D69_6D65_5F62_3634;
const QUOTED_PRINTABLE_SEED: u64 = 0x6D69_6D65_5F71_7050;

/// A seeded payload byte stream.
fn payload(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| (xorshift64_next(&mut state) & 0xFF) as u8)
        .collect()
}

/// The base64 body: canonical base64 wrapped at 76 octets with CRLF.
fn base64_body(bytes: &[u8]) -> String {
    let encoded = canonical_base64(bytes);
    let mut body = String::new();
    for chunk in encoded.as_bytes().chunks(76) {
        body.push_str(core::str::from_utf8(chunk).expect("base64 alphabet is ASCII"));
        body.push_str("\r\n");
    }
    body
}

/// The quoted-printable body: printable ASCII passes through, every other
/// octet escapes as `=XX`, soft breaks (`=` immediately before the CRLF) keep
/// encoded lines inside the RFC 2045 76-octet ceiling.
fn quoted_printable_body(bytes: &[u8]) -> String {
    let mut body = String::new();
    let mut line = String::new();
    for &byte in bytes {
        let token = if (33..=126).contains(&byte) && byte != b'=' {
            (byte as char).to_string()
        } else {
            format!("={}", canonical_hex(&[byte]))
        };
        if line.len() + token.len() > 72 {
            line.push('=');
            body.push_str(&line);
            body.push_str("\r\n");
            line.clear();
        }
        line.push_str(&token);
    }
    body.push_str(&line);
    body.push_str("\r\n");
    body
}

/// The seeded multipart message: a text intro, the base64 part, the
/// quoted-printable part, and a text closing, under one boundary.
fn message() -> String {
    let mut text = String::new();
    text.push_str("MIME-Version: 1.0\r\n");
    text.push_str("Subject: seeded transfer decodes\r\n");
    text.push_str("Content-Type: multipart/mixed; boundary=seed42\r\n");
    text.push_str("\r\n");
    text.push_str("preamble\r\n");
    text.push_str("--seed42\r\n");
    text.push_str("Content-Type: text/plain; charset=us-ascii\r\n");
    text.push_str("\r\n");
    text.push_str("intro\r\n");
    text.push_str("--seed42\r\n");
    text.push_str("Content-Transfer-Encoding: base64\r\n");
    text.push_str("\r\n");
    text.push_str(&base64_body(&payload(PAYLOAD_BYTES, BASE64_SEED)));
    text.push_str("--seed42\r\n");
    text.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
    text.push_str("\r\n");
    text.push_str(&quoted_printable_body(&payload(
        PAYLOAD_BYTES,
        QUOTED_PRINTABLE_SEED,
    )));
    text.push_str("--seed42\r\n");
    text.push_str("Content-Type: text/plain; charset=us-ascii\r\n");
    text.push_str("\r\n");
    text.push_str("closing\r\n");
    text.push_str("--seed42--\r\n");
    text
}

/// Locate the one part declared with `encoding`, for stable part indices.
fn part_with_encoding(document: &Document<'_>, encoding: &str) -> usize {
    document
        .parts
        .iter()
        .position(|part| part.transfer_encoding.name() == encoding)
        .expect("seeded fixture declares both transfer-encoded parts")
}

/// The parse and both transfer decoders over the one seeded message.
fn bench_decode(c: &mut Bench) {
    let profile =
        Profile::new("bench", Vocabulary::standard().expect("shipped vocabulary"), Limits::unbounded())
            .expect("bench profile builds");
    let text = message();
    let document = analyze(
        SourceDocument {
            id: "urn:example:seeded-message",
            bytes: text.as_bytes(),
        },
        &profile,
    )
    .expect("seeded message parses");
    let base64_part = part_with_encoding(&document, "base64");
    let quoted_part = part_with_encoding(&document, "quoted-printable");
    let base64_bytes = document
        .decoded_attachment(base64_part)
        .expect("base64 part decodes")
        .bytes
        .len();
    let quoted_bytes = document
        .decoded_attachment(quoted_part)
        .expect("quoted-printable part decodes")
        .bytes
        .len();

    let mut group = c.benchmark_group("mime_decode");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("analyze", |b| {
        b.iter(|| {
            analyze(
                SourceDocument {
                    id: "urn:example:seeded-message",
                    bytes: black_box(text.as_bytes()),
                },
                &profile,
            )
            .expect("seeded message parses");
        });
    });
    group.throughput(Throughput::Bytes(base64_bytes as u64));
    group.bench_function("base64", |b| {
        b.iter(|| {
            black_box(
                document
                    .decoded_attachment(base64_part)
                    .expect("base64 part decodes")
                    .bytes,
            )
        });
    });
    group.throughput(Throughput::Bytes(quoted_bytes as u64));
    group.bench_function("quoted_printable", |b| {
        b.iter(|| {
            black_box(
                document
                    .decoded_attachment(quoted_part)
                    .expect("quoted-printable part decodes")
                    .bytes,
            )
        });
    });
    group.finish();
}

bench_group!(benches, bench_decode);
bench_main!(benches);

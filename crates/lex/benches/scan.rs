// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! Byte-class scanner and JSON string escape benchmark.
//!
//! Report-only, `cargo bench -p purrdf-lex --bench scan`.
//!
//! `lex_scan/*` runs each chunked byte-class scanner in `purrdf_lex::terminals`
//! over one long clean run ending in the byte it stops at, so the sixteen-byte
//! chunk loop is the measured cost, and over a short token-sized run, so the
//! per-call setup is.
//!
//! `lex_json_escape/*` runs the shared JSON string escape law
//! (`purrdf_lex::json_escape`) in each of its four spellings, over a long clean
//! ASCII run (the chunked stop scan is the cost) and over mixed text dense in
//! stops (the per-`char` escape table is).
//!
//! `lex_json_reader_gb/next_event` walks every event of a seeded ~1 MiB JSON
//! document (an array of records, drawn from the fixed-seed `xoshiro256**`
//! stream in this file), so the reader's event loop is the measured cost.
//!
//! `lex_yaml_gb/{write,read}` emit and read back that same document as YAML
//! 1.2 block text, per byte of emitter output.
//!
//! `lex_cbor_roundtrip/{encode,decode}` encode and decode a seeded CBOR
//! document mirroring the JSON records, per byte both directions.
//!
//! `lex_unicode_norm/nfkc` normalizes about 256 KiB of mixed ASCII, CJK
//! ideographs and fullwidth capitals (the compatibility mappings NFKC
//! actually applies).

use purrdf_lex::json::{Event, Limits, Reader};
use purrdf_lex::json_escape::{JsonEscapes, push_body};
use purrdf_lex::terminals::{
    find_first_iri_body_special, find_first_json_string_special, find_first_trivia,
    find_first_xml_special,
};
use purrdf_lex::unicode::nfkc;
use purrdf_lex::{cbor, json, yaml};
use purrdf_testkit::bench::{Bench, Throughput, bench_group, bench_main, black_box};
use purrdf_testkit::rng::Xoshiro256;

/// The seed every seeded fixture in this file draws from.
const FIXTURE_SEED: u64 = 0x6c65_785f_6a73_6f6e; // "lex_json"

/// A seeded lowercase word of 8 to 23 letters.
fn seeded_word(rng: &mut Xoshiro256) -> String {
    let length = 8 + (rng.next_u64() % 16) as usize;
    (0..length)
        .map(|_| (b'a' + (rng.next_u64() % 26) as u8) as char)
        .collect()
}

/// A seeded JSON record: an id, a name, a score and a three-word tag list.
fn seeded_record(rng: &mut Xoshiro256) -> String {
    let tags: Vec<String> = (0..3).map(|_| seeded_word(rng)).collect();
    let tags = tags
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"id\":{},\"name\":\"{}\",\"score\":{},\"tags\":[{}]}}",
        rng.next_u64() % 1_000_000,
        seeded_word(rng),
        rng.next_u64() % 100_000,
        tags
    )
}

/// A seeded JSON document of about 1 MiB: an array of records, identical on
/// every run.
fn seeded_json_document() -> String {
    let mut rng = Xoshiro256::from_seed(FIXTURE_SEED);
    let mut records: Vec<String> = Vec::new();
    let mut total = 2;
    while total < (1 << 20) {
        let record = seeded_record(&mut rng);
        total += record.len() + 1;
        records.push(record);
    }
    let joined = records.join(",");
    let mut document = String::with_capacity(joined.len() + 2);
    document.push('[');
    document.push_str(&joined);
    document.push(']');
    document
}

/// A seeded CBOR value mirroring the JSON records: an array of four-member
/// maps, about 1 MiB encoded.
fn seeded_cbor_value() -> cbor::Value {
    let mut rng = Xoshiro256::from_seed(FIXTURE_SEED);
    let records: Vec<cbor::Value> = (0..8192)
        .map(|_| {
            cbor::Value::Map(vec![
                (
                    cbor::Value::from("id"),
                    cbor::Value::from(rng.next_u64() % 1_000_000),
                ),
                (
                    cbor::Value::from("name"),
                    cbor::Value::from(seeded_word(&mut rng).as_str()),
                ),
                (
                    cbor::Value::from("score"),
                    cbor::Value::Float((rng.next_u64() % 100_000) as f64),
                ),
                (
                    cbor::Value::from("tags"),
                    cbor::Value::Array(
                        (0..3)
                            .map(|_| cbor::Value::from(seeded_word(&mut rng).as_str()))
                            .collect(),
                    ),
                ),
            ])
        })
        .collect();
    cbor::Value::Array(records)
}

/// Mixed ASCII and CJK text of about 256 KiB: each 39-byte unit carries ASCII
/// data, CJK ideographs and fullwidth capitals, the compatibility forms the
/// NFKC pass actually maps.
fn mixed_ascii_cjk() -> String {
    const TARGET: usize = 256 * 1024;
    const UNIT: &str = "data 中華文字 ＡＢＣＤＥ 0123 ";
    UNIT.repeat(TARGET / UNIT.len())
}

/// A scanner under measurement and the byte its input ends at.
type ScanCase = (&'static str, fn(&[u8]) -> Option<usize>, u8, u8);

fn bench_scan(c: &mut Bench) {
    // (name, scanner, the clean byte a run is made of, the byte that ends it)
    let cases: [ScanCase; 4] = [
        ("trivia", find_first_trivia, b' ', b'?'),
        ("iri_body", find_first_iri_body_special, b'a', b'>'),
        ("json_string", find_first_json_string_special, b'a', b'"'),
        ("xml", find_first_xml_special, b'a', b'<'),
    ];
    for (run, label) in [(4096_usize, "long"), (7, "token")] {
        let mut group = c.benchmark_group(format!("lex_scan_{label}"));
        group.throughput(Throughput::Bytes(run as u64 + 1));
        for (name, scan, clean, end) in cases {
            let mut input = vec![clean; run];
            input.push(end);
            group.bench_function(name, |bencher| {
                bencher.iter(|| {
                    let at = scan(black_box(&input)).expect("the input ends at a member");
                    black_box(at);
                });
            });
        }
        group.finish();
    }
}

fn bench_json_escape(c: &mut Bench) {
    let mut clean = "a".repeat(4096);
    clean.push('"');
    let mixed =
        "caf\u{e9} \"quoted\"\tline\n\u{1}\u{7f}\u{85} \u{4e2d}\u{6587} \u{1f431} ".repeat(64);
    let spellings = [
        ("minimal", JsonEscapes::Minimal),
        ("short_forms", JsonEscapes::ShortForms),
        ("controls", JsonEscapes::Controls),
        ("ascii", JsonEscapes::Ascii),
    ];
    for (label, value) in [("clean", &clean), ("mixed", &mixed)] {
        let mut group = c.benchmark_group(format!("lex_json_escape_{label}"));
        group.throughput(Throughput::Bytes(value.len() as u64));
        for (name, escapes) in spellings {
            let mut out = String::with_capacity(value.len() * 6);
            group.bench_function(name, |bencher| {
                bencher.iter(|| {
                    out.clear();
                    push_body(&mut out, black_box(value), escapes);
                    black_box(out.len());
                });
            });
        }
        group.finish();
    }
}

fn bench_json_reader(c: &mut Bench) {
    let document = seeded_json_document();
    let mut group = c.benchmark_group("lex_json_reader_gb");
    group.throughput(Throughput::Bytes(document.len() as u64));
    group.bench_function("next_event", |bencher| {
        bencher.iter(|| {
            let mut reader = Reader::new(black_box(&document), Limits::DEFAULT);
            let mut events = 0usize;
            loop {
                let event = reader
                    .next_event()
                    .expect("the seeded document is well-formed");
                if let Event::End = event {
                    break;
                }
                events += 1;
            }
            black_box(events);
        });
    });
    group.finish();
}

fn bench_yaml(c: &mut Bench) {
    let value = json::read(&seeded_json_document()).expect("the seeded document is well-formed");
    let text = yaml::write(&value);
    let mut group = c.benchmark_group("lex_yaml_gb");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("write", |bencher| {
        bencher.iter(|| black_box(yaml::write(black_box(&value))));
    });
    group.bench_function("read", |bencher| {
        bencher.iter(|| {
            black_box(yaml::read(black_box(&text)).expect("the emitter's own text reads back"));
        });
    });
    group.finish();
}

fn bench_cbor_roundtrip(c: &mut Bench) {
    let value = seeded_cbor_value();
    let bytes = cbor::encode(&value);
    let mut group = c.benchmark_group("lex_cbor_roundtrip");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("encode", |bencher| {
        bencher.iter(|| black_box(cbor::encode(black_box(&value))));
    });
    group.bench_function("decode", |bencher| {
        bencher.iter(|| {
            black_box(
                cbor::decode(black_box(&bytes), cbor::Limits::DEFAULT)
                    .expect("the encoder's own bytes decode"),
            );
        });
    });
    group.finish();
}

fn bench_unicode_norm(c: &mut Bench) {
    let text = mixed_ascii_cjk();
    let mut group = c.benchmark_group("lex_unicode_norm");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("nfkc", |bencher| {
        bencher.iter(|| black_box(nfkc(black_box(&text))));
    });
    group.finish();
}

bench_group!(
    benches,
    bench_scan,
    bench_json_escape,
    bench_json_reader,
    bench_yaml,
    bench_cbor_roundtrip,
    bench_unicode_norm
);
bench_main!(benches);

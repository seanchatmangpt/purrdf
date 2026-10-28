// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! TOML 1.1 lossless codec hot path: the source-model scan (`analyze`) as
//! GB/s over seeded documents, and the verified RDF 1.2 byte-cover codec
//! (`project`, `decode_document`) over the same documents.
//!
//! Report-only, `cargo bench -p purrdf-toml --bench parse`. Fixtures are
//! deterministic xorshift64 streams at const seeds — no RNG dependency, no
//! clock — so every document is replayable byte-for-byte from its seed alone.

use purrdf_toml::{Profile, SourceDocument, analyze, decode_document, project};
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};
use purrdf_testkit::rng::xorshift64_next;

/// Table counts exercised: small enough to stay cache-resident, large enough
/// that per-row admission and the flat node arena dominate per-call overhead.
/// Eight seeded rows per table.
const TABLES: [usize; 3] = [64, 512, 4_096];

/// Table counts exercised by the codec rows (project + decode round trips the
/// whole document through RDF, so only the two smaller sizes run there).
const CODEC_TABLES: [usize; 2] = [64, 512];

/// Seed of the document corpus, `toml_doc` in hex.
const DOC_SEED: u64 = 0x746F_6D6C_5F64_6F63;

/// Append one seeded `key = value` row: integers, quoted strings, booleans,
/// floats and inline arrays, so per-value admission covers every scalar shape.
fn push_row(text: &mut String, key: &str, draw: u64) {
    text.push_str(key);
    text.push_str(" = ");
    match draw % 5 {
        0 => text.push_str(&((draw >> 8) % 1_000_000).to_string()),
        1 => text.push_str(if (draw >> 8) % 2 == 0 { "true" } else { "false" }),
        2 => {
            text.push('"');
            for _ in 0..(3 + (draw >> 8) % 12) {
                text.push((b'a' as u64 + (draw >> 16) % 26) as u8 as char);
            }
            text.push('"');
        }
        3 => text.push_str(&format!("{}", ((draw >> 8) % 100_000) as f64 / 8.0)),
        _ => {
            text.push('[');
            for value in 0..(1 + (draw >> 8) % 4) {
                if value > 0 {
                    text.push_str(", ");
                }
                text.push_str(&(value * ((draw >> 16) % 97)).to_string());
            }
            text.push(']');
        }
    }
    text.push_str("\r\n");
}

/// A seeded TOML document of `tables` tables at eight rows each.
fn document(tables: usize, seed: u64) -> String {
    let mut state = seed | 1;
    let mut text = String::new();
    for table in 0..tables {
        text.push_str("[table-");
        text.push_str(&table.to_string());
        text.push_str("]\r\n");
        for row in 0..8 {
            let mut key = String::from("key_");
            key.push_str(&row.to_string());
            push_row(&mut text, &key, xorshift64_next(&mut state));
        }
    }
    text
}

/// The source-model scan: GB/s over the seeded corpus. `Throughput::Bytes` is
/// the document length, so the rate is source bytes per second.
fn bench_analyze(c: &mut Bench) {
    let profile = Profile::standard().expect("standard profile builds");
    let mut group = c.benchmark_group("toml_analyze");
    for &tables in &TABLES {
        let text = document(tables, DOC_SEED);
        let source = SourceDocument {
            id: "https://example.org/seeded.toml",
            bytes: text.as_bytes(),
        };
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_function(BenchmarkId::new("analyze", tables), |b| {
            b.iter(|| analyze(black_box(source), &profile).expect("seeded document parses"));
        });
    }
    group.finish();
}

/// The verified codec: `project` (model to RDF dataset) and `decode_document`
/// (cover-law reconstruction back to the source text) on the same documents.
fn bench_codec(c: &mut Bench) {
    let profile = Profile::standard().expect("standard profile builds");
    let mut group = c.benchmark_group("toml_codec");
    // decode_document/512 (~833ms/iter) and project/512 (~749ms/iter) make the
    // default 100 samples a ~150s group; 30 samples lands ~25s. The 64-row cases
    // stay proportionally cheap. Report-only.
    group.sample_size(30);
    for &tables in &CODEC_TABLES {
        let text = document(tables, DOC_SEED);
        let model = analyze(
            SourceDocument {
                id: "https://example.org/seeded.toml",
                bytes: text.as_bytes(),
            },
            &profile,
        )
        .expect("seeded document parses");
        let id = model.id().to_owned();
        group.throughput(Throughput::Bytes(text.len() as u64));

        group.bench_function(BenchmarkId::new("project", tables), |b| {
            b.iter(|| project(black_box(&model), &profile).expect("seeded document projects"));
        });

        let dataset = project(&model, &profile).expect("seeded document projects");
        group.bench_function(BenchmarkId::new("decode_document", tables), |b| {
            b.iter(|| {
                decode_document(black_box(&dataset), black_box(&id), &profile)
                    .expect("cover reconstructs the seeded document");
            });
        });
    }
    group.finish();
}

bench_group!(benches, bench_analyze, bench_codec);
bench_main!(benches);

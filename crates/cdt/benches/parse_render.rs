// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! SEP-0009 composite-datatype hot path: the lexical scan
//! (`parse_list`/`parse_map`) as GB/s over the seeded lexical corpus, and the
//! canonical lexical mapping (`canonical_lexical`) as ns/op over the
//! already-scanned values.
//!
//! Report-only, `cargo bench -p purrdf-cdt --bench parse_render`. Fixtures are
//! deterministic xorshift64 streams at const seeds — no RNG dependency, no
//! clock — so every corpus is replayable byte-for-byte from its seed alone.

use purrdf_cdt::{canonical_lexical, parse_list, parse_map};
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};
use purrdf_testkit::rng::xorshift64_next;

/// The composite sizes exercised: small enough to stay cache-resident, large
/// enough that element dispatch, the numeric shorthands and the heap worklist
/// dominate per-call overhead.
const SIZES: [usize; 3] = [128, 2_048, 32_768];

/// Seed of the list corpus, `cdt_list` in hex.
const LIST_SEED: u64 = 0x6364_745F_6C69_7374;

/// Seed of the map corpus, `cdt_map0` in hex.
const MAP_SEED: u64 = 0x6364_745F_6D61_7030;

/// Append one seeded list/map element: integers dominate (the numeric
/// shorthand resolve path), with booleans, `null` and short strings mixed in
/// so element dispatch never collapses to one production.
fn push_element(text: &mut String, draw: u64) {
    match draw % 8 {
        0..=4 => text.push_str(&((draw >> 8) % 1_000_000).to_string()),
        5 => text.push_str(if (draw >> 8) % 2 == 0 {
            "true"
        } else {
            "false"
        }),
        6 => text.push_str("null"),
        _ => {
            text.push('"');
            for _ in 0..(1 + (draw >> 8) % 8) {
                text.push((b'a' as u64 + (draw >> 16) % 26) as u8 as char);
            }
            text.push('"');
        }
    }
}

/// A `cdt:List` lexical form of `n` seeded elements.
fn list_lexical(n: usize, seed: u64) -> String {
    let mut state = seed | 1;
    let mut text = String::from("[");
    for index in 0..n {
        if index > 0 {
            text.push(',');
        }
        push_element(&mut text, xorshift64_next(&mut state));
    }
    text.push(']');
    text
}

/// A `cdt:Map` lexical form of `n` seeded entries with pairwise-distinct
/// string keys (the entry index keeps them distinct); values are seeded.
fn map_lexical(n: usize, seed: u64) -> String {
    let mut state = seed | 1;
    let mut text = String::from("{");
    for index in 0..n {
        if index > 0 {
            text.push(',');
        }
        text.push_str("\"k");
        text.push_str(&index.to_string());
        text.push('"');
        text.push(':');
        push_element(&mut text, xorshift64_next(&mut state));
    }
    text.push('}');
    text
}

/// The scan: GB/s over the seeded lexical corpus, both composites at every
/// size. `Throughput::Bytes` is the lexical length, so the rate is bytes of
/// input scanned per second.
fn bench_scan(c: &mut Bench) {
    let mut group = c.benchmark_group("cdt_scan");
    for &n in &SIZES {
        let lexical = list_lexical(n, LIST_SEED);
        group.throughput(Throughput::Bytes(lexical.len() as u64));
        group.bench_function(BenchmarkId::new("list", n), |b| {
            b.iter(|| parse_list(black_box(&lexical)).expect("seeded list scans"));
        });

        let lexical = map_lexical(n, MAP_SEED);
        group.throughput(Throughput::Bytes(lexical.len() as u64));
        group.bench_function(BenchmarkId::new("map", n), |b| {
            b.iter(|| parse_map(black_box(&lexical)).expect("seeded map scans"));
        });
    }
    group.finish();
}

/// The canonical lexical mapping: ns/op per value, the already-scanned value
/// re-spelled per iteration. Map rows include the total-key-order entry walk.
fn bench_render(c: &mut Bench) {
    let mut group = c.benchmark_group("cdt_render");
    for &n in &SIZES {
        let lexical = list_lexical(n, LIST_SEED);
        let value = parse_list(&lexical).expect("seeded list scans");
        group.bench_function(BenchmarkId::new("list", n), |b| {
            b.iter(|| canonical_lexical(black_box(&value)));
        });

        let lexical = map_lexical(n, MAP_SEED);
        let value = parse_map(&lexical).expect("seeded map scans");
        group.bench_function(BenchmarkId::new("map", n), |b| {
            b.iter(|| canonical_lexical(black_box(&value)));
        });
    }
    group.finish();
}

bench_group!(benches, bench_scan, bench_render);
bench_main!(benches);

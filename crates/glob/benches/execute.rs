// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! Compiled-glob execution: matches/sec at constant workspace, metered.
//!
//! Report-only, `cargo bench -p purrdf-glob --bench execute`. Every pattern
//! compiles once outside the timed region; matching borrows the compiled
//! program and allocates nothing (the constant-workspace law), and each
//! decision's charged work units are accumulated and returned, so the meter
//! is part of the measured result rather than observed beside it. Subjects
//! are a deterministic xorshift64 path corpus at a const seed — no RNG
//! dependency, no clock — mixed so both positive and negative decisions run.

use purrdf_glob::{CompiledGlob, Mode};
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};
use purrdf_testkit::rng::xorshift64_next;

/// The compiled patterns: path-mode component stars, `**` trees, `?` scalars
/// and bracket classes, plus one text-mode surface with a trailing tree.
const PATTERNS: [(&str, &str, Mode); 4] = [
    ("components", "src/**/*.rs", Mode::Path),
    ("dated_logs", "logs/????-??-??/*.log", Mode::Path),
    ("classes", "assets/[a-f][0-9]*/thumb.png", Mode::Path),
    ("text_tree", "query=**&lang=??", Mode::Text),
];

/// Corpus size per pattern; one shared subject list, matched by each pattern.
const SUBJECTS: usize = 1_024;

/// Seed of the subject corpus, `glob_sub` in hex.
const SUBJECT_SEED: u64 = 0x676C_6F62_5F73_7562;

/// A seeded subject path: seeded depth from a small directory vocabulary, a
/// seeded stem, and a seeded extension, so positives and negatives mix.
fn subjects(count: usize, seed: u64) -> Vec<String> {
    const DIRS: [&str; 6] = ["src", "docs", "assets", "logs", "tests", "vendor"];
    const STEMS: [&str; 6] = ["alpha", "guide-1", "img-07", "2026-01-02", "thumb", "main"];
    const EXTS: [&str; 4] = ["rs", "md", "log", "png"];
    let mut state = seed | 1;
    (0..count)
        .map(|_| {
            let depth = 1 + (xorshift64_next(&mut state) % 3) as usize;
            let mut path = String::new();
            for _ in 0..depth {
                let draw = xorshift64_next(&mut state);
                path.push_str(DIRS[(draw % 6) as usize]);
                path.push('/');
            }
            let draw = xorshift64_next(&mut state);
            path.push_str(STEMS[(draw % 6) as usize]);
            path.push('.');
            path.push_str(EXTS[((draw >> 8) % 4) as usize]);
            path
        })
        .collect()
}

/// Compile once, then match every subject under each pattern. Each iteration
/// accumulates the meter (`MatchReport::work`) and the decision count, and
/// both leave through `black_box`, so no decision is optimized away.
fn bench_match(c: &mut Bench) {
    let corpus = subjects(SUBJECTS, SUBJECT_SEED);
    let mut group = c.benchmark_group("glob_execute");
    group.throughput(Throughput::Elements(corpus.len() as u64));
    for (label, pattern, mode) in PATTERNS {
        let compiled = CompiledGlob::compile(pattern, mode).expect("lane pattern compiles");
        group.bench_function(BenchmarkId::new("match_literal", label), |b| {
            b.iter(|| {
                let mut matched = 0_u64;
                let mut work = 0_u64;
                for subject in &corpus {
                    let report = compiled
                        .match_literal(black_box(subject), u64::MAX)
                        .expect("unbounded work allowance admits every decision");
                    matched += u64::from(report.matched);
                    work += report.work;
                }
                black_box((matched, work));
            });
        });
    }
    group.finish();
}

bench_group!(benches, bench_match);
bench_main!(benches);

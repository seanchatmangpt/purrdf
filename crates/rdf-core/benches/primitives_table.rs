// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! Kernel-level benchmark for `purrdf_core::primitives` and the CSV field
//! scanner's kernel pair (A4 audit lanes: the four `primitives.rs` kernels
//! had zero timing rows; `csv/scan.rs` had no per-kernel GB/s).
//!
//! Two groups, per the rate-vs-latency convention:
//!
//! * `primitives_kernel` — ns/op rows (no `Throughput`) for the four
//!   register-level kernels over seeded `u64` fixtures.
//! * `csv_scan_kernel` — GB/s rows (`Throughput::Bytes`) for the live
//!   `StopSet::find` dispatch vs the portable SWAR kernel over a 1 MiB
//!   seeded CSV-shaped haystack with the reader's stop bytes absent (full
//!   scan, no early exit), so the number is scan cost, not hit position.
//!
//! Fixtures are deterministic: the one-home LCG
//! `purrdf_hash::mix::lcg64_next`, never `rand`.
//!
//! NOTE on `clamped_scaling_u64`: it is superseded by branchless saturation
//! on the saturation path; it is benched anyway to keep the before/after row
//! (the branchy `checked_mul` fallback vs the predicated form it replaced).
//!
//! Report-only, `cargo bench -p purrdf-core --bench primitives_table`. No
//! timing or speedup threshold is asserted. `make bench` lane only —
//! excluded from `make check`.

use purrdf_core::csv::backend;
use purrdf_core::primitives::{
    bitset_intersect_u64, clamped_scaling_u64, sort_quad_lanes_branchless, swar_find_delimiter_u64,
};
use purrdf_hash::mix::{LCG64_MMIX_INCREMENT, lcg64_next};
use purrdf_testkit::bench::{Bench, Throughput, bench_group, bench_main, black_box};

/// The reader/writer stop set benched (delimiter, quote, CR, LF), as plain
/// bytes — `StopSet` is `pub(crate)`, so the bench goes through the
/// doc-hidden `csv::backend` adapters, which construct it internally.
const MEMBERS: &[u8] = b",\"\r\n";

/// Haystack length: 1 MiB, the CSV-shaped corpus the audit names.
const HAYSTACK_LEN: usize = 1024 * 1024;

/// Fill `dst` with deterministic LCG bytes drawn from `alphabet`.
fn fill_alphabet(dst: &mut [u8], mut state: u64, alphabet: &[u8]) {
    for byte in dst.iter_mut() {
        // High bits for the small-range projection (low LCG bits are weak).
        let draw = lcg64_next(&mut state, LCG64_MMIX_INCREMENT) >> 33;
        *byte = alphabet[(draw % alphabet.len() as u64) as usize];
    }
}

/// The bench haystack: 1 MiB over a printable alphabet that excludes every
/// stop byte, so `find` walks the whole slice (full-scan GB/s, no early
/// exit) — the worst case both kernels must survive.
fn haystack() -> Vec<u8> {
    let mut data = vec![b'x'; HAYSTACK_LEN];
    fill_alphabet(
        &mut data,
        0x00C5_F1E1_D5CA_2026,
        b"abcdefghijklmnopqrstuvwxyz0123456789",
    );
    data
}

fn bench_primitives(c: &mut Bench) {
    // Seeded fixtures: SWAR words with a mix of matching and non-matching
    // lanes; sort lanes in a shuffled (unsorted) order; bitset words with
    // ~50% popcount; scaling values chosen so the checked_mul sometimes
    // overflows (the saturation fallback is part of the measured cost).
    let mut state = 0x5EED_0001_u64;
    let words: Vec<u64> = (0..4096)
        .map(|_| lcg64_next(&mut state, LCG64_MMIX_INCREMENT))
        .collect();
    let lane_quads: Vec<[u64; 4]> = (0..1024)
        .map(|i| {
            [
                words[i * 4],
                words[i * 4 + 1],
                words[i * 4 + 2],
                words[i * 4 + 3],
            ]
        })
        .collect();

    let mut group = c.benchmark_group("primitives_kernel");

    group.bench_function("swar_find_delimiter_u64", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &word in &words {
                acc |= swar_find_delimiter_u64(word, b',');
            }
            black_box(acc)
        });
    });

    group.bench_function("sort_quad_lanes_branchless", |b| {
        b.iter(|| {
            let mut acc = [0_u64; 4];
            for &quad in &lane_quads {
                acc = sort_quad_lanes_branchless(quad);
            }
            black_box(acc)
        });
    });

    group.bench_function("bitset_intersect_u64", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &word in &words {
                acc |= bitset_intersect_u64(word, word.rotate_left(13));
            }
            black_box(acc)
        });
    });

    // Superseded by branchless saturation; benched as the before row of the
    // before/after pair (the overflow-retry branch is on the hot path here:
    // large LCG values overflow `checked_mul` against num = u64::MAX / 2).
    group.bench_function("clamped_scaling_u64", |b| {
        b.iter(|| {
            let mut acc = 0_u64;
            for &word in &words {
                acc = acc.wrapping_add(clamped_scaling_u64(word, u64::MAX / 2, 3));
            }
            black_box(acc)
        });
    });

    group.finish();
}

fn bench_csv_scan(c: &mut Bench) {
    let data = haystack();
    let named: Vec<(&'static str, backend::ScanKernel)> = backend::kernels()
        .into_iter()
        .filter(|(name, _)| *name == "dispatch" || *name == "portable")
        .collect();
    assert_eq!(named.len(), 2, "dispatch and portable must both exist");

    let mut group = c.benchmark_group("csv_scan_kernel");
    group.throughput(Throughput::Bytes(HAYSTACK_LEN as u64));
    for (name, kernel) in &named {
        group.bench_function(*name, |b| {
            b.iter(|| black_box(kernel(MEMBERS, &data)));
        });
    }
    group.finish();
}

bench_group!(benches, bench_primitives, bench_csv_scan);
bench_main!(benches);

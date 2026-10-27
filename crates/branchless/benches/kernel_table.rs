// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! The branchless kernel table — the crate's raison d'être, measured.
//!
//! Report-only, `cargo bench -p purrdf-branchless --bench kernel_table`.
//! Each row is a SELLABLE comparison: branchless vs the branchy std baseline
//! for the same operation, over deterministic fixtures (one-home LCG seeds;
//! the same seed produces byte-identical fixtures on every host, so numbers
//! are comparable across runs).

use purrdf_branchless::{eytzinger, hash, packing, sort16, sort_small, swar};
use purrdf_hash::mix::lcg64_next;
use purrdf_testkit::bench::{Bench, Throughput, bench_group, bench_main, black_box};

/// Deterministic fixture source (one-home LCG delegation).
struct FixtureRng(u64);

impl FixtureRng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        lcg64_next(&mut self.0, 1)
    }
}

const SEARCH_N: usize = 1 << 16;

fn eyt_fixture() -> (Vec<u64>, Vec<u64>) {
    let mut rng = FixtureRng::new(0xDE5EED_0001);
    let mut sorted = Vec::with_capacity(SEARCH_N);
    sorted.push(1u64 << 20);
    for _ in 1..SEARCH_N {
        let step = 1 + rng.next_u64() % 7;
        let next = sorted.last().expect("nonempty") + step;
        sorted.push(next);
    }
    let mut eyt = vec![0u64; SEARCH_N + 1];
    eytzinger::build_eytzinger_layout(&sorted, &mut eyt);
    (sorted, eyt)
}

fn csv_fixture() -> Vec<u8> {
    let mut rng = FixtureRng::new(0xDE5EED_0002);
    let mut v = Vec::with_capacity(1 << 20);
    while v.len() < (1 << 20) {
        v.extend_from_slice(&rng.next_u64().to_le_bytes());
        if v.len() % 9 == 0 {
            v.push(b',');
        }
    }
    v.truncate(1 << 20);
    v
}

fn coords_fixture() -> Vec<(u32, u32)> {
    let mut rng = FixtureRng::new(0xDE5EED_0003);
    (0..1_000_000)
        .map(|_| {
            (
                (rng.next_u64() >> 33) as u32,
                (rng.next_u64() >> 33) as u32,
            )
        })
        .collect()
}

fn bench_eytzinger_vs_binary_search(c: &mut Bench) {
    let (sorted, eyt) = eyt_fixture();
    let mut rng = FixtureRng::new(0xDE5EED_0004);
    let targets: Vec<u64> = (0..8192)
        .map(|_| sorted[(rng.next_u64() % SEARCH_N as u64) as usize])
        .collect();
    let mut group = c.benchmark_group("branchless_eytzinger_vs_binary_search");
    group.throughput(Throughput::Elements(8192));
    group.bench_function("64k_keys", |bencher| {
        bencher.iter(|| {
            let mut hits = 0u64;
            for &t in &targets {
                hits ^= eytzinger::search_eytzinger_u64(&eyt, t).map_or(0, |i| i as u64);
                hits ^= sorted.binary_search(&t).map_or(0, |i| i as u64);
            }
            black_box(hits)
        })
    });
    group.finish();
}

fn bench_sort_networks_vs_std(c: &mut Bench) {
    let mut rng = FixtureRng::new(0xDE5EED_0005);
    let mut seed16 = [0u64; 16];
    for slot in &mut seed16 {
        *slot = rng.next_u64();
    }
    let mut group = c.benchmark_group("branchless_sort_networks_vs_std");

    group.bench_function("sort8_8u32", |bencher| {
        bencher.iter(|| {
            let mut a = [
                seed16[0] as u32,
                seed16[1] as u32,
                seed16[2] as u32,
                seed16[3] as u32,
                seed16[4] as u32,
                seed16[5] as u32,
                seed16[6] as u32,
                seed16[7] as u32,
            ];
            sort_small::sort8(&mut a);
            black_box(a)
        })
    });

    group.bench_function("batcher_sort16_16u32", |bencher| {
        bencher.iter(|| {
            let mut a = [0u32; 16];
            for (i, slot) in a.iter_mut().enumerate() {
                *slot = seed16[i] as u32;
            }
            let a = sort16::batcher_sort16_u32(a);
            black_box(a)
        })
    });

    group.bench_function("std_sort_unstable_16u32", |bencher| {
        bencher.iter(|| {
            let mut a = [0u32; 16];
            for (i, slot) in a.iter_mut().enumerate() {
                *slot = seed16[i] as u32;
            }
            a.sort_unstable();
            black_box(a)
        })
    });
    group.finish();
}

fn bench_wyhash_throughput(c: &mut Bench) {
    let blob: Vec<u8> = (0..(1usize << 20)).map(|i| (i * 7) as u8).collect();
    let mut group = c.benchmark_group("branchless_wyhash_64");
    group.throughput(Throughput::Bytes(1 << 20));
    group.bench_function("1mib", |bencher| {
        bencher.iter(|| black_box(hash::wyhash_64(black_box(&blob), 0x5EED)))
    });
    group.finish();
}

fn bench_swar_find_delimiter(c: &mut Bench) {
    let csv = csv_fixture();
    let mut group = c.benchmark_group("branchless_swar_find_delimiter");
    group.throughput(Throughput::Bytes(1 << 20));
    group.bench_function("1mib", |bencher| {
        bencher.iter(|| {
            let mut hits = 0u64;
            for w in csv.chunks_exact(8) {
                let word = u64::from_le_bytes(w.try_into().expect("exact chunk"));
                hits += u64::from(swar::find_byte_mask_word(word, b',') != 0);
                hits += u64::from(swar::find_byte_mask_word(word, b'\n') != 0);
            }
            black_box(hits)
        })
    });
    group.finish();
}

fn bench_morton(c: &mut Bench) {
    let coords = coords_fixture();
    let mut group = c.benchmark_group("branchless_morton_2d");
    group.throughput(Throughput::Elements(1_000_000));
    group.bench_function("spread_compact_1m", |bencher| {
        bencher.iter(|| {
            let mut acc = 0u64;
            for (x, y) in &coords {
                let m = packing::morton_spread_2d(*x) | (packing::morton_spread_2d(*y) << 1);
                acc ^= u64::from(packing::morton_compact_2d(m));
            }
            black_box(acc)
        })
    });
    group.finish();
}

bench_group!(
    benches,
    bench_eytzinger_vs_binary_search,
    bench_sort_networks_vs_std,
    bench_wyhash_throughput,
    bench_swar_find_delimiter,
    bench_morton
);
bench_main!(benches);

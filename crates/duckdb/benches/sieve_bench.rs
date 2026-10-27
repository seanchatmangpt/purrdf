// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: CC-BY-4.0

#![allow(missing_docs)]

//! High-Throughput Physical Microbenchmark and Sieve Velocity Suite.
//!
//! This suite measures RAW SQL on plain IN-MEMORY DuckDB tables (CREATE TABLE
//! + INSERT data). It does NOT measure the SQL-in-RDF pipeline
//! (compile_turtle -> five-table Parquet views -> RDF triple projection); that
//! end-to-end path is measured by `benches/sql_rdf_e2e.rs`.
//!
//! Rows: SWAR scanning, raw analytical SQL sieving (PIVOT/ASOF/rank collapse
//! over in-memory tables), circuit breaker trips, and 128-byte framed
//! canonical Arrow IPC egress.

use purrdf_arrow::header::ArrowEgressHeader;
use purrdf_duckdb::PurRdfDuckDB;
use purrdf_hash::blake3;
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main};
use std::hint::black_box;

// ============================================================================
// 1. Branchless SWAR Scanning
// ============================================================================

#[inline(always)]
fn swar_scan(bytes: &[u8]) -> (usize, usize) {
    let mut zeros = 0usize;
    let mut uppers = 0usize;
    let chunks = bytes.chunks_exact(8);
    let remainder = chunks.remainder();

    for chunk in chunks {
        let v = u64::from_le_bytes(chunk.try_into().unwrap());
        // SWAR Zero Byte Detector: (v - 0x0101010101010101) & ~v & 0x8080808080808080
        let zero_mask = v.wrapping_sub(0x0101010101010101) & !v & 0x8080808080808080;
        zeros += (zero_mask.count_ones() as usize) / 8;

        // SWAR Uppercase ASCII Detector ('A'..='Z', 0x41..=0x5A)
        let sub_a = v.wrapping_sub(0x4141414141414141);
        let sub_z_plus_1 = v.wrapping_sub(0x5B5B5B5B5B5B5B5B);
        let upper_mask = (sub_a ^ sub_z_plus_1) & 0x8080808080808080;
        uppers += (upper_mask.count_ones() as usize) / 8;
    }

    for &b in remainder {
        if b == 0 {
            zeros += 1;
        }
        if b >= b'A' && b <= b'Z' {
            uppers += 1;
        }
    }

    (zeros, uppers)
}

fn bench_swar(c: &mut Bench) {
    let mut group = c.benchmark_group("swar_scan");
    const SIZES: &[usize] = &[64, 512, 1024, 4096, 16384];

    for &size in SIZES {
        let buffer = vec![b'A'; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(BenchmarkId::from_parameter(size), |bencher| {
            bencher.iter(|| black_box(swar_scan(black_box(&buffer))));
        });
    }
}

// ============================================================================
// 2. In-Memory Analytical Sieving (10k Transactions -> <= 64 quads)
// ============================================================================

fn setup_sieve_db() -> PurRdfDuckDB {
    let db = PurRdfDuckDB::open_in_memory().expect("open db");
    let conn = db.raw_connection();
    conn.execute_batch(
        r#"
        PRAGMA max_memory = '32MB';
        SET threads = 1;
        SET preserve_insertion_order = false;

        CREATE TABLE trades (
            id BIGINT PRIMARY KEY,
            entity_id VARCHAR,
            trade_ts TIMESTAMP,
            nominal DOUBLE,
            tenor VARCHAR
        );

        CREATE TABLE spreads (
            effective_ts TIMESTAMP,
            entity_id VARCHAR,
            spread DOUBLE
        );

        INSERT INTO trades
        SELECT 
            i AS id,
            'ENTITY_' || (i % 16) AS entity_id,
            TIMESTAMP '2026-03-01 00:00:00' + INTERVAL (i * 5) SECOND AS trade_ts,
            (100.0 + (i % 100) * 10.0) AS nominal,
            CASE WHEN i % 3 = 0 THEN '1M' WHEN i % 3 = 1 THEN '3M' ELSE '6M' END AS tenor
        FROM generate_series(1, 10000) g(i);

        INSERT INTO spreads VALUES
            (TIMESTAMP '2026-03-01 00:00:00', 'ENTITY_0', 0.05),
            (TIMESTAMP '2026-03-01 00:00:00', 'ENTITY_1', 0.08),
            (TIMESTAMP '2026-03-01 00:00:00', 'ENTITY_2', 0.12);
        "#,
    )
    .expect("setup trade tables");
    db
}

fn bench_analytical_sieve(c: &mut Bench) {
    let db = setup_sieve_db();
    let conn = db.raw_connection();
    let mut group = c.benchmark_group("analytical_sieve_10k");
    group.throughput(Throughput::Elements(10000));

    let query = r#"
        WITH pivoted_trades AS (
            PIVOT trades
            ON tenor IN ('1M', '3M', '6M')
            USING sum(nominal)
            GROUP BY id, entity_id, trade_ts
        ),
        ranked_anomalies AS (
            SELECT 
                p.id,
                p.entity_id,
                COALESCE(s.spread, 0.02) AS spread,
                row_number() OVER (PARTITION BY p.entity_id ORDER BY p.id DESC) AS rnk
            FROM pivoted_trades p
            ASOF LEFT JOIN spreads s
                ON p.entity_id = s.entity_id
                AND p.trade_ts >= s.effective_ts
        )
        SELECT id, entity_id, spread
        FROM ranked_anomalies
        WHERE rnk <= 3
        ORDER BY id DESC
        LIMIT 64;
    "#;

    group.bench_function("pivot_asof_qualify_collapse", |bencher| {
        bencher.iter(|| {
            let mut stmt = conn.prepare(query).expect("prepare");
            let rows = stmt
                .query_map([], |r| {
                    let id: i64 = r.get(0)?;
                    let entity: String = r.get(1)?;
                    let spread: f64 = r.get(2)?;
                    Ok((id, entity, spread))
                })
                .expect("execute");
            let mut count = 0;
            for r in rows {
                let _ = black_box(r.expect("row"));
                count += 1;
            }
            assert!(count <= 64);
            black_box(count)
        });
    });
}

// ============================================================================
// 3. Arrow IPC Wire Serialization & BLAKE3 Streaming Attestation
// ============================================================================

fn bench_arrow_egress(c: &mut Bench) {
    let mut group = c.benchmark_group("arrow_egress_batch");
    const BATCHES: &[usize] = &[16, 32, 48, 64];

    for &quad_count in BATCHES {
        let quad_size = 48usize;
        let payload_len = quad_count * quad_size;
        let total_byte_len = (payload_len as u64) + (ArrowEgressHeader::SIZE as u64) + 64;
        let payload = vec![0xABu8; payload_len];

        group.throughput(Throughput::Bytes(total_byte_len));
        group.bench_function(BenchmarkId::from_parameter(quad_count), |bencher| {
            bencher.iter(|| {
                let hash = blake3::hash(black_box(&payload));
                let digest = *hash.as_bytes();

                let header = ArrowEgressHeader {
                    magic: ArrowEgressHeader::MAGIC,
                    schema_version: ArrowEgressHeader::SCHEMA_VERSION,
                    flags: ArrowEgressHeader::FLAG_STRICT_PROJECTION_ORDER,
                    batch_count: 1,
                    payload_offset: ArrowEgressHeader::PAYLOAD_OFFSET,
                    payload_byte_len: payload_len as u64,
                    total_byte_len,
                    triples_digest: digest,
                    columnar_digest: digest,
                    reserved: [0u8; 24],
                };

                let header_bytes = header.to_bytes();
                let mut wire = Vec::with_capacity(total_byte_len as usize);
                wire.extend_from_slice(&header_bytes);
                wire.extend_from_slice(&payload);
                wire.extend_from_slice(&[0u8; 64]);
                black_box(wire)
            });
        });
    }
}

bench_group!(
    benches,
    bench_swar,
    bench_analytical_sieve,
    bench_arrow_egress
);
bench_main!(benches);

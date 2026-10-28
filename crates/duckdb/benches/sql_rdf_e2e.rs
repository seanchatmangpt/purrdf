// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: CC-BY-4.0

#![allow(missing_docs)]

//! SQL-in-RDF end-to-end pipeline velocity suite.
//!
//! Measures the buyer-facing SQL-in-RDF path that `sieve_bench` deliberately
//! does not: a deterministic seeded Turtle fixture is parsed, projected into
//! the canonical five-table Parquet schema, mounted as zero-copy virtual views
//! in the embedded DuckDB engine, queried through a `compile_turtle`-compiled
//! SQL-in-RDF AST, and the resulting rows are projected into RDF triples.
//!
//! Rows:
//!
//! - `sql_rdf_e2e/e2e_triples`: the full pipeline (stage Parquet -> open
//!   engine -> mount views -> evaluate projection -> count triples), reported
//!   as triples/sec.
//! - `sql_rdf_view_build/view_build`: the five-table Parquet view mount over a
//!   freshly opened engine, in milliseconds.
//! - `sql_rdf_projection/rows`: the compiled projection query plus triple
//!   iteration over mounted views, reported as rows/sec.
//! - `duckdb_cold_start/open_in_memory`: a cold `open_in_memory` engine open,
//!   in milliseconds; each opened connection is dropped outside the timed
//!   region through `iter_batched`'s output handling.

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;

use purrdf_columnar::{Compression, project};
use purrdf_duckdb::{DEFAULT_ROW_ENVELOPE, ExecutionResult, PurRdfDuckDB};
use purrdf_sql::CompiledQuery;
use purrdf_testkit::bench::{BatchSize, Bench, Throughput, bench_group, bench_main};

// ============================================================================
// Deterministic fixture (seeded LCG — no rand crate, no wall clock)
// ============================================================================

/// Contract count in the fixture: 64 contracts x 3 triples = 192 quads, well
/// above the candidate envelope so the pushed-down LIMIT is the binding bound.
const CONTRACTS: usize = 64;
/// The compiled query pushes `LIMIT 64` (candidate envelope), so the
/// projection returns exactly the envelope when the fixture can fill it.
const EXPECTED_ROWS: usize = DEFAULT_ROW_ENVELOPE;
/// s, p, o — one triple per selected column; the quads projection's columns
/// are required (non-null), so every row yields all three.
const TRIPLES_PER_QUAD_ROW: usize = 3;

/// Fixed fixture seed: the generated corpus is byte-identical every run.
const LCG_SEED: u64 = 0x005E_ED51_B7E0_0001;

/// The one-home seeded generator: [`purrdf_hash::mix::lcg64_next`] (Knuth's
/// MMIX constants — helpers-ledger job `lcg64-mmix`), the same step every
/// other bench fixture runs; deterministic by construction.
#[derive(Debug)]
struct Lcg(u64);

impl Lcg {
    const fn next_u64(&mut self) -> u64 {
        purrdf_hash::mix::lcg64_next(&mut self.0, purrdf_hash::mix::LCG64_MMIX_INCREMENT)
    }
}

/// The seeded Turtle fixture: `CONTRACTS` contracts with a type, a seeded
/// label word, and a seeded value each.
fn fixture_turtle() -> String {
    let mut rng = Lcg(LCG_SEED);
    let mut ttl = String::from(
        "@prefix ex: <http://example.org/> .\n\
         @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\n",
    );
    for i in 0..CONTRACTS {
        let label_len = 8 + (rng.next_u64() % 17) as usize;
        let mut label = String::with_capacity(label_len);
        for _ in 0..label_len {
            let byte = b'a' + (rng.next_u64() % 26) as u8;
            label.push(byte as char);
        }
        let value = 1_000 + rng.next_u64() % 900_000;
        let _ = write!(
            ttl,
            "ex:contract_{i:03} a ex:Contract ;\n    \
             rdfs:label \"{label}\" ;\n    \
             ex:value {value} .\n"
        );
    }
    ttl
}

// ============================================================================
// SQL-in-RDF query fixture and projection identity
// ============================================================================

/// The declarative SQL-in-RDF query: select s/p/o from the `quads` view with
/// the candidate-envelope filter. Same AST shape as the Parquet pipeline
/// court (`tests/chicago_sql_parquet_pipeline.rs`).
const QUERY_TTL: &str = r#"
    @prefix sql: <http://graphlaw.org/sql#> .
    @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
    @prefix ex: <http://example.org/queries#> .

    ex:Q a sql:SelectStatement ;
        sql:statementIndex 1 ;
        sql:fromTable [ sql:tableName "quads" ] ;
        sql:selectList (
            [ sql:columnName "s" ]
            [ sql:columnName "p" ]
            [ sql:columnName "o" ]
        ) ;
        sql:whereClause [
            sql:operator sql:GreaterThan ;
            sql:leftOperand [ sql:columnName "s" ] ;
            sql:rightOperand [ sql:literalValue "0"^^xsd:integer ]
        ] .
"#;

/// Query IRI passed through to the evaluator.
const QUERY_IRI: &str = "http://example.org/queries#Q";
/// Subject IRI prefix for projected row subjects.
const ROW_SUBJECT_PREFIX: &str = "http://example.org/row";
/// Predicate IRI prefix for projected column predicates.
const COLUMN_PREDICATE_PREFIX: &str = "http://example.org/col";

// ============================================================================
// Untimed staging helpers (real collaborators, no mocks)
// ============================================================================

/// A fresh scratch directory under the bench target's `CARGO_TARGET_TMPDIR`
/// (the testkit-owned enforced home for test temporary paths — one body,
/// removed when the returned [`purrdf_testkit::TempDir`] drops).
fn bench_temp_dir(tag: &str) -> purrdf_testkit::TempDir {
    purrdf_testkit::temp_dir!(format!("purrdf-duckdb-sql-rdf-e2e-{tag}-").as_str())
        .expect("create bench temp dir")
}

/// Real pipeline ingest stage: parse the Turtle fixture into a dataset,
/// project the canonical five-table schema, serialize Parquet, and write one
/// file per table into `dir`.
fn stage_parquet_views(turtle: &str, dir: &Path) {
    let dataset = purrdf_rdf::parse_dataset(turtle.as_bytes(), "text/turtle", None)
        .expect("parse turtle dataset");
    let blobs = purrdf_core::ContentStore::default();
    let projection = project(dataset.as_ref(), &blobs).expect("project five-table schema");
    let columnar_write = projection
        .to_parquet(Compression::Uncompressed)
        .expect("serialize five-table parquet");
    for (table, bytes) in columnar_write.files.iter() {
        std::fs::write(dir.join(table.file_name()), bytes)
            .expect("write parquet file");
    }
}

/// Evaluate the compiled projection query over `db`.
fn run_projection(db: &PurRdfDuckDB, compiled: &CompiledQuery) -> ExecutionResult {
    db.eval_ast_query(
        QUERY_IRI,
        compiled,
        ROW_SUBJECT_PREFIX,
        COLUMN_PREDICATE_PREFIX,
    )
    .expect("evaluate projection query")
}

/// Open a fresh engine, mount the five-table Parquet views from `dir`, and
/// run the projection once — the post-ingest half of the pipeline.
fn probe_pipeline(dir: &Path, compiled: &CompiledQuery) -> ExecutionResult {
    let db = PurRdfDuckDB::open_in_memory().expect("open in-memory duckdb");
    db.register_purrdf_parquet_views(dir)
        .expect("mount parquet views");
    run_projection(&db, compiled)
}

/// Pin the probe to the envelope contract: the fixture must fill the
/// pushed-down LIMIT and every row must project all three s/p/o triples.
/// A drift here fails loudly instead of silently skewing the declared
/// throughput.
fn assert_envelope(result: &ExecutionResult) {
    assert_eq!(
        result.rows_emitted, EXPECTED_ROWS,
        "projection must fill the candidate envelope ({EXPECTED_ROWS} rows) from the fixture"
    );
    assert_eq!(
        result.triples.len(),
        EXPECTED_ROWS * TRIPLES_PER_QUAD_ROW,
        "each projected row must yield {TRIPLES_PER_QUAD_ROW} s/p/o triples"
    );
}

// ============================================================================
// 1. End-to-end: stage Parquet -> open -> mount views -> project -> triples
// ============================================================================

fn bench_sql_rdf_e2e(c: &mut Bench) {
    let turtle = fixture_turtle();
    let dir = bench_temp_dir("e2e");
    let compiled = purrdf_sql::compile_turtle(QUERY_TTL).expect("compile SQL-in-RDF query");

    stage_parquet_views(&turtle, dir.path());
    let probe = probe_pipeline(dir.path(), &compiled);
    assert_envelope(&probe);
    let triples_per_iteration = probe.triples.len() as u64;

    let mut group = c.benchmark_group("sql_rdf_e2e");
    // Parquet re-stage + engine start per iter; stay inside the ceiling
    group.sample_size(10);
    group.throughput(Throughput::Elements(triples_per_iteration));
    group.bench_function("e2e_triples", |bencher| {
        bencher.iter(|| {
            stage_parquet_views(&turtle, dir.path());
            let db = PurRdfDuckDB::open_in_memory().expect("open in-memory duckdb");
            db.register_purrdf_parquet_views(dir.path())
                .expect("mount parquet views");
            let result = run_projection(&db, &compiled);
            assert_envelope(&result);
            black_box(result.triples.len())
        });
    });
    group.finish();
}

// ============================================================================
// 2. Five-table Parquet view build (mount only, engine open in setup)
// ============================================================================

fn bench_view_build(c: &mut Bench) {
    let turtle = fixture_turtle();
    let dir = bench_temp_dir("view-build");
    stage_parquet_views(&turtle, dir.path());

    let mut group = c.benchmark_group("sql_rdf_view_build");
    group.bench_function("view_build", |bencher| {
        bencher.iter_batched(
            || {
                let db = PurRdfDuckDB::open_in_memory().expect("open in-memory duckdb");
                (db, dir.path().to_path_buf())
            },
            |(db, dir)| {
                black_box(
                    db.register_purrdf_parquet_views(&dir)
                        .expect("mount parquet views"),
                )
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

// ============================================================================
// 3. Projection query rows/sec over mounted views
// ============================================================================

fn bench_projection_rows(c: &mut Bench) {
    let turtle = fixture_turtle();
    let dir = bench_temp_dir("projection");
    stage_parquet_views(&turtle, dir.path());
    let db = PurRdfDuckDB::open_in_memory().expect("open in-memory duckdb");
    db.register_purrdf_parquet_views(dir.path())
        .expect("mount parquet views");
    let compiled = purrdf_sql::compile_turtle(QUERY_TTL).expect("compile SQL-in-RDF query");

    let probe = run_projection(&db, &compiled);
    assert_envelope(&probe);

    let mut group = c.benchmark_group("sql_rdf_projection");
    group.throughput(Throughput::Elements(probe.rows_emitted as u64));
    group.bench_function("rows", |bencher| {
        bencher.iter(|| {
            let result = run_projection(&db, &compiled);
            assert_envelope(&result);
            black_box(result.triples.len())
        });
    });
    group.finish();
}

// ============================================================================
// 4. Cold engine open (connection dropped outside the timed region)
// ============================================================================

fn bench_open_in_memory_cold_start(c: &mut Bench) {
    let mut group = c.benchmark_group("duckdb_cold_start");
    group.bench_function("open_in_memory", |bencher| {
        bencher.iter_batched(
            || (),
            |()| black_box(PurRdfDuckDB::open_in_memory().expect("open in-memory duckdb")),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

bench_group!(
    benches,
    bench_sql_rdf_e2e,
    bench_view_build,
    bench_projection_rows,
    bench_open_in_memory_cold_start
);
bench_main!(benches);

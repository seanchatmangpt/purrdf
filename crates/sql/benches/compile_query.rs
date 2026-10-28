// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

// Bench targets are not public API, so the workspace `missing_docs` lint is
// not asked of their items.
#![allow(missing_docs)]

//! SQL-in-RDF admission hot path: [`purrdf_sql::compile_turtle`] — SHACL
//! validation against `ontologies/sql-ast.ttl` + the dialect pack, RDFC-1.0
//! canonicalization, BLAKE3 memo keying, and statement compilation.
//!
//! Report-only, `cargo bench -p purrdf-sql --bench compile_query` (`make bench`
//! lane). Fixtures are deterministic: a fixed wide-projection statement shape
//! seeded from a compile-time-constant table/column nomenclature — no RNG, no
//! clock — so every document is replayable byte-for-byte from its column count
//! alone.
//!
//! The crate compiles SQL-in-RDF graphs to parameterized SQL text; it exposes
//! no standalone execution engine (DuckDB handles live outside this crate), so
//! there is deliberately no execution group here.

use purrdf_sql::compile_turtle;
use purrdf_testkit::bench::{Bench, BenchmarkId, Throughput, bench_group, bench_main, black_box};

/// Projection widths exercised: 64 columns (~133 triples) keeps the per-column
/// admission cost visible; 240 columns (~485 triples) is the ~500-triple class
/// the OPT bench-goal named. Both stay inside every SHACL envelope (the shapes
/// cap per-property counts, not list length).
const COLUMNS: [usize; 2] = [64, 240];

/// The admitted statement graph for `columns` projections over one table, with
/// one integer WHERE literal. Byte-documented provenance: derived from the
/// `test_compile_turtle_select` fixture in `tests/sql_golden.rs` (same
/// prefixes, `sql:SelectStatement` class, `sql:statementIndex 1`, one
/// `sql:whereClause`), widened along the projection rdf:List only — every
/// other element is byte-identical to the golden fixture.
fn statement_document(columns: usize, literal: i64) -> String {
    let mut doc = String::from(
        "\
         @prefix sql: <http://graphlaw.org/sql#> .\n\
         @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
         @prefix ex: <http://example.org/queries#> .\n\
         \n\
         ex:Q a sql:SelectStatement ;\n\
         \x20   sql:statementIndex 1 ;\n\
         \x20   sql:fromTable [ sql:tableName \"telemetry_events\" ] ;\n\
         \x20   sql:selectList (\n",
    );
    for column in 0..columns {
        doc.push_str("        [ sql:columnName \"col_");
        doc.push_str(&column.to_string());
        doc.push_str("\" ]\n");
    }
    doc.push_str("    ) ;\n    sql:whereClause [\n");
    doc.push_str("        sql:operator sql:GreaterThan ;\n");
    doc.push_str("        sql:leftOperand [ sql:columnName \"latency_ms\" ] ;\n");
    doc.push_str("        sql:rightOperand [ sql:literalValue \"");
    doc.push_str(&literal.to_string());
    doc.push_str("\"^^xsd:integer ]\n    ] .\n");
    doc
}

/// Full admission path: SHACL validation + canonicalization + compile, on a
/// graph the memo has never seen. Each iteration renders a document with a
/// fresh WHERE literal (a monotone counter), so `h_ast = BLAKE3(Canon(G_AST))`
/// differs every time and the Charter-A04 memo provably misses — otherwise a
/// repeated document would measure the hit path, not the cold path.
/// `Throughput::Bytes` is the document length, so the rate is source bytes per
/// second of admitted SQL-in-RDF.
fn bench_compile_cold(c: &mut Bench) {
    let mut group = c.benchmark_group("sql_compile_cold");
    group.sample_size(20);
    for &columns in &COLUMNS {
        let literal = 400_i64;
        let doc = statement_document(columns, literal);
        group.throughput(Throughput::Bytes(doc.len() as u64));
        // Memo-miss proof: the literal advances once per iteration, so no two
        // iterations ever render the same canonical graph.
        let mut seen = 0_u64;
        group.bench_function(BenchmarkId::new("columns", columns), |b| {
            b.iter(|| {
                seen += 1;
                let doc = statement_document(columns, literal + seen as i64);
                compile_turtle(black_box(&doc)).expect("seeded statement compiles");
            });
        });
    }
    group.finish();
}

/// Memo-hit path: compile the SAME document every iteration. `compile_turtle`
/// still runs canonicalization + BLAKE3 before the lookup (the memo key is the
/// digest), but a hit skips SHACL validation, graph parsing and statement
/// compilation — this row measures exactly that residual cost. The first
/// iteration primes the memo. Same throughput contract as the cold group.
fn bench_compile_memo_hit(c: &mut Bench) {
    let mut group = c.benchmark_group("sql_compile_memo_hit");
    group.sample_size(20);
    for &columns in &COLUMNS {
        let doc = statement_document(columns, 400);
        group.throughput(Throughput::Bytes(doc.len() as u64));
        group.bench_function(BenchmarkId::new("columns", columns), |b| {
            b.iter(|| {
                compile_turtle(black_box(&doc)).expect("seeded statement compiles");
            });
        });
    }
    group.finish();
}

bench_group!(benches, bench_compile_cold, bench_compile_memo_hit);
bench_main!(benches);

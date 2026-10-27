// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! End-to-end Chicago sociable integration court testing the full pipeline:
//! 1. Ingest RDF dataset (purrdf).
//! 2. Project into 5-table Parquet profile (purrdf-columnar).
//! 3. Mount zero-copy virtual views in embedded DuckDB (PurRdfDuckDB).
//! 4. Lower declarative SQL-in-RDF AST into parameterized query (compile_turtle).
//! 5. Execute query on DuckDB against the Parquet tables.
//! 6. Verify result triples and candidate envelope compliance.

use purrdf_columnar::{Compression, project};
use purrdf_duckdb::PurRdfDuckDB;
use purrdf_sql::compile_turtle;
use purrdf_testkit::temp_dir;

#[test]
fn test_chicago_end_to_end_purrdf_sqlrdf_duckdb_pipeline() {
    let ttl_data = r#"
        @prefix ex: <http://example.org/> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

        ex:contract_001 a ex:Contract ;
            rdfs:label "Sovereign Analytics Master Agreement" ;
            ex:value 150000 .

        ex:contract_002 a ex:Contract ;
            rdfs:label "Air-gapped Cloud Tenant Provision" ;
            ex:value 75000 .
    "#;

    // 1. Ingest RDF dataset
    let dataset = purrdf_rdf::parse_dataset(ttl_data.as_bytes(), "text/turtle", None)
        .expect("parse turtle dataset");

    // 2. Project into 5-table Parquet schema
    let blobs = purrdf_core::ContentStore::default();
    let projection = project(dataset.as_ref(), &blobs).expect("project 5-table schema");
    let columnar_write = projection
        .to_parquet(Compression::Uncompressed)
        .expect("serialize to parquet tables");

    // Write to a temporary directory
    let temp = temp_dir!("chicago_pipeline").expect("temp dir");
    for (table, bytes) in columnar_write.files.iter() {
        std::fs::write(temp.path().join(table.file_name()), bytes).expect("write parquet file");
    }

    // 3. Mount zero-copy virtual views in embedded DuckDB
    let db = PurRdfDuckDB::open_in_memory().expect("open duckdb");
    db.register_purrdf_parquet_views(temp.path())
        .expect("mount 5-table parquet views");

    // 4. Verify quads table is queryable directly via DuckDB
    let quads_count: i64 = db
        .raw_connection()
        .query_row("SELECT count(*) FROM quads", [], |r| r.get(0))
        .expect("count quads");
    assert!(quads_count >= 2, "must have at least 2 quads");

    // 5. Lower declarative SQL-in-RDF query AST into parameterized SQL
    let query_ttl = r#"
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

    let compiled = compile_turtle(query_ttl).expect("compile SQL-in-RDF AST");
    assert!(
        compiled.sql.contains("LIMIT 64"),
        "must push down candidate envelope limit"
    );

    // 6. Execute query on DuckDB against mounted Parquet views
    let result = db
        .eval_ast_query(
            "http://example.org/queries#Q",
            &compiled,
            "http://example.org/row",
            "http://example.org/col",
        )
        .expect("evaluate query against parquet views");

    assert!(result.rows_emitted >= 2);
    assert!(!result.triples.is_empty());
}

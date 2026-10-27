// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

use purrdf_core::TermValue;
use purrdf_duckdb::{EvaluatorError, PurRdfDuckDB};
use purrdf_sql::{CompiledQuery, SqlLiteral};

#[test]
fn test_duckdb_row_projection_isolation() {
    let db = PurRdfDuckDB::open_in_memory().expect("open duckdb");
    db.raw_connection()
        .execute_batch(
            "CREATE TABLE contracts (id BIGINT, name VARCHAR, value DOUBLE);
             INSERT INTO contracts VALUES (1, 'Master Services Agreement', 50000.0);
             INSERT INTO contracts VALUES (2, 'Cloud Compute Provision', 25000.0);",
        )
        .expect("create and seed table");

    let query = CompiledQuery::new(
        "SELECT id, name, value FROM contracts WHERE value > $1 ORDER BY id".to_string(),
        vec![SqlLiteral::Float(30000.0)],
    );

    let result = db
        .eval_ast_query(
            "http://example.org/q1",
            &query,
            "http://example.org/contract",
            "http://example.org/prop",
        )
        .expect("evaluate query");

    assert_eq!(result.rows_emitted, 1);
    assert_eq!(result.triples.len(), 3);
    assert_eq!(
        result.triples[0].object,
        TermValue::Literal {
            lexical_form: "1".to_string(),
            datatype: "http://www.w3.org/2001/XMLSchema#integer".to_string(),
            language: None,
            direction: None,
        }
    );
    assert_eq!(
        result.triples[1].object,
        TermValue::Literal {
            lexical_form: "Master Services Agreement".to_string(),
            datatype: "http://www.w3.org/2001/XMLSchema#string".to_string(),
            language: None,
            direction: None,
        }
    );
}

#[test]
fn test_sieve_circuit_breaker_trips_past_envelope() {
    let db = PurRdfDuckDB::open_in_memory().expect("open duckdb");
    // Generate 100 rows
    db.raw_connection()
        .execute_batch("CREATE TABLE large_table AS SELECT range AS id FROM range(100);")
        .expect("seed 100 rows");

    let query = CompiledQuery::new("SELECT id FROM large_table".to_string(), vec![]);

    // Default envelope is 64; 100 rows must fail with RowEnvelopeExceeded
    let err = db
        .eval_ast_query(
            "http://example.org/q_large",
            &query,
            "http://example.org/row",
            "http://example.org/col",
        )
        .expect_err("envelope must trip at row 65");

    match err {
        EvaluatorError::RowEnvelopeExceeded {
            limit,
            observed,
            max,
        } => {
            assert_eq!(limit, "duckdb_result_rows");
            assert_eq!(observed, 65);
            assert_eq!(max, 64);
        }
        other => panic!("expected RowEnvelopeExceeded, got {other:?}"),
    }

    // ANTI-VACUITY MUTATION TWIN: Bounded to 64 succeeds cleanly
    let ok_result = db
        .eval_ast_query_bounded(
            "http://example.org/q_bounded",
            &query,
            "http://example.org/row",
            "http://example.org/col",
            100, // explicit envelope of 100
        )
        .expect("bounded query succeeds");
    assert_eq!(ok_result.rows_emitted, 100);
}

// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Edge-case and failure-mode conformance court for the purrdf analytical sieve.
//!
//! Evaluates three-valued logic, sparse PIVOT, UNPIVOT null pruning, schema drift
//! through UNION ALL BY NAME, boundary joins (ASOF left-edge, POSITIONAL cardinality mismatch),
//! unary/ternary operator parsing disambiguation, declarative LIMIT envelope refusal,
//! mid-materialization circuit breaker on row 65, and zero-dependency Arrow IPC framing.

use purrdf_arrow::{ArrowEgressHeader, ArrowEgressHeaderError, canon_arrow_empty_v1};
use purrdf_core::TermValue;
use purrdf_duckdb::{EvaluatorError, PurRdfDuckDB};
use purrdf_hash::blake3;
use purrdf_sql::{
    AstError, Operator, RefusalCode, SelectItem, SelectStatement, SetOpKind, SetOperationStatement,
    SqlExpr, SqlLiteral, TableRef, compile_turtle, refusal_arm,
};
use purrdf_testkit::temp_dir;

#[test]
fn test_section_1_three_valued_logic_null_reshaping_and_schema_drift() {
    let temp = temp_dir!("sparse_pivot_court").expect("temp dir");
    let db = PurRdfDuckDB::open_in_memory().expect("open duckdb");

    // ------------------------------------------------------------------------
    // Scenario 1.1: PIVOT Over Highly Sparse and Incomplete Predicate Spaces
    // ------------------------------------------------------------------------
    // Setup a table of 2,000 records where only 20% (400) have fx_spread.
    db.raw_connection()
        .execute_batch(
            "
        CREATE TABLE raw_trades (
            trade_id INTEGER,
            predicate VARCHAR,
            val DOUBLE
        );
        -- Insert 2000 trade records: 1600 base volume, 400 fx_spread
        INSERT INTO raw_trades
        SELECT i AS trade_id, 'volume' AS predicate, 100.0 + (i % 10) AS val
        FROM range(1, 2001) t(i);

        INSERT INTO raw_trades
        SELECT i AS trade_id, 'fx_spread' AS predicate, 0.0025 AS val
        FROM range(1, 401) t(i);
        ",
        )
        .expect("setup sparse trades");

    // PIVOT to rotate fx_spread into a dedicated column
    let pivot_sql = "
        PIVOT raw_trades
        ON predicate
        USING first(val)
        GROUP BY trade_id
        ORDER BY trade_id
        LIMIT 64;
    ";
    let compiled = purrdf_sql::CompiledQuery::raw_compiled(pivot_sql.to_string(), vec![]);
    let res = db
        .eval_ast_query_bounded(
            "http://graphlaw.org/test#sparse_pivot",
            &compiled,
            "http://example.org/trades",
            "http://example.org/prop",
            64,
        )
        .expect("pivot execution");

    assert_eq!(res.rows_emitted, 64);
    // Verified: Null values are NOT emitted as phantom zero literals.
    // For rows 1..64, volume exists for all 64 rows, fx_spread exists for 64 rows (since 1..400).
    // Let's test a slice where fx_spread is absent:
    let pivot_sparse_sql = "
        PIVOT (SELECT * FROM raw_trades WHERE trade_id BETWEEN 1000 AND 1050)
        ON predicate
        USING first(val)
        GROUP BY trade_id
        ORDER BY trade_id
        LIMIT 64;
    ";
    let compiled_sparse =
        purrdf_sql::CompiledQuery::raw_compiled(pivot_sparse_sql.to_string(), vec![]);
    let res_sparse = db
        .eval_ast_query_bounded(
            "http://graphlaw.org/test#sparse_pivot_nulls",
            &compiled_sparse,
            "http://example.org/trades",
            "http://example.org/prop",
            64,
        )
        .expect("pivot sparse execution");

    assert_eq!(res_sparse.rows_emitted, 51);
    // trade_id and volume exist (51 * 2 = 102), but fx_spread is NULL for all 51 rows.
    // Confirm zero fx_spread quads are emitted (no phantom zero literals):
    let fx_spread_triples: Vec<_> = res_sparse
        .triples
        .iter()
        .filter(|t| t.predicate.ends_with("fx_spread"))
        .collect();
    assert_eq!(
        fx_spread_triples.len(),
        0,
        "Null cells must not project phantom zero literals into the interning arena"
    );

    // ------------------------------------------------------------------------
    // Scenario 1.2: UNPIVOT with NULL Value Pruning and Value Retention
    // ------------------------------------------------------------------------
    db.raw_connection()
        .execute_batch(
            "
        CREATE TABLE wide_metrics (
            entity_id INTEGER,
            metric_a DOUBLE,
            metric_b DOUBLE,
            metric_c DOUBLE
        );
        INSERT INTO wide_metrics VALUES
            (1, 10.5, NULL, 30.2),
            (2, NULL, 20.1, NULL),
            (3, 15.0, 25.0, 35.0),
            (4, NULL, NULL, NULL);
        ",
        )
        .expect("setup wide metrics");

    let unpivot_sql = "
        UNPIVOT wide_metrics
        ON metric_a, metric_b, metric_c
        INTO
            NAME metric_name
            VALUE metric_val
        ORDER BY entity_id, metric_name
        LIMIT 64;
    ";
    let compiled_unpivot = purrdf_sql::CompiledQuery::raw_compiled(unpivot_sql.to_string(), vec![]);
    let res_unpivot = db
        .eval_ast_query_bounded(
            "http://graphlaw.org/test#unpivot_null_pruning",
            &compiled_unpivot,
            "http://example.org/entities",
            "http://example.org/prop",
            64,
        )
        .expect("unpivot execution");

    // Total non-null cells: Row 1 has 2, Row 2 has 1, Row 3 has 3, Row 4 has 0 -> 6 rows
    assert_eq!(res_unpivot.rows_emitted, 6);
    // Confirm zero quads contain NULL
    for triple in &res_unpivot.triples {
        if let TermValue::Literal { lexical_form, .. } = &triple.object {
            assert_ne!(lexical_form, "null");
            assert_ne!(lexical_form, "NULL");
        }
    }

    // ------------------------------------------------------------------------
    // Scenario 1.3: Schema Mismatch Reconciliation via UNION BY NAME
    // ------------------------------------------------------------------------
    db.raw_connection()
        .execute_batch(
            "
        CREATE TABLE primary_quads (
            s VARCHAR,
            p VARCHAR,
            o VARCHAR,
            g VARCHAR
        );
        CREATE TABLE aux_quads (
            s VARCHAR,
            o VARCHAR,
            p VARCHAR,
            confidence_score DOUBLE
        );
        INSERT INTO primary_quads VALUES
            ('http://ex.org/s1', 'http://ex.org/p1', 'val1', 'http://ex.org/g1');
        INSERT INTO aux_quads VALUES
            ('http://ex.org/s2', 'val2', 'http://ex.org/p2', 0.99);
        ",
        )
        .expect("setup union tables");

    let union_sql = "
        (SELECT s, p, o, g FROM primary_quads)
        UNION ALL BY NAME
        (SELECT s, o, p, confidence_score FROM aux_quads);
    ";
    let compiled_union = purrdf_sql::CompiledQuery::raw_compiled(union_sql.to_string(), vec![]);
    let res_union = db
        .eval_ast_query_bounded(
            "http://graphlaw.org/test#union_by_name",
            &compiled_union,
            "http://example.org/union",
            "http://example.org/prop",
            64,
        )
        .expect("union by name execution");

    assert_eq!(res_union.rows_emitted, 2);
    // Primary row missing confidence_score is projected without confidence_score predicate
    let row1_triples: Vec<_> = res_union
        .triples
        .iter()
        .filter(|t| t.subject.ends_with("row_1"))
        .collect();
    let row2_triples: Vec<_> = res_union
        .triples
        .iter()
        .filter(|t| t.subject.ends_with("row_2"))
        .collect();

    assert!(
        row1_triples
            .iter()
            .all(|t| !t.predicate.ends_with("confidence_score"))
    );
    assert!(
        row2_triples
            .iter()
            .any(|t| t.predicate.ends_with("confidence_score"))
    );
    // Attributes aligned by name (s, p, o), not physical column ordinal
    let p_in_row2 = row2_triples
        .iter()
        .find(|t| t.predicate.ends_with("/p"))
        .expect("found p in row 2");
    if let TermValue::Literal { lexical_form, .. } = &p_in_row2.object {
        assert_eq!(lexical_form, "http://ex.org/p2");
    } else {
        panic!("expected literal");
    }
}

#[test]
fn test_section_2_temporal_boundary_and_join_corner_cases() {
    let db = PurRdfDuckDB::open_in_memory().expect("open duckdb");

    // ------------------------------------------------------------------------
    // Scenario 2.1: ASOF JOIN with Zero Preceding Match (Left Edge Boundary)
    // ------------------------------------------------------------------------
    db.raw_connection()
        .execute_batch(
            "
        CREATE TABLE asset_trades (
            trade_id INTEGER,
            trade_time TIMESTAMP
        );
        CREATE TABLE fx_rates (
            fx_time TIMESTAMP,
            rate DOUBLE
        );
        -- Asset trade at 00:00:00Z
        INSERT INTO asset_trades VALUES (101, TIMESTAMP '2026-10-01 00:00:00');
        -- FX rates start strictly later at 01:00:00Z
        INSERT INTO fx_rates VALUES
            (TIMESTAMP '2026-10-01 01:00:00', 1.0850),
            (TIMESTAMP '2026-10-01 02:00:00', 1.0875);
        ",
        )
        .expect("setup asof tables");

    // ASOF LEFT JOIN: trade_time >= fx_time. Since all fx_time > trade_time, FX columns must be NULL.
    let asof_sql = "
        SELECT t.trade_id, t.trade_time, f.fx_time, f.rate
        FROM asset_trades t
        ASOF LEFT JOIN fx_rates f
        ON t.trade_time >= f.fx_time;
    ";
    let compiled_asof = purrdf_sql::CompiledQuery::raw_compiled(asof_sql.to_string(), vec![]);
    let res_asof = db
        .eval_ast_query_bounded(
            "http://graphlaw.org/test#asof_zero_preceding",
            &compiled_asof,
            "http://example.org/asof",
            "http://example.org/prop",
            64,
        )
        .expect("asof execution completes without panic or infinite rewind");

    assert_eq!(res_asof.rows_emitted, 1);
    // Matched fx columns evaluate to NULL, so only trade_id and trade_time project quads
    assert_eq!(res_asof.triples.len(), 2);
    assert!(
        res_asof
            .triples
            .iter()
            .any(|t| t.predicate.ends_with("/trade_id"))
    );
    assert!(
        res_asof
            .triples
            .iter()
            .any(|t| t.predicate.ends_with("/trade_time"))
    );
    assert!(
        res_asof
            .triples
            .iter()
            .all(|t| !t.predicate.ends_with("/rate"))
    );

    // ------------------------------------------------------------------------
    // Scenario 2.2: POSITIONAL JOIN Over Mismatched Row Cardinalities
    // ------------------------------------------------------------------------
    db.raw_connection()
        .execute_batch(
            "
        CREATE TABLE left_pos (id INTEGER, l_val VARCHAR);
        CREATE TABLE right_pos (id INTEGER, r_val VARCHAR);

        INSERT INTO left_pos
        SELECT i, 'left_' || i FROM range(1, 41) t(i); -- 40 rows

        INSERT INTO right_pos
        SELECT i, 'right_' || i FROM range(1, 16) t(i); -- 15 rows
        ",
        )
        .expect("setup positional join tables");

    let pos_sql = "
        SELECT l.id AS l_id, l.l_val, r.id AS r_id, r.r_val
        FROM left_pos l
        POSITIONAL JOIN right_pos r
        ORDER BY l.id
        LIMIT 64;
    ";
    let compiled_pos = purrdf_sql::CompiledQuery::raw_compiled(pos_sql.to_string(), vec![]);
    let res_pos = db
        .eval_ast_query_bounded(
            "http://graphlaw.org/test#positional_mismatch",
            &compiled_pos,
            "http://example.org/pos",
            "http://example.org/prop",
            64,
        )
        .expect("positional join execution");

    assert_eq!(res_pos.rows_emitted, 40);
    // Rows 1..15 have both left and right values; rows 16..40 have r_id and r_val as NULL
    let row10_triples: Vec<_> = res_pos
        .triples
        .iter()
        .filter(|t| t.subject.ends_with("row_10"))
        .collect();
    let row30_triples: Vec<_> = res_pos
        .triples
        .iter()
        .filter(|t| t.subject.ends_with("row_30"))
        .collect();

    assert_eq!(row10_triples.len(), 4, "Row 10 has 4 non-null values");
    assert_eq!(
        row30_triples.len(),
        2,
        "Row 30 has 2 non-null values (right side is NULL)"
    );
}

#[test]
fn test_section_3_ternary_and_unary_parsing_integrity_in_purrdf_sql() {
    // ------------------------------------------------------------------------
    // Scenario 3.1: Disambiguation of Unary Bitwise Not vs Infix Regex Match
    // ------------------------------------------------------------------------
    // 1. Unary Bitwise Not: ~mask_col
    let unary_not = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "devices".to_string(),
            alias: None,
            schema: None,
        },
        joins: vec![],
        select_list: vec![SelectItem::Expr(SqlExpr::Unary {
            op: Operator::BitwiseNot,
            expr: Box::new(SqlExpr::ColumnRef {
                column: "mask_col".to_string(),
                table_alias: None,
            }),
        })],
        where_clause: None,
        group_by: vec![],
        having: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };
    let compiled_not = unary_not.compile().expect("compile bitwise not");
    assert_eq!(compiled_not.sql, "SELECT ~mask_col FROM devices LIMIT 10");

    // 2. Infix Regex Match: lexical_col ~ '^[A-Z]+$'
    let infix_regex = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "tokens".to_string(),
            alias: None,
            schema: None,
        },
        joins: vec![],
        select_list: vec![SelectItem::Expr(SqlExpr::ColumnRef {
            column: "lexical_col".to_string(),
            table_alias: None,
        })],
        where_clause: Some(SqlExpr::Binary {
            op: Operator::RegexMatch,
            left: Box::new(SqlExpr::ColumnRef {
                column: "lexical_col".to_string(),
                table_alias: None,
            }),
            right: Box::new(SqlExpr::Literal(SqlLiteral::String("^[A-Z]+$".to_string()))),
        }),
        group_by: vec![],
        having: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };
    let compiled_regex = infix_regex.compile().expect("compile regex");
    assert!(
        compiled_regex.sql.contains("WHERE (lexical_col ~ $1)"),
        "Formatted SQL: {}",
        compiled_regex.sql
    );

    // 3. Infix Regex Case Match: lexical_col ~* '^[a-z]+$'
    let infix_regex_case = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "tokens".to_string(),
            alias: None,
            schema: None,
        },
        joins: vec![],
        select_list: vec![SelectItem::Expr(SqlExpr::ColumnRef {
            column: "lexical_col".to_string(),
            table_alias: None,
        })],
        where_clause: Some(SqlExpr::Binary {
            op: Operator::RegexCaseMatch,
            left: Box::new(SqlExpr::ColumnRef {
                column: "lexical_col".to_string(),
                table_alias: None,
            }),
            right: Box::new(SqlExpr::Literal(SqlLiteral::String("^[a-z]+$".to_string()))),
        }),
        group_by: vec![],
        having: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };
    let compiled_regex_case = infix_regex_case.compile().expect("compile regex case");
    assert!(
        compiled_regex_case
            .sql
            .contains("WHERE (lexical_col ~* $1)"),
        "Formatted SQL: {}",
        compiled_regex_case.sql
    );

    // ------------------------------------------------------------------------
    // Scenario 3.2: Lowering Ternary BETWEEN Clause from SQL-in-RDF
    // ------------------------------------------------------------------------
    let between_stmt = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "metrics".to_string(),
            alias: None,
            schema: None,
        },
        joins: vec![],
        select_list: vec![SelectItem::Expr(SqlExpr::ColumnRef {
            column: "target_col".to_string(),
            table_alias: None,
        })],
        where_clause: Some(SqlExpr::Between {
            expr: Box::new(SqlExpr::ColumnRef {
                column: "target_col".to_string(),
                table_alias: None,
            }),
            low: Box::new(SqlExpr::Literal(SqlLiteral::Integer(100))),
            high: Box::new(SqlExpr::Literal(SqlLiteral::Integer(500))),
        }),
        group_by: vec![],
        having: None,
        order_by: vec![],
        limit: Some(64),
        offset: None,
    };
    let compiled_between = between_stmt.compile().expect("compile between statement");
    assert!(
        compiled_between
            .sql
            .contains("WHERE target_col BETWEEN $1 AND $2"),
        "Formatted SQL: {}",
        compiled_between.sql
    );
    assert_eq!(
        compiled_between.params,
        vec![SqlLiteral::Integer(100), SqlLiteral::Integer(500)]
    );
}

#[test]
fn test_section_4_adversarial_ast_and_circuit_breaker_boundary_refusals() {
    // ------------------------------------------------------------------------
    // Scenario 4.1: Declarative LIMIT Pushdown Injection Defense
    // ------------------------------------------------------------------------
    let adversarial_query = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "transactions".to_string(),
            alias: None,
            schema: None,
        },
        joins: vec![],
        select_list: vec![SelectItem::Expr(SqlExpr::ColumnRef {
            column: "tx_id".to_string(),
            table_alias: None,
        })],
        where_clause: None,
        group_by: vec![],
        having: None,
        order_by: vec![],
        limit: Some(256), // Exceeds candidate envelope of 64
        offset: None,
    };

    let compile_err = adversarial_query
        .compile()
        .expect_err("must refuse limit > 64");
    assert_eq!(compile_err.code, RefusalCode::MalformedAst);
    assert_eq!(
        compile_err.ctx.shape_id,
        refusal_arm::LIMIT_EXCEEDS_ENVELOPE
    );
    assert_eq!(compile_err.ctx.num, 256);

    // ------------------------------------------------------------------------
    // Scenario 4.2: Mid-Materialization Circuit Breaker Exactly on Row 65
    // ------------------------------------------------------------------------
    let db = PurRdfDuckDB::open_in_memory().expect("open duckdb");
    db.raw_connection()
        .execute_batch(
            "
        CREATE TABLE large_scan AS
        SELECT i AS id, 'entity_' || i AS name FROM range(1, 101) t(i);
        ",
        )
        .expect("create 100 rows");

    let scan_query = purrdf_sql::CompiledQuery::raw_compiled(
        "SELECT id, name FROM large_scan;".to_string(),
        vec![],
    );
    // Envelope locked strictly to 64 rows
    let err = db
        .eval_ast_query_bounded(
            "http://graphlaw.org/test#envelope_saturation",
            &scan_query,
            "http://example.org/entity",
            "http://example.org/prop",
            64,
        )
        .expect_err("materialization must trip on row 65");

    match err {
        EvaluatorError::RowEnvelopeExceeded {
            limit,
            observed,
            max,
        } => {
            assert_eq!(limit, "duckdb_result_rows");
            assert_eq!(observed, 65, "Circuit breaker must trip on the 65th row");
            assert_eq!(max, 64);
        }
        other => panic!("expected RowEnvelopeExceeded, got {other:?}"),
    }
}

#[test]
fn test_section_5_zero_dependency_arrow_ipc_payload_integrity() {
    // ------------------------------------------------------------------------
    // Scenario 5.1: Arrow IPC Egress Framing for Empty Result Sets
    // ------------------------------------------------------------------------
    let null_digest = canon_arrow_empty_v1();
    assert_eq!(
        *null_digest,
        *blake3::hash(b"CANON_ARROW_EMPTY_V1").as_bytes(),
        "Canonical null digest matches BLAKE3(CANON_ARROW_EMPTY_V1)"
    );

    let empty_header = ArrowEgressHeader {
        magic: ArrowEgressHeader::MAGIC,
        schema_version: ArrowEgressHeader::SCHEMA_VERSION,
        flags: 0,
        batch_count: 0,
        payload_offset: ArrowEgressHeader::PAYLOAD_OFFSET,
        payload_byte_len: 0,
        total_byte_len: 192, // 128 header + 0 payload + 64 padding
        triples_digest: *null_digest,
        columnar_digest: *null_digest,
        reserved: [0u8; 24],
    };

    let wire_bytes = empty_header.to_bytes();
    assert_eq!(wire_bytes.len(), 128);
    assert_eq!(core::mem::align_of::<ArrowEgressHeader>(), 64);

    let parsed = ArrowEgressHeader::from_bytes(&wire_bytes).expect("parse empty header");
    assert_eq!(parsed.batch_count, 0);
    assert_eq!(parsed.payload_byte_len, 0);
    assert_eq!(parsed.total_byte_len, 192);
    assert_eq!(parsed.triples_digest, *null_digest);
    assert_eq!(parsed.columnar_digest, *null_digest);

    // ------------------------------------------------------------------------
    // Scenario 5.2: Maximum Envelope Payload (64 Quads)
    // ------------------------------------------------------------------------
    let mut payload = Vec::with_capacity(64 * 128);
    for i in 0..64 {
        let quad_str = format!(
            "<http://example.org/asset/{i:04}> <http://example.org/pred/wide_prop_{i:04}> \"Deterministic wide string literal payload {i} conforming to candidate envelope\" <http://example.org/graph/v1> .\n"
        );
        payload.extend_from_slice(quad_str.as_bytes());
    }

    // Ensure 64-byte alignment padding for data buffer
    let unaligned = payload.len() % 64;
    let pad_len = if unaligned > 0 { 64 - unaligned } else { 0 };
    payload.extend(std::iter::repeat(0u8).take(pad_len));

    let payload_len = payload.len() as u64;
    let payload_digest = *blake3::hash(&payload).as_bytes();

    let full_header = ArrowEgressHeader {
        magic: ArrowEgressHeader::MAGIC,
        schema_version: ArrowEgressHeader::SCHEMA_VERSION,
        flags: ArrowEgressHeader::FLAG_STRICT_PROJECTION_ORDER,
        batch_count: 1,
        payload_offset: ArrowEgressHeader::PAYLOAD_OFFSET,
        payload_byte_len: payload_len,
        total_byte_len: payload_len + 128 + 64,
        triples_digest: payload_digest,
        columnar_digest: payload_digest,
        reserved: [0u8; 24],
    };

    let header_bytes = full_header.to_bytes();
    let mut full_ipc_stream = Vec::new();
    full_ipc_stream.extend_from_slice(&header_bytes);
    full_ipc_stream.extend_from_slice(&payload);
    full_ipc_stream.extend(std::iter::repeat(0u8).take(64)); // 64-byte trailing padding

    assert_eq!(full_ipc_stream.len() as u64, full_header.total_byte_len);
    assert_eq!(
        full_ipc_stream.len() % 64,
        0,
        "Entire stream is 64-byte aligned"
    );

    let parsed_full = ArrowEgressHeader::from_bytes(&header_bytes).expect("parse full header");
    assert_eq!(parsed_full.batch_count, 1);
    assert_eq!(parsed_full.payload_byte_len, payload_len);
    assert_eq!(parsed_full.columnar_digest, payload_digest);
}

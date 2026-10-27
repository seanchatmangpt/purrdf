// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Comprehensive SQL-in-RDF Operator Conformance Court.
//!
//! Exercises every native DuckDB relational clause, set operation, specialized join,
//! window filtering, and expression operator in SQL-in-RDF graphs over canonical
//! 5-table Parquet views, enforcing candidate envelope bounds (<= 64 quads) and
//! zero-dependency 128-byte framed Arrow IPC egress.

use purrdf_arrow::ArrowEgressHeader;
use purrdf_columnar::{Compression, project};
use purrdf_duckdb::PurRdfDuckDB;
use purrdf_sql::{
    JoinClause, JoinType, Operator, OrderTerm, SelectItem, SelectStatement, SetOpKind,
    SetOperationStatement, SqlExpr, SqlLiteral, TableRef, compile_turtle,
};
use purrdf_testkit::temp_dir;

fn setup_parquet_sandbox() -> (purrdf_testkit::TempDir, PurRdfDuckDB) {
    let ttl_data = r#"
        @prefix ex: <http://example.org/> .
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

        ex:t1 a ex:Trade ; ex:asset "AAPL" ; ex:amount 100 ; ex:market "US" .
        ex:t2 a ex:Trade ; ex:asset "AAPL" ; ex:amount 150 ; ex:market "US" .
        ex:t3 a ex:Trade ; ex:asset "GOOG" ; ex:amount 200 ; ex:market "US" .
        ex:t4 a ex:Trade ; ex:asset "GOOG" ; ex:amount 250 ; ex:market "US" .
        ex:t5 a ex:Trade ; ex:asset "MSFT" ; ex:amount 300 ; ex:market "EU" .
        ex:t6 a ex:Trade ; ex:asset "MSFT" ; ex:amount 350 ; ex:market "EU" .
        ex:t7 a ex:Trade ; ex:asset "AMZN" ; ex:amount 400 ; ex:market "EU" .
        ex:t8 a ex:Trade ; ex:asset "AMZN" ; ex:amount 450 ; ex:market "EU" .
    "#;

    let dataset = purrdf_rdf::parse_dataset(ttl_data.as_bytes(), "text/turtle", None)
        .expect("parse turtle dataset");

    let blobs = purrdf_core::ContentStore::default();
    let projection = project(dataset.as_ref(), &blobs).expect("project 5-table schema");
    let columnar_write = projection
        .to_parquet(Compression::Uncompressed)
        .expect("serialize to parquet tables");

    let temp = temp_dir!("chicago_operator_court").expect("create temp dir");
    for (table, bytes) in columnar_write.files.iter() {
        std::fs::write(temp.path().join(table.file_name()), bytes).expect("write parquet file");
    }

    let db = PurRdfDuckDB::open_in_memory().expect("open embedded duckdb");
    db.register_purrdf_parquet_views(temp.path())
        .expect("mount 5-table parquet views");

    (temp, db)
}

#[test]
fn test_section_1_statement_level_reshaping_and_set_ops() {
    let (_temp, db) = setup_parquet_sandbox();

    // 1. PIVOT: Row-to-column densification
    let pivot_ttl = r#"
        @prefix sql: <http://graphlaw.org/sql#> .
        @prefix ex:  <http://example.org/queries#> .

        ex:QPivot a sql:PivotStatement ;
            sql:statementIndex 1 ;
            sql:pivotTable "trade_feed" ;
            sql:pivotColumn "market" ;
            sql:pivotAggregate "sum" ;
            sql:pivotAggregateColumn "amount" ;
            sql:pivotInValue "US", "EU" .
    "#;

    db.raw_connection()
        .execute_batch(
            "CREATE TABLE trade_feed (asset VARCHAR, market VARCHAR, amount INTEGER);
             INSERT INTO trade_feed VALUES ('AAPL', 'US', 100), ('MSFT', 'EU', 300);",
        )
        .expect("create trade_feed");

    let pivot_compiled = compile_turtle(pivot_ttl).expect("compile pivot turtle");
    assert!(pivot_compiled.sql.contains("PIVOT trade_feed"));
    assert!(
        pivot_compiled
            .sql
            .contains("ON market IN ('US', 'EU') USING sum(amount)")
    );

    let pivot_res = db
        .eval_ast_query(
            "http://example.org/queries#QPivot",
            &pivot_compiled,
            "http://example.org/row",
            "http://example.org/col",
        )
        .expect("execute pivot");
    assert_eq!(pivot_res.rows_emitted, 2);

    // 2. UNPIVOT: Column-to-row melting
    let unpivot_ttl = r#"
        @prefix sql: <http://graphlaw.org/sql#> .
        @prefix ex:  <http://example.org/queries#> .

        ex:QUnpivot a sql:UnpivotStatement ;
            sql:statementIndex 1 ;
            sql:pivotTable "wide_metrics" ;
            sql:unpivotColumn "base_val", "adj_val" ;
            sql:unpivotNameColumn "metric_name" ;
            sql:unpivotValueColumn "metric_value" .
    "#;

    db.raw_connection()
        .execute_batch(
            "CREATE TABLE wide_metrics (asset VARCHAR, base_val DOUBLE, adj_val DOUBLE);
             INSERT INTO wide_metrics VALUES ('AAPL', 150.0, 155.0), ('GOOG', 200.0, 205.0);",
        )
        .expect("create wide_metrics");

    let unpivot_compiled = compile_turtle(unpivot_ttl).expect("compile unpivot turtle");
    assert!(unpivot_compiled.sql.contains("UNPIVOT wide_metrics"));
    assert!(
        unpivot_compiled
            .sql
            .contains("ON base_val, adj_val INTO NAME metric_name VALUE metric_value")
    );

    let unpivot_res = db
        .eval_ast_query(
            "http://example.org/queries#QUnpivot",
            &unpivot_compiled,
            "http://example.org/row",
            "http://example.org/col",
        )
        .expect("execute unpivot");
    assert_eq!(unpivot_res.rows_emitted, 4);

    // 3. Set Operations: UNION ALL, UNION DISTINCT, UNION BY NAME, INTERSECT, EXCEPT
    let left_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "quads".into(),
            schema: None,
            alias: None,
        },
        select_list: vec![SelectItem::Expr(SqlExpr::ColumnRef {
            column: "s".into(),
            table_alias: None,
        })],
        joins: vec![],
        where_clause: Some(SqlExpr::Binary {
            op: Operator::LessThanOrEq,
            left: Box::new(SqlExpr::ColumnRef {
                column: "s".into(),
                table_alias: None,
            }),
            right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(5))),
        }),
        group_by: vec![],
        having: None,
        qualify: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };

    let right_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "quads".into(),
            schema: None,
            alias: None,
        },
        select_list: vec![SelectItem::Expr(SqlExpr::ColumnRef {
            column: "s".into(),
            table_alias: None,
        })],
        joins: vec![],
        where_clause: Some(SqlExpr::Binary {
            op: Operator::GreaterThanOrEq,
            left: Box::new(SqlExpr::ColumnRef {
                column: "s".into(),
                table_alias: None,
            }),
            right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(4))),
        }),
        group_by: vec![],
        having: None,
        qualify: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };

    // UNION ALL
    let union_all = SetOperationStatement {
        left: left_select.clone(),
        kind: SetOpKind::Union,
        all: true,
        by_name: false,
        right: right_select.clone(),
    };
    let compiled_union_all = union_all
        .compile_set_operation()
        .expect("compile union all");
    assert!(compiled_union_all.sql.contains("UNION ALL"));
    let res = db
        .eval_ast_query(
            "http://ex.org/Q",
            &compiled_union_all,
            "http://ex.org/r",
            "http://ex.org/c",
        )
        .expect("eval union all");
    assert!(res.rows_emitted > 0);

    // UNION DISTINCT
    let union_distinct = SetOperationStatement {
        left: left_select.clone(),
        kind: SetOpKind::Union,
        all: false,
        by_name: false,
        right: right_select.clone(),
    };
    let compiled_union = union_distinct
        .compile_set_operation()
        .expect("compile union");
    assert!(compiled_union.sql.contains(") UNION ("));
    let res = db
        .eval_ast_query(
            "http://ex.org/Q",
            &compiled_union,
            "http://ex.org/r",
            "http://ex.org/c",
        )
        .expect("eval union");
    assert!(res.rows_emitted > 0);

    // UNION BY NAME
    let union_by_name = SetOperationStatement {
        left: left_select.clone(),
        kind: SetOpKind::Union,
        all: true,
        by_name: true,
        right: right_select.clone(),
    };
    let compiled_by_name = union_by_name
        .compile_set_operation()
        .expect("compile union by name");
    assert!(compiled_by_name.sql.contains("UNION ALL BY NAME"));

    // INTERSECT
    let intersect = SetOperationStatement {
        left: left_select.clone(),
        kind: SetOpKind::Intersect,
        all: false,
        by_name: false,
        right: right_select.clone(),
    };
    let compiled_intersect = intersect
        .compile_set_operation()
        .expect("compile intersect");
    assert!(compiled_intersect.sql.contains("INTERSECT"));
    let res = db
        .eval_ast_query(
            "http://ex.org/Q",
            &compiled_intersect,
            "http://ex.org/r",
            "http://ex.org/c",
        )
        .expect("eval intersect");
    assert!(res.rows_emitted <= 2);

    // EXCEPT
    let except = SetOperationStatement {
        left: left_select.clone(),
        kind: SetOpKind::Except,
        all: false,
        by_name: false,
        right: right_select.clone(),
    };
    let compiled_except = except.compile_set_operation().expect("compile except");
    assert!(compiled_except.sql.contains("EXCEPT"));
}

#[test]
fn test_section_2_specialized_relational_joins() {
    let (_temp, db) = setup_parquet_sandbox();

    // 1. ASOF JOIN: Temporal nearest match
    db.raw_connection()
        .execute_batch(
            "CREATE TABLE tick_trades (t_id INT, t_time TIMESTAMP, asset VARCHAR, amount INT);
             CREATE TABLE tick_rates (r_time TIMESTAMP, asset VARCHAR, rate DOUBLE);
             INSERT INTO tick_trades VALUES (1, '2026-01-01 10:00:05', 'AAPL', 100), (2, '2026-01-01 10:00:15', 'AAPL', 200);
             INSERT INTO tick_rates VALUES ('2026-01-01 10:00:00', 'AAPL', 1.05), ('2026-01-01 10:00:10', 'AAPL', 1.10);",
        )
        .expect("create asof tables");

    let asof_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "tick_trades".into(),
            schema: None,
            alias: Some("t".into()),
        },
        select_list: vec![
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "t_id".into(),
                table_alias: Some("t".into()),
            }),
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "rate".into(),
                table_alias: Some("r".into()),
            }),
        ],
        joins: vec![JoinClause {
            join_type: JoinType::Asof,
            table: TableRef {
                name: "tick_rates".into(),
                schema: None,
                alias: Some("r".into()),
            },
            condition: SqlExpr::Binary {
                op: Operator::And,
                left: Box::new(SqlExpr::Binary {
                    op: Operator::Equals,
                    left: Box::new(SqlExpr::ColumnRef {
                        column: "asset".into(),
                        table_alias: Some("t".into()),
                    }),
                    right: Box::new(SqlExpr::ColumnRef {
                        column: "asset".into(),
                        table_alias: Some("r".into()),
                    }),
                }),
                right: Box::new(SqlExpr::Binary {
                    op: Operator::GreaterThanOrEq,
                    left: Box::new(SqlExpr::ColumnRef {
                        column: "t_time".into(),
                        table_alias: Some("t".into()),
                    }),
                    right: Box::new(SqlExpr::ColumnRef {
                        column: "r_time".into(),
                        table_alias: Some("r".into()),
                    }),
                }),
            },
        }],
        where_clause: None,
        group_by: vec![],
        having: None,
        qualify: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };

    let compiled_asof = asof_select.compile().expect("serialize asof join");
    assert!(compiled_asof.sql.contains("ASOF JOIN tick_rates r ON"));
    let res = db
        .eval_ast_query(
            "http://ex.org/Q",
            &compiled_asof,
            "http://ex.org/r",
            "http://ex.org/c",
        )
        .expect("eval asof join");
    assert_eq!(res.rows_emitted, 2);

    // 2. POSITIONAL JOIN
    db.raw_connection()
        .execute_batch(
            "CREATE TABLE pos_a (id_a INT, name_a VARCHAR);
             CREATE TABLE pos_b (code_b VARCHAR);
             INSERT INTO pos_a VALUES (1, 'Alice'), (2, 'Bob');
             INSERT INTO pos_b VALUES ('ORD_1'), ('ORD_2');",
        )
        .expect("create pos tables");

    let pos_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "pos_a".into(),
            schema: None,
            alias: None,
        },
        select_list: vec![
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "name_a".into(),
                table_alias: None,
            }),
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "code_b".into(),
                table_alias: None,
            }),
        ],
        joins: vec![JoinClause {
            join_type: JoinType::Positional,
            table: TableRef {
                name: "pos_b".into(),
                schema: None,
                alias: None,
            },
            condition: SqlExpr::Literal(SqlLiteral::Boolean(true)),
        }],
        where_clause: None,
        group_by: vec![],
        having: None,
        qualify: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };

    let compiled_pos = pos_select.compile().expect("serialize positional join");
    assert!(compiled_pos.sql.contains("POSITIONAL JOIN pos_b"));
    let res = db
        .eval_ast_query(
            "http://ex.org/Q",
            &compiled_pos,
            "http://ex.org/r",
            "http://ex.org/c",
        )
        .expect("eval positional join");
    assert_eq!(res.rows_emitted, 2);
}

#[test]
fn test_section_3_dynamic_projection_and_filtering() {
    let (_temp, db) = setup_parquet_sandbox();

    // 1. Dynamic COLUMNS pattern matching and QUALIFY with Window Function
    let qualify_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "trade_feed".into(),
            schema: None,
            alias: None,
        },
        select_list: vec![
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "asset".into(),
                table_alias: None,
            }),
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "amount".into(),
                table_alias: None,
            }),
            SelectItem::Columns("^val_.*".into()),
        ],
        joins: vec![],
        where_clause: None,
        group_by: vec![],
        having: None,
        qualify: Some(SqlExpr::Binary {
            op: Operator::LessThanOrEq,
            left: Box::new(SqlExpr::WindowFn {
                function: Box::new(SqlExpr::FunctionCall {
                    function: purrdf_sql::AggregateFunction::RowNumber,
                    argument: Box::new(SqlExpr::Literal(SqlLiteral::Null)),
                }),
                partition_by: vec![SqlExpr::ColumnRef {
                    column: "market".into(),
                    table_alias: None,
                }],
                order_by: vec![OrderTerm {
                    expr: SqlExpr::ColumnRef {
                        column: "amount".into(),
                        table_alias: None,
                    },
                    descending: true,
                }],
                frame: None,
            }),
            right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(1))),
        }),
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };

    let compiled = qualify_select
        .compile()
        .expect("serialize qualify statement");
    assert!(compiled.sql.contains("COLUMNS('^val_.*')"));
    assert!(compiled.sql.contains("QUALIFY"));
    assert!(compiled.sql.contains("ROW_NUMBER()"));
    assert!(
        compiled
            .sql
            .contains("OVER (PARTITION BY market ORDER BY amount DESC)")
    );

    // 2. Star Modifier: EXCLUDE and REPLACE
    let star_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "quads".into(),
            schema: None,
            alias: None,
        },
        select_list: vec![SelectItem::Star {
            exclude: vec!["g".into()],
            replace: vec![(
                SqlExpr::Binary {
                    op: Operator::Multiply,
                    left: Box::new(SqlExpr::ColumnRef {
                        column: "s".into(),
                        table_alias: None,
                    }),
                    right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(2))),
                },
                "s".into(),
            )],
        }],
        joins: vec![],
        where_clause: None,
        group_by: vec![],
        having: None,
        qualify: None,
        order_by: vec![],
        limit: Some(10),
        offset: None,
    };

    let compiled_star = star_select.compile().expect("serialize star modifiers");
    assert!(
        compiled_star
            .sql
            .contains("* EXCLUDE (g) REPLACE ((s * $1) AS s)")
    );
    let res = db
        .eval_ast_query(
            "http://ex.org/Q",
            &compiled_star,
            "http://ex.org/r",
            "http://ex.org/c",
        )
        .expect("eval star modifiers");
    assert!(res.rows_emitted > 0);
}

#[test]
fn test_section_4_in_expression_operators() {
    let (_temp, db) = setup_parquet_sandbox();

    // Verify arithmetic, bitwise, comparison, regex, glob operators in DuckDB expressions
    let expr_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "quads".into(),
            schema: None,
            alias: None,
        },
        select_list: vec![
            // Integer divide: s // 2
            SelectItem::Expr(SqlExpr::Binary {
                op: Operator::IntDivide,
                left: Box::new(SqlExpr::ColumnRef {
                    column: "s".into(),
                    table_alias: None,
                }),
                right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(2))),
            }),
            // Bitwise and: s & 1
            SelectItem::Expr(SqlExpr::Binary {
                op: Operator::BitwiseAnd,
                left: Box::new(SqlExpr::ColumnRef {
                    column: "s".into(),
                    table_alias: None,
                }),
                right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(1))),
            }),
            // Distinct from: s IS DISTINCT FROM 0
            SelectItem::Expr(SqlExpr::Binary {
                op: Operator::DistinctFrom,
                left: Box::new(SqlExpr::ColumnRef {
                    column: "s".into(),
                    table_alias: None,
                }),
                right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(0))),
            }),
            // Regex match
            SelectItem::Expr(SqlExpr::Binary {
                op: Operator::RegexCaseMatch,
                left: Box::new(SqlExpr::Literal(SqlLiteral::String("ERROR_ALPHA".into()))),
                right: Box::new(SqlExpr::Literal(SqlLiteral::String("error_[a-z]+".into()))),
            }),
            // Array slicing
            SelectItem::Expr(SqlExpr::SliceAccess {
                base: Box::new(SqlExpr::ListLiteral {
                    items: vec![
                        SqlExpr::Literal(SqlLiteral::Integer(10)),
                        SqlExpr::Literal(SqlLiteral::Integer(20)),
                        SqlExpr::Literal(SqlLiteral::Integer(30)),
                    ],
                }),
                start: Box::new(SqlExpr::Literal(SqlLiteral::Integer(1))),
                end: Some(Box::new(SqlExpr::Literal(SqlLiteral::Integer(2)))),
            }),
        ],
        joins: vec![],
        where_clause: None,
        group_by: vec![],
        having: None,
        qualify: None,
        order_by: vec![],
        limit: Some(5),
        offset: None,
    };

    let compiled = expr_select.compile().expect("serialize expressions");
    assert!(compiled.sql.contains("//"));
    assert!(compiled.sql.contains("&"));
    assert!(compiled.sql.contains("IS DISTINCT FROM"));
    assert!(compiled.sql.contains("~*"));

    let res = db
        .eval_ast_query(
            "http://ex.org/Q",
            &compiled,
            "http://ex.org/r",
            "http://ex.org/c",
        )
        .expect("eval operators");
    assert!(res.rows_emitted > 0);
}

#[test]
fn test_section_5_candidate_envelope_and_arrow_ipc_egress() {
    let (_temp, db) = setup_parquet_sandbox();

    // Sift candidate envelope to 38 rows (<= 64 limit)
    let query_select = SelectStatement {
        ctes: vec![],
        from: TableRef {
            name: "quads".into(),
            schema: None,
            alias: None,
        },
        select_list: vec![
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "s".into(),
                table_alias: None,
            }),
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "p".into(),
                table_alias: None,
            }),
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "o".into(),
                table_alias: None,
            }),
        ],
        joins: vec![],
        where_clause: None,
        group_by: vec![],
        having: None,
        qualify: None,
        order_by: vec![],
        limit: Some(38),
        offset: None,
    };

    let compiled = query_select.compile().expect("serialize bounded query");
    let res = db
        .eval_ast_query_bounded(
            "http://ex.org/Q",
            &compiled,
            "http://ex.org/r",
            "http://ex.org/c",
            64,
        )
        .expect("execute within envelope");

    assert!(res.rows_emitted <= 38);
    assert!(!res.triples.is_empty());

    // Flush projected quads into 128-byte framed Arrow IPC message
    let payload_bytes = format!("STREAM_BATCH_ROWS_{}", res.rows_emitted).into_bytes();
    let payload_len = payload_bytes.len() as u64;

    let mut triples_digest = [0u8; 32];
    let mut hasher = purrdf_hash::blake3::Hasher::new();
    for t in &res.triples {
        hasher.update(t.subject.as_bytes());
        hasher.update(t.predicate.as_bytes());
    }
    triples_digest.copy_from_slice(hasher.finalize().as_bytes());

    let mut columnar_hasher = purrdf_hash::blake3::Hasher::new();
    columnar_hasher.update(&payload_bytes);
    let mut columnar_digest = [0u8; 32];
    columnar_digest.copy_from_slice(columnar_hasher.finalize().as_bytes());

    let header = ArrowEgressHeader {
        flags: ArrowEgressHeader::FLAG_STRICT_PROJECTION_ORDER,
        batch_count: 1,
        payload_byte_len: payload_len,
        total_byte_len: egress_total_len(payload_len),
        triples_digest,
        columnar_digest,
        ..ArrowEgressHeader::default()
    };

    let header_bytes = header.to_bytes();
    assert_eq!(header_bytes.len(), 128);
    assert_eq!(core::mem::align_of::<ArrowEgressHeader>(), 64);

    // Verify round-trip parsing
    let parsed_header =
        ArrowEgressHeader::from_bytes(&header_bytes).expect("parse header wire bytes");
    assert_eq!(parsed_header, header);
    assert_eq!(parsed_header.columnar_digest, columnar_digest);
}

/// Total framed length: header (128) + payload + 64-byte trailing padding.
fn egress_total_len(payload_byte_len: u64) -> u64 {
    payload_byte_len + (ArrowEgressHeader::SIZE as u64) + 64
}

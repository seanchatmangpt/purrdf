// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

use purrdf_sql::{
    AggregateFunction, AstError, Operator, RefusalCode, SelectItem, SelectStatement, SqlExpr,
    SqlLiteral, TableRef, compile_turtle, refusal_arm,
};

#[test]
fn test_valid_identifier_compilation() {
    let stmt = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "telemetry_events".to_string(),
            alias: Some("t".to_string()),
            schema: None,
        },
        joins: vec![],
        select_list: vec![
            SelectItem::Expr(SqlExpr::ColumnRef {
                column: "batch_id".to_string(),
                table_alias: None,
            }),
            SelectItem::Expr(SqlExpr::Alias {
                expr: Box::new(SqlExpr::FunctionCall {
                    function: AggregateFunction::Sum,
                    argument: Box::new(SqlExpr::ColumnRef {
                        column: "cost".to_string(),
                        table_alias: None,
                    }),
                }),
                alias: "total_cost".to_string(),
            }),
        ],
        where_clause: Some(SqlExpr::Binary {
            op: Operator::GreaterThan,
            left: Box::new(SqlExpr::ColumnRef {
                column: "latency".to_string(),
                table_alias: None,
            }),
            right: Box::new(SqlExpr::Literal(SqlLiteral::Integer(500))),
        }),
        group_by: vec![SqlExpr::ColumnRef {
            column: "batch_id".to_string(),
            table_alias: None,
        }],
        having: None,
        order_by: Vec::new(),
        limit: Some(50),
        offset: None,
    };

    let compiled = stmt.compile().unwrap();
    assert_eq!(
        compiled.sql,
        "SELECT batch_id, SUM(cost) AS total_cost FROM telemetry_events t WHERE (latency > $1) GROUP BY batch_id LIMIT 50"
    );
    assert_eq!(compiled.params, vec![SqlLiteral::Integer(500)]);
}

#[test]
fn test_mandatory_candidate_envelope_pushdown() {
    let stmt_unbounded = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "events".to_string(),
            alias: None,
            schema: None,
        },
        joins: vec![],
        select_list: vec![SelectItem::Expr(SqlExpr::ColumnRef {
            column: "id".to_string(),
            table_alias: None,
        })],
        where_clause: None,
        group_by: vec![],
        having: None,
        order_by: Vec::new(),
        limit: None, // Unspecified -> must automatically push down LIMIT 64
        offset: None,
    };

    let compiled = stmt_unbounded.compile().unwrap();
    assert!(
        compiled.sql.contains("LIMIT 64"),
        "unbounded query must declaratively push down LIMIT 64: {}",
        compiled.sql
    );

    // Limit exceeding candidate envelope (> 64) must be refused at compile time
    let stmt_excessive = SelectStatement {
        limit: Some(100),
        ..stmt_unbounded
    };
    let err = stmt_excessive
        .compile()
        .expect_err("limit > 64 must be refused");
    assert_eq!(err.code, RefusalCode::MalformedAst);
    assert_eq!(err.ctx.shape_id, refusal_arm::LIMIT_EXCEEDS_ENVELOPE);
}

#[test]
fn test_injection_attempt_rejected() {
    let stmt = SelectStatement {
        ctes: vec![],
        qualify: None,
        from: TableRef {
            name: "events; DROP TABLE users; --".to_string(),
            alias: None,
            schema: None,
        },
        joins: vec![],
        select_list: vec![],
        where_clause: None,
        group_by: vec![],
        having: None,
        order_by: Vec::new(),
        limit: None,
        offset: None,
    };

    let result = stmt.compile();
    assert!(matches!(result, Err(e) if matches!(e.code, RefusalCode::InvalidIdentifier)));
}

#[test]
fn test_compile_turtle_select() {
    let ttl = r#"
        @prefix sql: <http://graphlaw.org/sql#> .
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
        @prefix ex: <http://example.org/queries#> .

        ex:Q a sql:SelectStatement ;
            sql:statementIndex 1 ;
            sql:fromTable [ sql:tableName "telemetry_events" ] ;
            sql:selectList ( [ sql:columnName "batch_id" ] ) ;
            sql:whereClause [
                sql:operator sql:GreaterThan ;
                sql:leftOperand [ sql:columnName "latency_ms" ] ;
                sql:rightOperand [ sql:literalValue "400"^^xsd:integer ]
            ] .
    "#;

    let compiled = compile_turtle(ttl).expect("compile turtle");
    assert!(
        compiled
            .sql
            .starts_with("SELECT batch_id FROM telemetry_events")
    );
    assert!(compiled.sql.contains("WHERE (latency_ms > $1)"));
    assert!(compiled.sql.contains("LIMIT 64"));
    assert_eq!(compiled.params, vec![SqlLiteral::Integer(400)]);
    assert!(compiled.ast_hash.is_some());
    assert!(compiled.canon_graph_bytes.is_some());
}

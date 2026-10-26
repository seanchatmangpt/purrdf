#![allow(missing_docs)]
#![allow(clippy::result_large_err)]

use crate::ast::{
    AggregateFunction, CompiledQuery, Cte, FrameSpec, JoinClause, JoinType, OrderTerm, ScopePos, SqlExpr,
    SqlLiteral, TableRef, validate_identifier, validate_qualified_identifier,
};
use crate::error::{refusal_arm, AstError, RefusalCode};

#[derive(Debug, Clone, PartialEq)]
pub struct SelectStatement {
    /// SQLRDF-25: `WITH [RECURSIVE] name (cols) AS (query), ...` arms,
    /// serialized before the main SELECT. Empty = no WITH clause.
    pub ctes: Vec<Cte>,
    pub from: TableRef,
    pub joins: Vec<JoinClause>,
    /// SQLRDF-25 Rows 1+2: projection items — plain expressions plus the
    /// star-modifier vocabulary (`* [EXCLUDE (...)] [REPLACE (...)]`) and the
    /// `COLUMNS('pattern')` item. The D1 empty-projection law is unchanged:
    /// an empty `select_list` refuses `EMPTY_PROJECTION` at extraction.
    pub select_list: Vec<SelectItem>,
    pub where_clause: Option<SqlExpr>,
    pub group_by: Vec<SqlExpr>,
    /// `sql:having` / `sql:havingClause` predicate (aggregates legal here).
    pub having: Option<SqlExpr>,
    /// `sql:orderBy` terms (aggregates legal here).
    pub order_by: Vec<OrderTerm>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    /// `sql:qualify` expression (SQLRDF-25): window-filtering predicate,
    /// rendered after HAVING, before ORDER BY/LIMIT. Window functions and
    /// aggregates are legal here; column-only predicates extract too.
    pub qualify: Option<SqlExpr>,
}

/// One projection item (SQLRDF-25 Rows 1+2): a plain expression, a star term
/// with EXCLUDE/REPLACE modifiers, or a `COLUMNS('pattern')` item.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectItem {
    Expr(SqlExpr),
    /// `* [EXCLUDE (col, …)] [REPLACE (expr AS name, …)]` — the only lawful
    /// carrier of the `sql:star` / `sql:exclude` / `sql:replace` vocabulary.
    Star {
        exclude: Vec<String>,
        replace: Vec<(SqlExpr, String)>,
    },
    /// `COLUMNS('pattern')` — regex form only; the lambda form refuses typed
    /// (`refusal_arm::COLUMNS_LAMBDA`) until specified.
    Columns(String),
}

impl From<SqlExpr> for SelectItem {
    fn from(expr: SqlExpr) -> Self {
        Self::Expr(expr)
    }
}

impl SelectStatement {
    const MAX_DEPTH: usize = 64;

    /// Serializes one projection item (SQLRDF-25 Rows 1+2). Star modifiers
    /// render DuckDB-exact: `* EXCLUDE (a, b) REPLACE (expr AS name)`; the
    /// COLUMNS pattern is single-quote-escaped verbatim, never concatenated
    /// SQL.
    fn serialize_select_item(
        item: &SelectItem,
        sql: &mut String,
        params: &mut Vec<SqlLiteral>,
        depth: usize,
    ) -> Result<(), AstError> {
        if depth > Self::MAX_DEPTH {
            return Err(AstError::RecursionLimitExceeded(Self::MAX_DEPTH));
        }
        match item {
            SelectItem::Expr(expr) => {
                Self::serialize_expr(expr, sql, params, depth, ScopePos::SelectList)
            }
            SelectItem::Star { exclude, replace } => {
                sql.push('*');
                if !exclude.is_empty() {
                    sql.push_str(" EXCLUDE (");
                    for (idx, col) in exclude.iter().enumerate() {
                        if idx > 0 {
                            sql.push_str(", ");
                        }
                        validate_identifier(col)?;
                        sql.push_str(col);
                    }
                    sql.push(')');
                }
                if !replace.is_empty() {
                    sql.push_str(" REPLACE (");
                    for (idx, (expr, name)) in replace.iter().enumerate() {
                        if idx > 0 {
                            sql.push_str(", ");
                        }
                        Self::serialize_expr(expr, sql, params, depth + 1, ScopePos::SelectList)?;
                        sql.push_str(" AS ");
                        validate_identifier(name)?;
                        sql.push_str(name);
                    }
                    sql.push(')');
                }
                Ok(())
            }
            SelectItem::Columns(pattern) => {
                sql.push_str("COLUMNS('");
                sql.push_str(&pattern.replace('\'', "''"));
                sql.push_str("')");
                Ok(())
            }
        }
    }

    /// Compiles the AST into parameterized SQL compatible with DuckDB
    /// prepared statements (`$1, $2, ...`).
    ///
    /// Returns the SQL text plus its typed parameter vector in
    /// [`CompiledQuery`]. Identifier validation runs before any
    /// serialization: a table/column/alias name with characters outside the
    /// validated identifier alphabet is a typed
    /// `[REFUSAL-01] MalformedAstRefusal` (see `docs/refusals.md`), never a
    /// panic or an injection surface. Note: this hand-built-AST path does
    /// not anchor a canonical graph; only [`compile_turtle`] produces the
    /// graph-backed form the receipt law keys on.
    pub fn compile(&self) -> Result<CompiledQuery, AstError> {
        let mut params = Vec::new();
        let mut sql = String::with_capacity(256);
        self.compile_into(&mut sql, &mut params)?;
        Ok(CompiledQuery::raw_compiled(sql, params))
    }

    /// Serializes this select (including any WITH clause) into `sql`,
    /// appending bound parameters to `params`. Nested CTE queries serialize
    /// with the SAME shared parameter vector: `serialize_expr` numbers each
    /// placeholder from `params.len()` at push time, so a CTE's literals
    /// occupy the lowest indices and the outer body continues after them —
    /// exactly DuckDB's global positional-prepared numbering.
    fn compile_into(&self, sql: &mut String, params: &mut Vec<SqlLiteral>) -> Result<(), AstError> {
        if !self.ctes.is_empty() {
            sql.push_str("WITH ");
            if self.ctes.iter().any(|cte| cte.recursive) {
                sql.push_str("RECURSIVE ");
            }
            for (idx, cte) in self.ctes.iter().enumerate() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                validate_identifier(&cte.name)?;
                sql.push_str(&cte.name);
                if !cte.columns.is_empty() {
                    sql.push('(');
                    for (col_idx, col) in cte.columns.iter().enumerate() {
                        if col_idx > 0 {
                            sql.push_str(", ");
                        }
                        validate_identifier(col)?;
                        sql.push_str(col);
                    }
                    sql.push(')');
                }
                sql.push_str(" AS (");
                cte.query.compile_into(sql, params)?;
                sql.push(')');
            }
            sql.push(' ');
        }

        validate_identifier(&self.from.name)?;
        sql.push_str("SELECT ");
        if self.select_list.is_empty() {
            sql.push('*');
        } else {
            for (idx, item) in self.select_list.iter().enumerate() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                Self::serialize_select_item(item, sql, params, 0)?;
            }
        }

        sql.push_str(" FROM ");
        if let Some(schema) = &self.from.schema {
            sql.push_str(schema);
            sql.push('.');
        }
        sql.push_str(&self.from.name);
        if let Some(alias) = &self.from.alias {
            sql.push(' ');
            sql.push_str(alias);
        }

        for join in &self.joins {
            validate_identifier(&join.table.name)?;
            if let Some(schema) = &join.table.schema {
                validate_qualified_identifier(schema)?;
            }
            if let Some(alias) = &join.table.alias {
                validate_identifier(alias)?;
            }
            sql.push(' ');
            sql.push_str(join.join_type.as_sql());
            sql.push(' ');
            if let Some(schema) = &join.table.schema {
                sql.push_str(schema);
                sql.push('.');
            }
            sql.push_str(&join.table.name);
            if let Some(alias) = &join.table.alias {
                sql.push(' ');
                sql.push_str(alias);
            }
            if join.join_type != JoinType::Positional {
                sql.push_str(" ON ");
                Self::serialize_expr(
                    &join.condition,
                    sql,
                    params,
                    0,
                    ScopePos::Predicate,
                )?;
            }
        }

        if let Some(where_clause) = &self.where_clause {
            sql.push_str(" WHERE ");
             Self::serialize_expr(where_clause, sql, params, 0, ScopePos::Predicate)?;
        }

        if !self.group_by.is_empty() {
            sql.push_str(" GROUP BY ");
            for (idx, item) in self.group_by.iter().enumerate() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                 Self::serialize_expr(item, sql, params, 0, ScopePos::GroupBy)?;
            }
        }

        if let Some(having) = &self.having {
            sql.push_str(" HAVING ");
             Self::serialize_expr(having, sql, params, 0, ScopePos::Having)?;
        }

        if let Some(qualify) = &self.qualify {
            sql.push_str(" QUALIFY ");
             Self::serialize_expr(qualify, sql, params, 0, ScopePos::Qualify)?;
        }

        if !self.order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            for (idx, term) in self.order_by.iter().enumerate() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                 Self::serialize_expr(&term.expr, sql, params, 0, ScopePos::OrderBy)?;
                if term.descending {
                    sql.push_str(" DESC");
                }
            }
        }

        // Declarative Candidate Envelope Bound (PRD §3):
        // Automatically inject LIMIT 64 if unspecified; refuse if declared limit > 64.
        const CANDIDATE_ENVELOPE_LIMIT: usize = 64;
        let limit = match self.limit {
            Some(l) if l > CANDIDATE_ENVELOPE_LIMIT => {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::LIMIT_EXCEEDS_ENVELOPE,
                ).with_num(l));
            }
            Some(l) => l,
            None => CANDIDATE_ENVELOPE_LIMIT,
        };
        sql.push_str(&format!(" LIMIT {}", limit));

        if let Some(offset) = self.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }
        Ok(())
    }

    pub(super) fn serialize_expr(
        expr: &SqlExpr,
        sql: &mut String,
        params: &mut Vec<SqlLiteral>,
        depth: usize,
        pos: ScopePos,
    ) -> Result<(), AstError> {
        if depth > Self::MAX_DEPTH {
            return Err(AstError::RecursionLimitExceeded(Self::MAX_DEPTH));
        }

        match expr {
            SqlExpr::ColumnRef {
                column,
                table_alias,
            } => {
                validate_identifier(column)?;
                if let Some(alias) = table_alias {
                    // Qualified references (`main.regional_sales.region_id`)
                    // validate per dot-separated part.
                    validate_qualified_identifier(alias)?;
                    sql.push_str(alias);
                    sql.push('.');
                }
                sql.push_str(column);
            }
            SqlExpr::Literal(lit) => {
                params.push(lit.clone());
                sql.push('$');
                sql.push_str(&params.len().to_string());
            }
            SqlExpr::Binary { op, left, right } => {
                sql.push('(');
                Self::serialize_expr(left, sql, params, depth + 1, pos)?;
                sql.push(' ');
                sql.push_str(op.as_sql());
                sql.push(' ');
                Self::serialize_expr(right, sql, params, depth + 1, pos)?;
                sql.push(')');
            }
            SqlExpr::FunctionCall { function, argument } => {
                if !pos.allows_aggregate() {
                    // Typed arm: `num` carries the aggregate discriminant that
                    // `aggregate_debug_name` renders on the cold Display path.
                    return Err(AstError::new(
                        RefusalCode::ScopeIntegrity,
                        refusal_arm::AGG_IN_POSITION,
                    )
                    .with_num(match function {
                        AggregateFunction::Count => 0,
                        AggregateFunction::Sum => 1,
                        AggregateFunction::Avg => 2,
                        AggregateFunction::Min => 3,
                        AggregateFunction::Max => 4,
                        // Zero-argument ranking function (SQLRDF-25); the
                        // cold-path debug name renders the generic aggregate.
                        AggregateFunction::RowNumber => 5,
                    }));
                }
                sql.push_str(function.as_sql());
                sql.push('(');
                if function == &AggregateFunction::RowNumber {
                    // Zero-argument ranking function (SQLRDF-25): the argument
                    // slot is structural (hand-built courts pass
                    // Literal(Null)); it never serializes.
                    sql.push(')');
                    return Ok(());
                }
                Self::serialize_expr(
                    argument,
                    sql,
                    params,
                    depth + 1,
                    ScopePos::AggregateArgument,
                )?;
                sql.push(')');
            }
            SqlExpr::WindowFn {
                function,
                partition_by,
                order_by,
                frame,
            } => {
                // SQLRDF-25: window functions are legal only in SELECT list,
                // ORDER BY and QUALIFY; under a predicate (WHERE / JOIN ON /
                // GROUP BY / HAVING) it is a typed REFUSAL-01, mirroring the
                // aggregate-position law.
                if !pos.allows_window() {
                    return Err(AstError::new(
                        RefusalCode::MalformedAst,
                        refusal_arm::WINDOW_FN_POSITION,
                    )
                    .with_ident("SqlExpr::WindowFn"));
                }
                // Serialize the inner function in the SAME position: an
                // aggregate window fn (SUM(x) OVER ...) keeps the caller's
                // aggregate legality; the OVER wrapper is what carries the
                // window semantics.
                Self::serialize_expr(function, sql, params, depth + 1, pos)?;
                sql.push_str(" OVER (");
                let mut first = true;
                if !partition_by.is_empty() {
                    sql.push_str("PARTITION BY ");
                    for (idx, part) in partition_by.iter().enumerate() {
                        if idx > 0 {
                            sql.push_str(", ");
                        }
                        Self::serialize_expr(part, sql, params, depth + 1, ScopePos::GroupBy)?;
                    }
                    first = false;
                }
                if !order_by.is_empty() {
                    if !first {
                        sql.push(' ');
                    }
                    sql.push_str("ORDER BY ");
                    for (idx, term) in order_by.iter().enumerate() {
                        if idx > 0 {
                            sql.push_str(", ");
                        }
                        Self::serialize_expr(&term.expr, sql, params, depth + 1, ScopePos::OrderBy)?;
                        if term.descending {
                            sql.push_str(" DESC");
                        }
                    }
                    first = false;
                }
                if let Some(frame) = frame {
                    if !first {
                        sql.push(' ');
                    }
                    serialize_frame(frame, sql, depth + 1)?;
                }
                sql.push(')');
            }
            SqlExpr::StructLiteral { fields } => {
                expr.validate_composite()?;
                sql.push('{');
                for (idx, (key, value)) in fields.iter().enumerate() {
                    if idx > 0 {
                        sql.push_str(", ");
                    }
                    sql.push('\'');
                    sql.push_str(&key.replace('\'', "''"));
                    sql.push_str("': ");
                    Self::serialize_expr(value, sql, params, depth + 1, pos)?;
                }
                sql.push('}');
            }
            SqlExpr::ListLiteral { items } => {
                expr.validate_composite()?;
                sql.push('[');
                for (idx, item) in items.iter().enumerate() {
                    if idx > 0 {
                        sql.push_str(", ");
                    }
                    Self::serialize_expr(item, sql, params, depth + 1, pos)?;
                }
                sql.push(']');
            }
            SqlExpr::IndexAccess { base, index } => {
                expr.validate_composite()?;
                Self::serialize_expr(base, sql, params, depth + 1, pos)?;
                sql.push('[');
                Self::serialize_expr(index, sql, params, depth + 1, pos)?;
                sql.push(']');
            }
            SqlExpr::SliceAccess { base, start, end } => {
                expr.validate_composite()?;
                Self::serialize_expr(base, sql, params, depth + 1, pos)?;
                sql.push('[');
                Self::serialize_expr(start, sql, params, depth + 1, pos)?;
                if let Some(end) = end {
                    sql.push(':');
                    Self::serialize_expr(end, sql, params, depth + 1, pos)?;
                }
                sql.push(']');
            }
            SqlExpr::Unary { op, expr } => {
                sql.push_str(op.as_sql());
                Self::serialize_expr(expr, sql, params, depth + 1, pos)?;
            }
            SqlExpr::Between { expr, low, high } => {
                Self::serialize_expr(expr, sql, params, depth + 1, pos)?;
                sql.push_str(" BETWEEN ");
                Self::serialize_expr(low, sql, params, depth + 1, pos)?;
                sql.push_str(" AND ");
                Self::serialize_expr(high, sql, params, depth + 1, pos)?;
            }
            SqlExpr::Alias { expr, alias } => {
                if pos == ScopePos::AggregateArgument {
                    return Err(AstError::new(
                        RefusalCode::ScopeIntegrity,
                        refusal_arm::ALIASED_AGG_ARG,
                    ));
                }
                validate_identifier(alias)?;
                Self::serialize_expr(expr, sql, params, depth + 1, pos)?;
                sql.push_str(" AS ");
                sql.push_str(alias);
            }
        }
        Ok(())
    }
}

/// SQLRDF-27 dialect gap #1: the set-operation combinator (`UNION`,
/// `INTERSECT`, `EXCEPT`). Rendered uppercase between parenthesized operands;
/// the missing/malformed token refusals are typed REFUSAL-01 arms
/// (`refusal_arm::SET_OP_KIND_TOKEN`), never a silent default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOpKind {
    Union,
    Intersect,
    Except,
}

impl SetOpKind {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Union => "UNION",
            Self::Intersect => "INTERSECT",
            Self::Except => "EXCEPT",
        }
    }

    /// Parses the `sql:setOperator` token (case-insensitive, matching the
    /// `sql:direction` ASC/DESC lexing law). Any other token is the typed
    /// REFUSAL-01 arm `SET_OP_KIND_TOKEN`.
    pub fn from_token(token: &str) -> Result<Self, AstError> {
        if token.eq_ignore_ascii_case("UNION") {
            Ok(Self::Union)
        } else if token.eq_ignore_ascii_case("INTERSECT") {
            Ok(Self::Intersect)
        } else if token.eq_ignore_ascii_case("EXCEPT") {
            Ok(Self::Except)
        } else {
            Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::SET_OP_KIND_TOKEN)
                    .with_ident(token),
            )
        }
    }
}

/// SQLRDF-27 dialect gap #1: a top-level set operation over two full
/// [`SelectStatement`] operands (`UNION [ALL] [BY NAME]`, `INTERSECT [ALL]`,
/// `EXCEPT [ALL]`). Compiled by [`SetOperationStatement::compile_set_operation`].
///
/// Engine-witnessed law (frozen-duckdb probe, this checkout): `BY NAME` is a
/// UNION-form flag — `INTERSECT BY NAME` is a real engine parser error, so a
/// `by_name` flag beside a non-UNION kind refuses typed at compile time
/// (`refusal_arm::SET_OP_BY_NAME_KIND`), never as a generic DB error at
/// prepare time. `ALL` is engine-legal with all three kinds.
#[derive(Debug, Clone, PartialEq)]
pub struct SetOperationStatement {
    pub left: SelectStatement,
    pub kind: SetOpKind,
    /// `sql:setAll true` → `ALL` (dedup suppression: `UNION ALL` keeps
    /// duplicate rows; `INTERSECT ALL`/`EXCEPT ALL` are engine-legal too).
    pub all: bool,
    /// `sql:byName true` → `BY NAME` (UNION-only; positional kinds refuse).
    pub by_name: bool,
    pub right: SelectStatement,
}

impl SetOperationStatement {
    /// Compiles the set operation to parameterized DuckDB SQL: each operand
    /// serializes in full (its own WITH clause included) inside parentheses,
    /// the combinator and flags render between them. Placeholder numbering is
    /// GLOBAL across both operands — `serialize_expr` numbers `$n` from the
    /// shared buffer's length at push time, and operands render left-then-
    /// right, so text order and buffer order agree exactly as DuckDB's
    /// positional prepared protocol requires.
    ///
    /// This is a composite (non-`SelectStatement`) arm: `CompiledQuery::
    /// raw_compiled` keeps `cacheable: false`, the C1-a memo never admits it,
    /// and `compile_turtle` stamps the receipt anchor
    /// `h_ast = BLAKE3(Canon(G_AST))` on the returned query unchanged
    /// (sql-rdf Rec. section 1.4 receipt law).
    pub fn compile_set_operation(&self) -> Result<CompiledQuery, AstError> {
        let mut params = Vec::new();
        let mut sql = String::with_capacity(256);
        sql.push('(');
        self.left.compile_into(&mut sql, &mut params)?;
        sql.push(')');
        sql.push(' ');
        sql.push_str(self.kind.as_sql());
        if self.all {
            sql.push_str(" ALL");
        }
        if self.by_name {
            sql.push_str(" BY NAME");
        }
        sql.push(' ');
        sql.push('(');
        self.right.compile_into(&mut sql, &mut params)?;
        sql.push(')');
        sql.push(';');
        // C1-a classification: composite statement arm, never cacheable.
        Ok(CompiledQuery::raw_compiled(sql, params))
    }
}

/// Serializes a window frame spec (SQLRDF-25): `ROWS BETWEEN a AND b` in the
/// two-bound form, `ROWS a` in the single-bound form.
fn serialize_frame(
    frame: &FrameSpec,
    sql: &mut String,
    depth: usize,
) -> Result<(), AstError> {
    let _ = depth;
    sql.push_str(frame.unit.as_sql());
    match &frame.end {
        Some(end) => {
            sql.push_str(" BETWEEN ");
            sql.push_str(&frame.start.as_sql());
            sql.push_str(" AND ");
            sql.push_str(&end.as_sql());
        }
        None => {
            sql.push(' ');
            sql.push_str(&frame.start.as_sql());
        }
    }
    Ok(())
}

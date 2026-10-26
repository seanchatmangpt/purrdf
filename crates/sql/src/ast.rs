#![allow(missing_docs)]
#![allow(clippy::result_large_err)]

use crate::error::{AstError, RefusalCode, refusal_arm};
use crate::serializer::SelectStatement;

/// Validates strict SQL identifier semantics to eliminate structural injection.
/// Conforms to `^[a-zA-Z_][a-zA-Z0-9_]*$`.
pub fn validate_identifier(ident: &str) -> Result<&str, AstError> {
    if ident.is_empty() {
        return Err(AstError::InvalidIdentifier(ident.to_string()));
    }
    let mut chars = ident.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(AstError::InvalidIdentifier(ident.to_string()));
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return Err(AstError::InvalidIdentifier(ident.to_string()));
        }
    }
    Ok(ident)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Equals,
    NotEquals,
    GreaterThan,
    GreaterThanOrEq,
    LessThan,
    LessThanOrEq,
    And,
    Or,
    Like,
    ILike,
    Add,
    Subtract,
    Multiply,
    Divide,
    IntDivide,
    Modulo,
    Power,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseNot,
    ShiftLeft,
    ShiftRight,
    DistinctFrom,
    NotDistinctFrom,
    RegexMatch,
    RegexCaseMatch,
    GlobMatch,
    /// JSON extraction operator `->` (SQLRDF-25 Row 5; `sql:jsonExtract`).
    /// Doubles as list/index access in DuckDB (shared with Row 4's
    /// IndexAccess render site).
    JsonExtract,
    /// JSON extraction-as-text operator `->>` (SQLRDF-25 Row 5;
    /// `sql:jsonExtractText`).
    JsonExtractText,
}

impl Operator {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Equals => "=",
            Self::NotEquals => "<>",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEq => ">=",
            Self::LessThan => "<",
            Self::LessThanOrEq => "<=",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Like => "LIKE",
            Self::ILike => "ILIKE",
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::IntDivide => "//",
            Self::Modulo => "%",
            Self::Power => "^",
            Self::BitwiseAnd => "&",
            Self::BitwiseOr => "|",
            Self::BitwiseXor => "#",
            Self::BitwiseNot => "~",
            Self::ShiftLeft => "<<",
            Self::ShiftRight => ">>",
            Self::DistinctFrom => "IS DISTINCT FROM",
            Self::NotDistinctFrom => "IS NOT DISTINCT FROM",
            Self::RegexMatch => "~",
            Self::RegexCaseMatch => "~*",
            Self::GlobMatch => "GLOB",
            Self::JsonExtract => "->",
            Self::JsonExtractText => "->>",
        }
    }

    /// Legacy axiom IRIs (`sql:Equals`, ...) stay legal (SQLRDF-01 alias window).
    pub fn from_iri(iri: &str) -> Result<Self, AstError> {
        match iri {
            "http://graphlaw.org/sql#Equals" => Ok(Self::Equals),
            "http://graphlaw.org/sql#NotEquals" => Ok(Self::NotEquals),
            "http://graphlaw.org/sql#GreaterThan" => Ok(Self::GreaterThan),
            "http://graphlaw.org/sql#GreaterThanOrEq" => Ok(Self::GreaterThanOrEq),
            "http://graphlaw.org/sql#LessThan" => Ok(Self::LessThan),
            "http://graphlaw.org/sql#LessThanOrEq" => Ok(Self::LessThanOrEq),
            "http://graphlaw.org/sql#And" => Ok(Self::And),
            "http://graphlaw.org/sql#Or" => Ok(Self::Or),
            "http://graphlaw.org/sql#Like" => Ok(Self::Like),
            "http://graphlaw.org/sql#ILike" => Ok(Self::ILike),
            "http://graphlaw.org/sql#Add" => Ok(Self::Add),
            "http://graphlaw.org/sql#Subtract" => Ok(Self::Subtract),
            "http://graphlaw.org/sql#Multiply" => Ok(Self::Multiply),
            "http://graphlaw.org/sql#Divide" => Ok(Self::Divide),
            "http://graphlaw.org/sql#IntDivide" => Ok(Self::IntDivide),
            "http://graphlaw.org/sql#Modulo" => Ok(Self::Modulo),
            "http://graphlaw.org/sql#Power" => Ok(Self::Power),
            "http://graphlaw.org/sql#BitwiseAnd" => Ok(Self::BitwiseAnd),
            "http://graphlaw.org/sql#BitwiseOr" => Ok(Self::BitwiseOr),
            "http://graphlaw.org/sql#BitwiseXor" => Ok(Self::BitwiseXor),
            "http://graphlaw.org/sql#BitwiseNot" => Ok(Self::BitwiseNot),
            "http://graphlaw.org/sql#ShiftLeft" => Ok(Self::ShiftLeft),
            "http://graphlaw.org/sql#ShiftRight" => Ok(Self::ShiftRight),
            "http://graphlaw.org/sql#DistinctFrom" => Ok(Self::DistinctFrom),
            "http://graphlaw.org/sql#NotDistinctFrom" => Ok(Self::NotDistinctFrom),
            "http://graphlaw.org/sql#RegexMatch" => Ok(Self::RegexMatch),
            "http://graphlaw.org/sql#RegexCaseMatch" => Ok(Self::RegexCaseMatch),
            "http://graphlaw.org/sql#GlobMatch" => Ok(Self::GlobMatch),
            // SQLRDF-25 Row 5: JSON extraction operators. Both the full IRI
            // form and the short `sql:` prefixed form admit to the same
            // operator; any other IRI falls through to the token table.
            "http://graphlaw.org/sql#JsonExtract" | "sql:jsonExtract" => Ok(Self::JsonExtract),
            "http://graphlaw.org/sql#JsonExtractText" | "sql:jsonExtractText" => {
                Ok(Self::JsonExtractText)
            }
            _ => Self::from_token(iri),
        }
    }

    /// Section 4 operator token enum. Any other token is REFUSAL-01 (`MalformedAstRefusal`).
    pub fn from_token(token: &str) -> Result<Self, AstError> {
        match token {
            "=" => Ok(Self::Equals),
            "!=" => Ok(Self::NotEquals),
            "<" => Ok(Self::LessThan),
            "<=" => Ok(Self::LessThanOrEq),
            ">" => Ok(Self::GreaterThan),
            ">=" => Ok(Self::GreaterThanOrEq),
            "AND" => Ok(Self::And),
            "OR" => Ok(Self::Or),
            "+" => Ok(Self::Add),
            "-" => Ok(Self::Subtract),
            "*" => Ok(Self::Multiply),
            "/" => Ok(Self::Divide),
            "//" => Ok(Self::IntDivide),
            "%" => Ok(Self::Modulo),
            "^" => Ok(Self::Power),
            "&" => Ok(Self::BitwiseAnd),
            "|" => Ok(Self::BitwiseOr),
            "#" => Ok(Self::BitwiseXor),
            "~" => Ok(Self::BitwiseNot),
            "<<" => Ok(Self::ShiftLeft),
            ">>" => Ok(Self::ShiftRight),
            "LIKE" => Ok(Self::Like),
            "ILIKE" => Ok(Self::ILike),
            "IS DISTINCT FROM" => Ok(Self::DistinctFrom),
            "IS NOT DISTINCT FROM" => Ok(Self::NotDistinctFrom),
            "~*" => Ok(Self::RegexCaseMatch),
            "GLOB" => Ok(Self::GlobMatch),
            // SQLRDF-25 Row 5: JSON operator tokens. Legal exactly where
            // ordinary operators are (Binary); the render site emits the
            // token verbatim.
            "->" => Ok(Self::JsonExtract),
            "->>" => Ok(Self::JsonExtractText),
            other => Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::OPERATOR_TOKEN)
                    .with_ident(other),
            ),
        }
    }
}

/// Operational scope of a term (sql-rdf Rec section 1.3/3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    View,
    Process,
}

impl Scope {
    pub fn from_iri(iri: &str) -> Result<Self, AstError> {
        match iri {
            "http://graphlaw.org/sql#ViewScope" => Ok(Self::View),
            "http://graphlaw.org/sql#ProcessScope" => Ok(Self::Process),
            other => Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::SCOPE_INDIVIDUAL)
                    .with_ident(other),
            ),
        }
    }
}

/// Syntactic position of an expression, driving scope-integrity enforcement
/// (section 1.3/5, REFUSAL-03): aggregates are illegal in predicates and
/// GROUP BY; nested aggregates are illegal everywhere; an explicit
/// sql:ProcessScope column may not surface in a projection list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopePos {
    SelectList,
    OrderBy,
    Having,
    Predicate,
    GroupBy,
    AggregateArgument,
    /// Aggregate argument beneath a PREDICATE-rooted aggregate (WHERE/JOIN/
    /// HAVING/GROUP BY): same aggregate-nesting semantics as AggregateArgument,
    /// but an explicit sql:scope on the argument is a laundering marker
    /// (TC-MUT-01 arm B) — under an output aggregate (SELECT/ORDER BY) the
    /// section 7 example (ProcessScope amount inside a projection-list
    /// aggregate) stays lawful.
    PredicateAggregateArgument,
    /// QUALIFY clause of a window query (SQLRDF-25): aggregates and window
    /// functions are legal here; rendered after HAVING, before ORDER BY.
    Qualify,
}

impl ScopePos {
    pub(super) fn allows_aggregate(self) -> bool {
        matches!(
            self,
            Self::SelectList | Self::OrderBy | Self::Having | Self::Qualify
        )
    }

    pub(super) fn is_view_surface(self) -> bool {
        matches!(self, Self::SelectList | Self::OrderBy)
    }
}

/// Position lattice for an aggregate ARGUMENT: under an output-position
/// aggregate (SELECT list / ORDER BY — the section 7 example, where a
/// ProcessScope column inside a projection-list aggregate is lawful view
/// output of an internal computation) the argument keeps the plain
/// AggregateArgument position; under a predicate-rooted aggregate
/// (WHERE/JOIN/HAVING/GROUP BY) the argument is
/// PredicateAggregateArgument, where an explicit sql:scope marker is
/// REFUSAL-03 (TC-MUT-01 arm B).
impl ScopePos {
    /// Window functions are legal in SELECT list, ORDER BY and QUALIFY
    /// (SQLRDF-25); a window function under a predicate is REFUSAL-01.
    pub(super) fn allows_window(self) -> bool {
        matches!(self, Self::SelectList | Self::OrderBy | Self::Qualify)
    }
}

/// Window frame unit (`sql:frameUnit`): ROWS or RANGE (SQLRDF-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameUnit {
    Rows,
    Range,
}

impl FrameUnit {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Rows => "ROWS",
            Self::Range => "RANGE",
        }
    }
}

/// One window frame bound (SQLRDF-25).
#[derive(Debug, Clone, PartialEq)]
pub enum FrameBound {
    UnboundedPreceding,
    Preceding(u64),
    CurrentRow,
    Following(u64),
    UnboundedFollowing,
}

impl FrameBound {
    /// Parses a frame-bound token (`CURRENT ROW`, `UNBOUNDED PRECEDING`,
    /// `UNBOUNDED FOLLOWING`, `<n> PRECEDING`, `<n> FOLLOWING`); anything
    /// else is a typed REFUSAL-01 (arm FRAME_BOUND).
    pub fn parse_token(token: &str) -> Result<Self, AstError> {
        let t = token.trim();
        match t {
            "CURRENT ROW" | "CURRENT_ROW" => Ok(Self::CurrentRow),
            "UNBOUNDED PRECEDING" | "UNBOUNDED_PRECEDING" => Ok(Self::UnboundedPreceding),
            "UNBOUNDED FOLLOWING" | "UNBOUNDED_FOLLOWING" => Ok(Self::UnboundedFollowing),
            _ => {
                let mut parts = t.split_whitespace();
                let (n, kind) = match (parts.next(), parts.next(), parts.next()) {
                    (Some(n), Some(kind), None) => (n, kind),
                    _ => {
                        return Err(AstError::new(
                            RefusalCode::MalformedAst,
                            refusal_arm::FRAME_BOUND,
                        )
                        .with_ident(token))
                    }
                };
                let n: u64 = n.parse().map_err(|_| {
                    AstError::new(RefusalCode::MalformedAst, refusal_arm::FRAME_BOUND)
                        .with_ident(token)
                })?;
                match kind.to_ascii_uppercase().as_str() {
                    "PRECEDING" => Ok(Self::Preceding(n)),
                    "FOLLOWING" => Ok(Self::Following(n)),
                    _ => Err(
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::FRAME_BOUND)
                            .with_ident(token),
                    ),
                }
            }
        }
    }

    pub(super) fn as_sql(&self) -> String {
        match self {
            Self::UnboundedPreceding => "UNBOUNDED PRECEDING".to_string(),
            Self::Preceding(n) => format!("{} PRECEDING", n),
            Self::CurrentRow => "CURRENT ROW".to_string(),
            Self::Following(n) => format!("{} FOLLOWING", n),
            Self::UnboundedFollowing => "UNBOUNDED FOLLOWING".to_string(),
        }
    }
}

/// Window frame specification (SQLRDF-25): ROWS/RANGE unit plus a start
/// bound and an optional end bound (BETWEEN form when the end is present).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSpec {
    pub unit: FrameUnit,
    pub start: FrameBound,
    pub end: Option<FrameBound>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlExpr {
    ColumnRef {
        column: String,
        table_alias: Option<String>,
    },
    Literal(SqlLiteral),
    Binary {
        op: Operator,
        left: Box<SqlExpr>,
        right: Box<SqlExpr>,
    },
    FunctionCall {
        function: AggregateFunction,
        argument: Box<SqlExpr>,
    },
    /// Window function (`fn(...) OVER (PARTITION BY ... ORDER BY ... frame)`,
    /// SQLRDF-25). `function` is the aggregate/scalar expression node; legal
    /// only in SELECT list, ORDER BY and QUALIFY — a window function under a
    /// predicate is a typed REFUSAL-01 (arm WINDOW_FN_POSITION).
    WindowFn {
        function: Box<SqlExpr>,
        partition_by: Vec<SqlExpr>,
        order_by: Vec<OrderTerm>,
        frame: Option<FrameSpec>,
    },
    Alias {
        expr: Box<SqlExpr>,
        alias: String,
    },
    /// Composite struct literal `{'k': v, ...}` (SQLRDF-25 Row 4,
    /// `sql:structLiteral`). Field keys are strict identifiers (validated at
    /// serialize time; the identifier charset excludes quotes, so emitting a
    /// key verbatim inside `'…'` is injection-safe). Depth-capped — see
    /// [`SqlExpr::validate_composite`].
    StructLiteral {
        fields: Vec<(String, SqlExpr)>,
    },
    /// Composite list literal `[a, b, ...]` (SQLRDF-25 Row 4,
    /// `sql:listLiteral`). Depth-capped.
    ListLiteral { items: Vec<SqlExpr> },
    /// Element access `base[index]` (SQLRDF-25 Row 4, `sql:indexAccess`) —
    /// the `->` operator doubles as index access in DuckDB, so this is the
    /// shared render site with Row 5's JsonExtract token.
    IndexAccess {
        base: Box<SqlExpr>,
        index: Box<SqlExpr>,
    },
    /// Slice access `base[start:end]` (SQLRDF-25 Row 4, `sql:sliceAccess`).
    SliceAccess {
        base: Box<SqlExpr>,
        start: Box<SqlExpr>,
        end: Option<Box<SqlExpr>>,
    },
    /// Unary expression (e.g. `~expr` for bitwise not).
    Unary {
        op: Operator,
        expr: Box<SqlExpr>,
    },
    /// Ternary `expr BETWEEN low AND high`.
    Between {
        expr: Box<SqlExpr>,
        low: Box<SqlExpr>,
        high: Box<SqlExpr>,
    },
}

/// Maximum nesting depth of composite literal / accessor nodes
/// (`StructLiteral` / `ListLiteral` / `IndexAccess` / `SliceAccess`).
/// Mirrors the depth-capped RDF-list unroll discipline (cap 64).
pub(super) const MAX_COMPOSITE_DEPTH: usize = 64;

impl SqlExpr {
    /// Validates composite literals and accessors (SQLRDF-25 Row 4):
    /// struct field keys must satisfy [`validate_identifier`] (a malformed
    /// member is the typed REFUSAL-01 arm `COMPOSITE_MEMBER`), and nesting
    /// beyond [`MAX_COMPOSITE_DEPTH`] refuses with `COMPOSITE_DEPTH`.
    /// Recursion is bounded by the same cap, so a hostile deep graph cannot
    /// overflow the stack.
    pub fn validate_composite(&self) -> Result<(), AstError> {
        fn walk(expr: &SqlExpr, depth: usize) -> Result<(), AstError> {
            if depth > MAX_COMPOSITE_DEPTH {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::COMPOSITE_DEPTH,
                )
                .with_num(depth));
            }
            match expr {
                SqlExpr::StructLiteral { fields } => {
                    for (key, value) in fields {
                        validate_identifier(key).map_err(|_| {
                            AstError::new(
                                RefusalCode::MalformedAst,
                                refusal_arm::COMPOSITE_MEMBER,
                            )
                            .with_ident(key)
                        })?;
                        walk(value, depth + 1)?;
                    }
                }
                SqlExpr::ListLiteral { items } => {
                    for item in items {
                        walk(item, depth + 1)?;
                    }
                }
                SqlExpr::IndexAccess { base, index } => {
                    walk(base, depth + 1)?;
                    walk(index, depth + 1)?;
                }
                SqlExpr::SliceAccess { base, start, end } => {
                    walk(base, depth + 1)?;
                    walk(start, depth + 1)?;
                    if let Some(end) = end {
                        walk(end, depth + 1)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        walk(self, 0)
    }
}

/// Position lattice for an aggregate ARGUMENT: under an output-position
/// aggregate (SELECT list / ORDER BY — the section 7 example, where a
/// ProcessScope column inside a projection-list aggregate is lawful view
/// output of an internal computation) the argument keeps the plain
/// AggregateArgument position; under a predicate-rooted aggregate
/// (WHERE/JOIN/HAVING/GROUP BY) the argument is
/// PredicateAggregateArgument, where an explicit sql:scope marker is
/// REFUSAL-03 (TC-MUT-01 arm B).
pub(super) fn aggregate_argument_pos(root: ScopePos) -> ScopePos {
    if root.is_view_surface() {
        ScopePos::AggregateArgument
    } else {
        ScopePos::PredicateAggregateArgument
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    /// Zero-argument ranking function (`ROW_NUMBER() OVER (...)`); serialized
    /// with empty parens (SQLRDF-25).
    RowNumber,
}

impl AggregateFunction {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
            Self::RowNumber => "ROW_NUMBER",
        }
    }

    pub fn from_iri(iri: &str) -> Result<Self, AstError> {
        match iri {
            "http://graphlaw.org/functions#COUNT" => Ok(Self::Count),
            "http://graphlaw.org/functions#SUM" => Ok(Self::Sum),
            "http://graphlaw.org/functions#AVG" => Ok(Self::Avg),
            "http://graphlaw.org/functions#MIN" => Ok(Self::Min),
            "http://graphlaw.org/functions#MAX" => Ok(Self::Max),
            "http://graphlaw.org/functions#ROW_NUMBER" => Ok(Self::RowNumber),
            _ => Err(AstError::UnsupportedTerm(iri.to_string())),
        }
    }
}

/// LANE L5 minimal unblock (compile-freeze SLA, disclosed): name-form
/// constructor referenced by lane L2's in-flight section 7 function form
/// (`sql_ast.rs:703`). Accepts the SQL function name (case-insensitive) or
/// its `graphlaw.org/functions#` IRI; anything else is the typed
/// `AstError::UnsupportedTerm` refusal. Remove when lane L2 lands their own.
pub(super) fn aggregate_from_name(name: &str) -> Result<AggregateFunction, AstError> {
    let upper = name.to_ascii_uppercase();
    let token = upper
        .strip_prefix("HTTP://GRAPHLAW.ORG/FUNCTIONS#")
        .unwrap_or(&upper);
    match token {
        "COUNT" => Ok(AggregateFunction::Count),
        "SUM" => Ok(AggregateFunction::Sum),
        "AVG" => Ok(AggregateFunction::Avg),
        "MIN" => Ok(AggregateFunction::Min),
        "MAX" => Ok(AggregateFunction::Max),
        "ROW_NUMBER" => Ok(AggregateFunction::RowNumber),
        _ => Err(AstError::UnsupportedTerm(name.to_string())),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlLiteral {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
    /// Optional schema qualifier (`sql:schemaName`); rendered as `schema.name`.
    pub schema: Option<String>,
}

/// Validates a dotted identifier (`main.regional_sales`): every dot-separated
/// part must satisfy [`validate_identifier`].
pub fn validate_qualified_identifier(ident: &str) -> Result<(), AstError> {
    if ident.is_empty() {
        return Err(AstError::InvalidIdentifier(ident.to_string()));
    }
    for part in ident.split('.') {
        validate_identifier(part)?;
    }
    Ok(())
}

/// One CTE arm of a `WITH` clause (SQLRDF-25): a named sub-select with an
/// optional column list. `recursive` marks a member of a `WITH RECURSIVE`
/// list; the serializer promotes the whole `WITH` to `WITH RECURSIVE` when
/// any member is recursive (SQL allows the keyword once for the list).
#[derive(Debug, Clone, PartialEq)]
pub struct Cte {
    pub name: String,
    pub columns: Vec<String>,
    pub query: Box<SelectStatement>,
    pub recursive: bool,
}

/// One ORDER BY term: expression plus direction (`sql:direction`, ASC default).
#[derive(Debug, Clone, PartialEq)]
pub struct OrderTerm {
    pub expr: SqlExpr,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinType {
    Inner,
    LeftOuter,
    Asof,
    Positional,
    Lateral,
}

impl JoinType {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Inner => "JOIN",
            Self::LeftOuter => "LEFT JOIN",
            Self::Asof => "ASOF JOIN",
            Self::Positional => "POSITIONAL JOIN",
            Self::Lateral => "CROSS JOIN LATERAL",
        }
    }

    pub fn from_iri(iri: &str) -> Result<Self, AstError> {
        match iri {
            "http://graphlaw.org/sql#InnerJoin" => Ok(Self::Inner),
            "http://graphlaw.org/sql#LeftJoin" => Ok(Self::LeftOuter),
            "http://graphlaw.org/sql#AsofJoin" => Ok(Self::Asof),
            "http://graphlaw.org/sql#PositionalJoin" => Ok(Self::Positional),
            "http://graphlaw.org/sql#LateralJoin" => Ok(Self::Lateral),
            _ => Err(AstError::UnsupportedTerm(iri.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinClause {
    pub join_type: JoinType,
    pub table: TableRef,
    pub condition: SqlExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledQuery {
    pub sql: String,
    pub params: Vec<SqlLiteral>,
    pub ast_graph_turtle: Option<String>,
    pub canon_graph_bytes: Option<Vec<u8>>,
    pub ast_hash: Option<[u8; 32]>,
    pub cacheable: bool,
}

impl CompiledQuery {
    pub fn new(sql: String, params: Vec<SqlLiteral>) -> Self {
        Self {
            sql,
            params,
            ast_graph_turtle: None,
            canon_graph_bytes: None,
            ast_hash: None,
            cacheable: false,
        }
    }

    pub fn raw_compiled(sql: String, params: Vec<SqlLiteral>) -> Self {
        Self::new(sql, params)
    }

    pub fn with_graph(mut self, turtle: String, canon_bytes: Vec<u8>, hash: [u8; 32]) -> Self {
        self.ast_graph_turtle = Some(turtle);
        self.canon_graph_bytes = Some(canon_bytes);
        self.ast_hash = Some(hash);
        self
    }
}

/// A compiled statement-sequence document (SQLRDF-23, compile half): the
/// members in spine order plus the domain-separated document hash.
///
/// `doc_hash = BLAKE3(0x00 || h_1 || 0x00 || h_2 || ...)` where each `h_i` is
/// the member's `receipt_state` anchor — the 0x00 separators are the domain
/// separation from any bare-form hash, so a sequence-of-one hashes
/// differently from the bare single-root form of the same statement (the
/// envelope is part of the identity).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledDocument {
    /// Members in spine order (`unroll_rdf_list` walk order, cross-checked
    /// against the strictly-ascending `sql:statementIndex`).
    pub statements: Vec<CompiledQuery>,
    /// Domain-separated document hash; sequence form only.
    pub doc_hash: [u8; 32],
    /// SQLRDF-26: atomic mode. `false` (default) keeps the SQLRDF-23
    /// best-effort sequential-autocommit (pipeline) semantics. `true` —
    /// from `sql:atomic true` on the `sql:StatementSequence` root — wraps
    /// the whole document in BEGIN/COMMIT with ROLLBACK on member failure.
    pub atomic: bool,
}


// Number of RDFC-1.0 canonicalizations performed by this module. Instrument
// for the R1 receipt-cost reduction: canonicalization is amortized to once
// per compile, not once per receipt. Eval reads the carried
// `CompiledQuery::canon_graph_bytes` instead of re-canonicalizing.
thread_local! {
    pub static CANONICALIZATION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Computes `Canon(G_AST)` — the RDFC-1.0 canonical byte form of an admitted
/// AST graph — using the same `purrdf::try_canonicalize` law as
/// `duckdb_eval::canonical_ast_graph_bytes`. Called exactly once per
/// `compile_turtle`; failure is a typed REFUSAL-01 `AstError`, never a panic.
pub(super) fn canonical_graph_bytes(turtle: &str) -> Result<Vec<u8>, AstError> {
    let dataset = purrdf_rdf::parse_dataset(turtle.as_bytes(), "text/turtle", None)
        .map_err(|_| AstError::new(RefusalCode::MalformedAst, refusal_arm::CANONICALIZATION))?;
    let canon = purrdf_rdf::try_canonicalize(&dataset)
        .map_err(|_| AstError::new(RefusalCode::MalformedAst, refusal_arm::CANONICALIZATION))?;
    CANONICALIZATION_COUNT.set(CANONICALIZATION_COUNT.get() + 1);
    Ok(canon.nquads.into_bytes())
}

/// Section 7 analytical MERGE statement (rec 7/7.1): the target table, the
/// grouped USING sub-select, the ON condition, and the WHEN MATCHED update
/// assignments. Compiled by [`MergeStatement::compile_merge`] with the
/// sub-select literal (section 7.1: `$1`) extracted into the parameter
/// buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeStatement {
    pub target: TableRef,
    pub source: SelectStatement,
    pub on: SqlExpr,
    pub assignments: Vec<(String, SqlExpr)>,
}

impl MergeStatement {
    /// Compiles the section 7 graph to the section 7.1 parameterized MERGE.
    /// The sub-select literal is extracted into `params` ($1 = "SETTLED" for
    /// the section 7 graph), never string-interpolated.
    pub fn compile_merge(&self) -> Result<CompiledQuery, AstError> {
        validate_identifier(&self.target.name)?;
        if let Some(schema) = &self.target.schema {
            validate_qualified_identifier(schema)?;
        }
        if let Some(alias) = &self.target.alias {
            validate_identifier(alias)?;
        }

        let mut params = Vec::new();
        // Inner sub-select body (the leading "SELECT " of the clause compiler
        // output is stripped for embedding).
        let inner = self.source.compile()?;
        let body = inner.sql;
        params.extend(inner.params);

        let mut sql = String::with_capacity(body.len() + 160);
        sql.push_str("MERGE INTO ");
        if let Some(schema) = &self.target.schema {
            sql.push_str(schema);
            sql.push('.');
        }
        sql.push_str(&self.target.name);
        sql.push_str(" USING (");
        sql.push_str(&body);
        sql.push_str(") AS src ON ");
        let mut on_sql = String::new();
        SelectStatement::serialize_expr(
            &self.on,
            &mut on_sql,
            &mut params,
            0,
            ScopePos::Predicate,
        )?;
        sql.push_str(&on_sql);
        sql.push_str(" WHEN MATCHED THEN UPDATE SET ");
        for (idx, (column, value)) in self.assignments.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            validate_identifier(column)?;
            sql.push_str(column);
            sql.push_str(" = ");
            SelectStatement::serialize_expr(value, &mut sql, &mut params, 0, ScopePos::Predicate)?;
        }
        sql.push(';');
        // C1-a: merge compile is not the SelectStatement arm; never cacheable.
        Ok(CompiledQuery::raw_compiled(sql, params))
    }
}

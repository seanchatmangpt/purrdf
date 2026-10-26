//! Declarative SQL AST representation and deterministic compiler for GraphLaw.
//!
//! Provides in-memory AST terms matching the `ontologies/sql-ast.ttl` ontology,
//! identifier validation eliminating structural injection, parameterized SQL serialization,
//! and recursive bounded RDF collection unrolling.

#![allow(missing_docs)]
// Clippy `result_large_err` residual (coordinator decision, scoped to this
// file only): `AstError` intentionally embeds the ~208-byte `RefusalContext`
// by design — refusals are pure data (docs/refusals.md typed-refusal law).
// Boxing the context is an API change deferred to the switchover phase, not
// a lane-level edit.
#![allow(clippy::result_large_err)]

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::ast;
use crate::ast::*;
use crate::error::*;
use crate::serializer::*;
use purrdf_hash::blake3;

/// SQLRDF-25 Row 4: discriminator kinds for composite literal/accessor
/// classes (struct literal, list literal, index/slice access).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeKind {
    Struct,
    List,
    Index,
    Slice,
}

// =============================================================================
// Graph Unrolling Helper (purrdf / RDF list extraction)
// =============================================================================

pub trait RdfAstGraphReader {
    type Node: Clone + fmt::Display;

    fn get_single_object(&self, subject: &Self::Node, predicate: &str) -> Option<Self::Node>;
    fn get_datatype_literal(
        &self,
        subject: &Self::Node,
        predicate: &str,
    ) -> Option<(String, Option<String>)>;
    fn get_iri(&self, node: &Self::Node) -> Option<String>;

    /// Number of objects on `(node, predicate)`, across IRIs and literals.
    fn object_count(&self, node: &Self::Node, predicate: &str) -> usize;

    /// Counts only IRI-valued objects of `predicate` (dual-form
    /// discriminator refusals must not count string-literal objects).
    fn iri_object_count(&self, node: &Self::Node, predicate: &str) -> usize;

    /// Predicate IRIs asserted on `node` — backs the unknown-statement-
    /// predicate gate (D1/RUST-7: a rename must not be a silent-ignore
    /// escape hatch past the closed sql: predicate enum). Default: empty,
    /// so the gate degrades to a no-op for readers that cannot enumerate
    /// predicates (never a false refusal on a lawful document).
    fn predicate_iris(&self, _node: &Self::Node) -> Vec<String> {
        Vec::new()
    }

    /// SQLRDF-17: the admitted Turtle AST document this reader is backed
    /// by, when the reader is document-backed. Builtin eval admits only
    /// graph-backed queries compiled via `compile_turtle`, so a reader that
    /// cannot supply the document (default: `None`) is a typed REFUSAL-04
    /// at the builtin, never a hand-built-AST admission.
    fn query_document_turtle(&self) -> Option<String> {
        None
    }

    /// Cardinality-checked object read (audit E3/fuzz-4): more than one
    /// object on a single-cardinality clause predicate is REFUSAL-01; a
    /// duplicate must not win by document order.
    fn single_object_checked(
        &self,
        node: &Self::Node,
        predicate: &str,
    ) -> Result<Option<Self::Node>, AstError> {
        if self.object_count(node, predicate) > 1 {
            return Err(AstError::new(
                RefusalCode::MalformedAst,
                refusal_arm::DUP_CLAUSE_PREDICATE,
            )
            .with_ident(predicate)
            .with_aux_ident(node));
        }
        Ok(self.get_single_object(node, predicate))
    }

    /// Deterministically unwraps an rdf:List (rdf:first / rdf:rest chain) up to max_items.
    fn unroll_rdf_list(
        &self,
        head: &Self::Node,
        max_items: usize,
    ) -> Result<Vec<Self::Node>, AstError> {
        let rdf_first = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
        let rdf_rest = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
        let rdf_nil = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";

        let mut items = Vec::new();
        let mut curr = head.clone();
        let mut visited = HashSet::new();

        while let Some(iri) = self.get_iri(&curr) {
            if iri == rdf_nil {
                break;
            }

            if items.len() >= max_items {
                return Err(AstError::RecursionLimitExceeded(max_items));
            }

            if !visited.insert(iri.clone()) {
                return Err(AstError::new(RefusalCode::MalformedList, 0));
            }

            let first = self.get_single_object(&curr, rdf_first).ok_or_else(|| {
                AstError::MissingProperty(rdf_first.to_string(), curr.to_string())
            })?;
            items.push(first);

            curr = self
                .get_single_object(&curr, rdf_rest)
                .ok_or_else(|| AstError::MissingProperty(rdf_rest.to_string(), curr.to_string()))?;
        }

        Ok(items)
    }

    /// SQLRDF-25 Rows 1+2: extracts one projection item. Star vocabulary
    /// (`sql:Star` / `sql:exclude` / `sql:replace`) and the COLUMNS item
    /// (`sql:Columns` / `sql:columnsPattern`) are projection-list-only; the
    /// expression path refuses them typed (REFUSAL-01
    /// `STAR_MODIFIER_POSITION`) via the gate in [`Self::extract_expr_scoped`].
    fn extract_select_item(&self, node: &Self::Node, depth: usize) -> Result<SelectItem, AstError> {
        const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        const STAR: &str = "http://graphlaw.org/sql#Star";
        const COLUMNS: &str = "http://graphlaw.org/sql#Columns";
        let type_iri = || {
            self.get_single_object(node, RDF_TYPE)
                .and_then(|t| self.get_iri(&t))
        };
        let has_star_type = type_iri().as_deref() == Some(STAR);
        let has_columns_type = type_iri().as_deref() == Some(COLUMNS);
        let has_exclude = self
            .get_single_object(node, "http://graphlaw.org/sql#exclude")
            .is_some();
        let has_replace = self
            .get_single_object(node, "http://graphlaw.org/sql#replace")
            .is_some();
        let has_columns_pattern = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#columnsPattern")
            .is_some();

        if has_star_type || has_exclude || has_replace {
            let exclude = if let Some(head) =
                self.single_object_checked(node, "http://graphlaw.org/sql#exclude")?
            {
                self.unroll_rdf_list(&head, 64)?
                    .into_iter()
                    .map(|n| {
                        self.get_datatype_literal(&n, "http://graphlaw.org/sql#columnName")
                            .map(|(c, _)| c)
                            .ok_or_else(|| {
                                AstError::MissingProperty(
                                    "http://graphlaw.org/sql#columnName".to_string(),
                                    n.to_string(),
                                )
                            })
                    })
                    .collect::<Result<Vec<String>, AstError>>()?
            } else {
                Vec::new()
            };
            if has_exclude && exclude.is_empty() {
                // SQLRDF-25 malformed-member twin: `sql:exclude ()` (rdf:nil,
                // zero EXCLUDE members) is a malformed star modifier — never a
                // silent bare `*`. Carried on the existing star-vocabulary arm
                // (REFUSAL-01, STAR_MODIFIER_POSITION) pending the dedicated
                // EMPTY_EXCLUDE arm the refusals register lane assigns.
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::STAR_MODIFIER_POSITION,
                )
                .with_ident(node));
            }
            let mut replace = Vec::new();
            if let Some(head) =
                self.single_object_checked(node, "http://graphlaw.org/sql#replace")?
            {
                for member in self.unroll_rdf_list(&head, 64)? {
                    let target = self
                        .get_single_object(&member, "http://graphlaw.org/sql#expression")
                        .ok_or_else(|| {
                            AstError::MissingProperty(
                                "http://graphlaw.org/sql#expression".to_string(),
                                member.to_string(),
                            )
                        })?;
                    let expr = self.extract_expr_scoped(&target, depth + 1, ScopePos::SelectList)?;
                    let (name, _) = self
                        .get_datatype_literal(&member, "http://graphlaw.org/sql#aliasName")
                        .or_else(|| {
                            self.get_datatype_literal(&member, "http://graphlaw.org/sql#alias")
                        })
                        .ok_or_else(|| {
                            AstError::MissingProperty(
                                "http://graphlaw.org/sql#aliasName".to_string(),
                                member.to_string(),
                            )
                        })?;
                    replace.push((expr, name));
                }
            }
            return Ok(SelectItem::Star { exclude, replace });
        }

        if has_columns_type || has_columns_pattern {
            // Lambda form: sql:columnsLambda present, or a non-literal
            // sql:columnsPattern object — refused typed until specified.
            let lambda = self
                .get_single_object(node, "http://graphlaw.org/sql#columnsLambda")
                .is_some()
                || (self.object_count(node, "http://graphlaw.org/sql#columnsPattern") > 0
                    && !has_columns_pattern);
            if lambda {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::COLUMNS_LAMBDA,
                )
                .with_ident(node));
            }
            let pattern = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#columnsPattern")
                .map(|(p, _)| p)
                .ok_or_else(|| {
                    AstError::new(
                        RefusalCode::MalformedAst,
                        refusal_arm::COLUMNS_PATTERN,
                    )
                    .with_ident(node)
                })?;
            if pattern.trim().is_empty() {
                // SQLRDF-25 malformed-member twin: an empty (or whitespace)
                // `sql:columnsPattern` is no usable pattern literal — never a
                // silent `COLUMNS('')`. REFUSAL-01, COLUMNS_PATTERN arm.
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::COLUMNS_PATTERN,
                )
                .with_ident(node));
            }
            return Ok(SelectItem::Columns(pattern));
        }

        Ok(SelectItem::Expr(self.extract_expr_scoped(
            node,
            depth,
            ScopePos::SelectList,
        )?))
    }

    /// Recursively extracts a `SqlExpr` from an AST node in the graph.
    /// Legacy entry point: expression is assumed to sit in a permissive
    /// position (see [`Self::extract_expr_scoped`] for the scope-integrity
    /// aware path used by the clause compiler).
    fn extract_expr(&self, node: &Self::Node, depth: usize) -> Result<SqlExpr, AstError> {
        self.extract_expr_scoped(node, depth, ScopePos::SelectList)
    }

    /// Scope-integrity aware extraction (rec 1.3/3.3/5). `pos` is the
    /// syntactic position of `node`; aggregates are only legal in SELECT
    /// list, HAVING and ORDER BY; an explicit `sql:ProcessScope` column may
    /// not surface in a projection list; an explicit `sql:ViewScope` column
    /// may not filter inside a predicate. A property-stuffing node carrying
    /// more than one expression discriminator is REFUSAL-01 (never silently
    /// extract one term and let the aggregate vanish).
    fn extract_expr_scoped(
        &self,
        node: &Self::Node,
        depth: usize,
        pos: ScopePos,
    ) -> Result<SqlExpr, AstError> {
        if depth > 32 {
            return Err(AstError::RecursionLimitExceeded(32));
        }

        let sql_ns = "http://graphlaw.org/sql#";

        // sql:scope is single-cardinality and only meaningful on column
        // references; a scope annotation on any other expression node is
        // REFUSAL-01, never silently ignored (fuzz 1/4).
        let scope_objects = self.object_count(node, "http://graphlaw.org/sql#scope");
        if scope_objects > 1 {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::SCOPE_CARDINALITY)
                    .with_ident(node)
                    .with_num(scope_objects),
            );
        }
        let explicit_scope = self
            .get_single_object(node, "http://graphlaw.org/sql#scope")
            .and_then(|n| self.get_iri(&n))
            .or_else(|| {
                self.get_datatype_literal(node, "http://graphlaw.org/sql#scope")
                    .map(|(l, _)| l)
            });
        if explicit_scope.is_some() {
            // Scope is meaningful on column references and on output
            // expressions (the section 7 graph annotates the aliased
            // aggregate). It is meaningless on literals and binary/operator
            // nodes: carrying it there is REFUSAL-01, never silence.
            let is_column = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#columnName")
                .is_some();
            let is_output_expr = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#aliasName")
                .is_some()
                || self
                    .get_datatype_literal(node, "http://graphlaw.org/sql#alias")
                    .is_some()
                || self
                    .get_datatype_literal(node, "http://graphlaw.org/sql#functionName")
                    .is_some()
                || self
                    .get_single_object(node, "http://graphlaw.org/sql#function")
                    .is_some();
            if !is_column && !is_output_expr {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::SCOPE_WRONG_NODE,
                )
                .with_ident(node));
            }
        }

        // Ambiguity gate: at most one expression discriminator per node.
        let has_column = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#columnName")
            .is_some();
        let has_literal = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#literalValue")
            .or_else(|| self.get_datatype_literal(node, "http://graphlaw.org/sql#value"))
            .is_some();
        let has_alias_wrapper = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#aliasName")
            .is_some();
        let has_func_iri = self
            .get_single_object(node, "http://graphlaw.org/sql#function")
            .is_some();
        let has_func_name = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#functionName")
            .is_some();
        let has_operator = self
            .get_single_object(node, "http://graphlaw.org/sql#operator")
            .is_some()
            || self
                .get_datatype_literal(node, "http://graphlaw.org/sql#operator")
                .is_some();
        let has_window = self
            .get_single_object(node, "http://graphlaw.org/sql#windowFunction")
            .is_some();
        let has_between = self
            .get_single_object(node, "http://graphlaw.org/sql#between")
            .is_some()
            || self
                .get_single_object(node, "http://www.w3.org/1999/02/22-rdf-syntax-ns#type")
                .and_then(|n| self.get_iri(&n))
                .as_deref()
                == Some("http://graphlaw.org/sql#Between");
        let discriminators = [
            has_column,
            has_literal,
            has_alias_wrapper,
            has_func_iri,
            has_func_name,
            has_operator,
            has_window,
            has_between,
        ]
        .iter()
        .filter(|b| **b)
        .count();
        // SQLRDF-25 Row 4: composite literals/accessors are discriminated by
        // rdf:type; carrying a composite type alongside any other expression
        // discriminator is REFUSAL-01 (the ambiguity gate stays closed).
        let has_composite = self.composite_kind(node).is_some();
        let discriminators = discriminators + usize::from(has_composite);
        if discriminators > 1 {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::DISCRIMINATORS)
                    .with_ident(node)
                    .with_num(discriminators),
            );
        }

        // D3: `sql:expression` is the property of the alias wrapper
        // (sql:aliasName + sql:expression). Without sql:aliasName it is a
        // transparent expression wrapper (e.g. an sql:orderBy item annotating
        // an aggregate with sql:direction plus sql:expression). Carrying it
        // alongside any other discriminator is a laundering marker
        // (REFUSAL-01), never first-key-wins.
        let has_expr_wrapper = self
            .get_single_object(node, "http://graphlaw.org/sql#expression")
            .is_some();
        if has_expr_wrapper && !has_alias_wrapper && discriminators > 0 {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::EXPRESSION_WRAPPER)
                    .with_ident(node),
            );
        }

        // SQLRDF-25 Rows 1+2: star/COLUMNS projection vocabulary reached the
        // expression path — it is projection-list-only. The projection loop
        // routes through extract_select_item, so a star marker here is always
        // out of scope (WHERE / argument / wrapper laundering): REFUSAL-01.
        {
            let star_marker = self
                .get_single_object(node, "http://graphlaw.org/sql#exclude")
                .is_some()
                || self
                    .get_single_object(node, "http://graphlaw.org/sql#replace")
                    .is_some()
                || self
                    .get_datatype_literal(node, "http://graphlaw.org/sql#columnsPattern")
                    .is_some()
                || self
                    .get_single_object(node, "http://graphlaw.org/sql#columnsLambda")
                    .is_some()
                || {
                    let t = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
                    matches!(
                        self.get_single_object(node, t)
                            .and_then(|n| self.get_iri(&n))
                            .as_deref(),
                        Some("http://graphlaw.org/sql#Star")
                            | Some("http://graphlaw.org/sql#Columns")
                    )
                };
            if star_marker {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::STAR_MODIFIER_POSITION,
                )
                .with_ident(node));
            }
        }

        // Check if node is a ColumnRef
        if has_column {
            let (col_name, _) = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#columnName")
                .expect("checked above");
            let alias = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#tableAlias")
                .map(|(a, _)| a);
            // sql:qualifier (section 7) renders as a dotted prefix.
            let qualifier = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#qualifier")
                .map(|(q, _)| q)
                .or(alias);

            // Scope integrity on explicit scope annotations (section 1.3/5).
            if let Some(scope_iri) = explicit_scope {
                let scope = Scope::from_iri(&scope_iri)?;
                if scope == Scope::Process && pos.is_view_surface() {
                    return Err(AstError::new(
                        RefusalCode::ScopeIntegrity,
                        refusal_arm::PROCESS_SCOPE_IN_SELECT,
                    )
                    .with_ident(&col_name));
                }
                if scope == Scope::View
                    && matches!(
                        pos,
                        ScopePos::Predicate | ScopePos::GroupBy | ScopePos::Having
                    )
                {
                    return Err(AstError::new(
                        RefusalCode::ScopeIntegrity,
                        refusal_arm::VIEW_SCOPE_IN_PREDICATE,
                    )
                    .with_ident(&col_name));
                }
                // TC-MUT-01 arm B: an explicit scope annotation on an
                // aggregate ARGUMENT beneath a predicate-rooted aggregate is
                // a laundering marker — the argument position is process-only
                // by construction, so the marker can only re-scope the
                // aggregate's output past the gate. The aggregate's own
                // legality (e.g. HAVING) must not inherit. Under an
                // output-position aggregate the section 7 example stays
                // lawful (AggregateArgument, no refusal).
                if pos == ScopePos::PredicateAggregateArgument {
                    return Err(AstError::new(
                        RefusalCode::ScopeIntegrity,
                        refusal_arm::SCOPE_IN_PRED_AGG_ARG,
                    )
                    .with_ident(&col_name)
                    .with_aux_ident(&scope_iri));
                }
            }

            let expr = SqlExpr::ColumnRef {
                column: col_name,
                table_alias: qualifier,
            };
            return Self::finish_alias(self, node, expr, sql_ns, pos);
        }

        // Check if node is a Literal
        if has_literal {
            let (val_str, dtype) = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#literalValue")
                .or_else(|| self.get_datatype_literal(node, "http://graphlaw.org/sql#value"))
                .expect("checked above");
            let lit = match dtype.as_deref() {
                // Typed refusals (LITERAL_NOT_INT / LITERAL_NOT_FLOAT): a
                // typed literal that fails to parse refuses; it is never
                // silently stringified or coerced to 0/0.0.
                Some("http://www.w3.org/2001/XMLSchema#integer") => {
                    SqlLiteral::Integer(val_str.parse().map_err(|_| {
                        AstError::new(RefusalCode::LiteralNotInt, 0)
                            .with_ident(&val_str)
                            .with_aux_ident(node)
                    })?)
                }
                Some("http://www.w3.org/2001/XMLSchema#double") => {
                    SqlLiteral::Float(val_str.parse().map_err(|_| {
                        AstError::new(RefusalCode::LiteralNotFloat, 0)
                            .with_ident(&val_str)
                            .with_aux_ident(node)
                    })?)
                }
                Some("http://www.w3.org/2001/XMLSchema#boolean") => {
                    SqlLiteral::Boolean(val_str == "true" || val_str == "1")
                }
                _ => SqlLiteral::String(val_str),
            };
            return Self::finish_alias(self, node, SqlExpr::Literal(lit), sql_ns, pos);
        }

        // Check if node is an Alias wrapper
        if has_alias_wrapper {
            if matches!(
                pos,
                ScopePos::AggregateArgument | ScopePos::PredicateAggregateArgument
            ) {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::ALIAS_IN_AGG_ARG,
                )
                .with_ident(node));
            }
            let (alias_name, _) = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#aliasName")
                .expect("checked above");
            let target_expr_node = self
                .get_single_object(node, "http://graphlaw.org/sql#expression")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#expression".to_string(),
                        node.to_string(),
                    )
                })?;
            let inner_expr = self.extract_expr_scoped(&target_expr_node, depth + 1, pos)?;
            return Ok(SqlExpr::Alias {
                expr: Box::new(inner_expr),
                alias: alias_name,
            });
        }

        // Check if node is a FunctionCall (axiom IRI form: sql:function + sql:argument)
        if has_func_iri {
            let func_node = self
                .get_single_object(node, "http://graphlaw.org/sql#function")
                .expect("checked above");
            let func_iri = self.get_iri(&func_node).ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#function".to_string(),
                    node.to_string(),
                )
            })?;
            let function = AggregateFunction::from_iri(&func_iri)?;
            let arg_node = self
                .get_single_object(node, "http://graphlaw.org/sql#argument")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#argument".to_string(),
                        node.to_string(),
                    )
                })?;
            let argument =
                self.extract_expr_scoped(&arg_node, depth + 1, aggregate_argument_pos(pos))?;
            let expr = Self::check_aggregate_position(
                SqlExpr::FunctionCall {
                    function,
                    argument: Box::new(argument),
                },
                pos,
            )?;
            return Self::finish_alias(self, node, expr, sql_ns, pos);
        }

        // FunctionCall (section 7 literal form: sql:functionName + sql:arguments list)
        if has_func_name {
            let (fname, _) = self
                .get_datatype_literal(node, "http://graphlaw.org/sql#functionName")
                .expect("checked above");
            let function = aggregate_from_name(&fname)?;
            // SQLRDF-25: ROW_NUMBER is zero-argument — sql:arguments is
            // optional (an empty list is admitted); the argument slot carries
            // the structural Null sentinel and never serializes.
            if function == AggregateFunction::RowNumber {
                let expr = Self::check_aggregate_position(
                    SqlExpr::FunctionCall {
                        function,
                        argument: Box::new(SqlExpr::Literal(SqlLiteral::Null)),
                    },
                    pos,
                )?;
                return Self::finish_alias(self, node, expr, sql_ns, pos);
            }
            let args_head = self
                .get_single_object(node, "http://graphlaw.org/sql#arguments")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#arguments".to_string(),
                        node.to_string(),
                    )
                })?;
            let args = self.unroll_rdf_list(&args_head, 8)?;
            let arg_node = args.first().ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#arguments".to_string(),
                    node.to_string(),
                )
            })?;
            let argument =
                self.extract_expr_scoped(arg_node, depth + 1, aggregate_argument_pos(pos))?;
            let expr = Self::check_aggregate_position(
                SqlExpr::FunctionCall {
                    function,
                    argument: Box::new(argument),
                },
                pos,
            )?;
            return Self::finish_alias(self, node, expr, sql_ns, pos);
        }

        // Check if node is a Binary or Unary Expression
        if has_operator {
            let op = self.read_operator(node)?.ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#operator".to_string(),
                    node.to_string(),
                )
            })?;

            // Unary operator handling (e.g. BitwiseNot)
            if op == Operator::BitwiseNot {
                let operand_node = self
                    .get_single_object(node, "http://graphlaw.org/sql#operand")
                    .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#expression"))
                    .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#rightOperand"))
                    .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#right"))
                    .ok_or_else(|| {
                        AstError::MissingProperty(
                            "http://graphlaw.org/sql#operand".to_string(),
                            node.to_string(),
                        )
                    })?;
                let expr = self.extract_expr_scoped(&operand_node, depth + 1, pos)?;
                return Ok(SqlExpr::Unary {
                    op,
                    expr: Box::new(expr),
                });
            }

            let left_legacy =
                self.single_object_checked(node, "http://graphlaw.org/sql#leftOperand")?;
            let left_norm = self.single_object_checked(node, "http://graphlaw.org/sql#left")?;
            if left_legacy.is_some() && left_norm.is_some() {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::DUAL_LEFT_OPERAND,
                )
                .with_ident(node));
            }
            let right_legacy =
                self.single_object_checked(node, "http://graphlaw.org/sql#rightOperand")?;
            let right_norm = self.single_object_checked(node, "http://graphlaw.org/sql#right")?;
            if right_legacy.is_some() && right_norm.is_some() {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::DUAL_RIGHT_OPERAND,
                )
                .with_ident(node));
            }
            let left_node = self
                .get_single_object(node, "http://graphlaw.org/sql#leftOperand")
                .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#left"))
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#leftOperand".to_string(),
                        node.to_string(),
                    )
                })?;
            let right_node = self
                .get_single_object(node, "http://graphlaw.org/sql#rightOperand")
                .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#right"))
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#rightOperand".to_string(),
                        node.to_string(),
                    )
                })?;

            let left = self.extract_expr_scoped(&left_node, depth + 1, pos)?;
            let right = self.extract_expr_scoped(&right_node, depth + 1, pos)?;

            return Ok(SqlExpr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
        }

        // Check if node is a Between expression (Ternary)
        if has_between {
            let expr_node = self
                .get_single_object(node, "http://graphlaw.org/sql#expression")
                .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#target"))
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#expression".to_string(),
                        node.to_string(),
                    )
                })?;
            let low_node = self
                .get_single_object(node, "http://graphlaw.org/sql#lowerBound")
                .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#low"))
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#lowerBound".to_string(),
                        node.to_string(),
                    )
                })?;
            let high_node = self
                .get_single_object(node, "http://graphlaw.org/sql#upperBound")
                .or_else(|| self.get_single_object(node, "http://graphlaw.org/sql#high"))
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#upperBound".to_string(),
                        node.to_string(),
                    )
                })?;

            let expr = self.extract_expr_scoped(&expr_node, depth + 1, pos)?;
            let low = self.extract_expr_scoped(&low_node, depth + 1, pos)?;
            let high = self.extract_expr_scoped(&high_node, depth + 1, pos)?;

            return Ok(SqlExpr::Between {
                expr: Box::new(expr),
                low: Box::new(low),
                high: Box::new(high),
            });
        }

        // Check if node is a WindowFn (SQLRDF-25: sql:windowFunction ->
        // aggregate/scalar expression node, optional sql:partitionBy /
        // sql:orderBy rdf:Lists, optional sql:frame node).
        if has_window {
            let func_node = self
                .get_single_object(node, "http://graphlaw.org/sql#windowFunction")
                .expect("checked above");
            // Window functions are legal only in SELECT list, ORDER BY and
            // QUALIFY — mirror of check_aggregate_position (REFUSAL-01,
            // arm WINDOW_FN_POSITION).
            if !pos.allows_window() {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::WINDOW_FN_POSITION,
                )
                .with_ident(node));
            }
            let function = self.extract_expr_scoped(&func_node, depth + 1, pos)?;

            let mut partition_by = Vec::new();
            if let Some(part_head) =
                self.single_object_checked(node, "http://graphlaw.org/sql#partitionBy")?
            {
                for part in self.unroll_rdf_list(&part_head, 64)? {
                    partition_by.push(self.extract_expr_scoped(&part, depth + 1, ScopePos::GroupBy)?);
                }
            }

            let mut order_by = Vec::new();
            if let Some(order_head) =
                self.single_object_checked(node, "http://graphlaw.org/sql#orderBy")?
            {
                for item in self.unroll_rdf_list(&order_head, 64)? {
                    let expr = self.extract_expr_scoped(&item, depth + 1, ScopePos::OrderBy)?;
                    let descending = self.read_direction(&item, node)?;
                    order_by.push(OrderTerm { expr, descending });
                }
            }

            let frame = if let Some(frame_node) =
                self.single_object_checked(node, "http://graphlaw.org/sql#frame")?
            {
                let (unit_lex, _) = self
                    .get_datatype_literal(&frame_node, "http://graphlaw.org/sql#frameUnit")
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::FRAME_UNIT)
                            .with_ident(frame_node.to_string())
                    })?;
                let unit = match unit_lex.to_ascii_uppercase().as_str() {
                    "ROWS" => FrameUnit::Rows,
                    "RANGE" => FrameUnit::Range,
                    _ => {
                        return Err(AstError::new(
                            RefusalCode::MalformedAst,
                            refusal_arm::FRAME_UNIT,
                        )
                        .with_ident(unit_lex))
                    }
                };
                let read_bound = |prop: &str| -> Result<Option<FrameBound>, AstError> {
                    match self.get_datatype_literal(
                        &frame_node,
                        &format!("http://graphlaw.org/sql#{prop}"),
                    ) {
                        Some((tok, _)) => FrameBound::parse_token(&tok).map(Some),
                        None => Ok(None),
                    }
                };
                let start = read_bound("frameStart")?.ok_or_else(|| {
                    AstError::new(RefusalCode::MalformedAst, refusal_arm::FRAME_BOUND)
                        .with_ident(frame_node.to_string())
                })?;
                let end = read_bound("frameEnd")?;
                Some(FrameSpec { unit, start, end })
            } else {
                None
            };

            return Self::finish_alias(
                self,
                node,
                SqlExpr::WindowFn {
                    function: Box::new(function),
                    partition_by,
                    order_by,
                    frame,
                },
                sql_ns,
                pos,
            );
        }

        // SQLRDF-25 Row 4: composite literals and accessors. The node type is
        // the discriminator; members link through the same expression
        // properties, so nesting validates member-by-member (rec section 4;
        // depth/key law enforced by SqlExpr::validate_composite at serialize
        // time and by extract_composite during extraction).
        if let Some(kind) = self.composite_kind(node) {
            let expr = self.extract_composite(node, kind, 0, depth, pos)?;
            return Self::finish_alias(self, node, expr, sql_ns, pos);
        }

        // D3: a bare sql:expression wrapper (no sql:aliasName, no other
        // discriminator — laundering guarded above) is transparent: extract
        // the inner expression in the SAME syntactic position, so an
        // sql:orderBy item carrying sql:expression -> aggregate extracts the
        // aggregate under ScopePos::OrderBy (which allows aggregates, rec 5)
        // instead of dying as an untyped "Unexpected terminal node".
        if let Some(inner) = self.get_single_object(node, "http://graphlaw.org/sql#expression") {
            return self.extract_expr_scoped(&inner, depth + 1, pos);
        }

        Err(AstError::MalformedNode(node.to_string()))
    }

    /// SQLRDF-25 Row 4 discriminator: the rdf:type of `node` names a
    /// composite literal/accessor class.
    // CompositeKind resolves to the module-level enum (line ~35,
    // GWS-LANE-UNBLOCK); an earlier in-impl duplicate here was illegal
    // (`enum is not supported in impls`) and was removed by the empty-star
    // lane under the compile-freeze SLA.
    fn composite_kind(&self, node: &Self::Node) -> Option<CompositeKind> {
        let t = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        match self
            .get_single_object(node, t)
            .and_then(|n| self.get_iri(&n))
            .as_deref()
        {
            Some("http://graphlaw.org/sql#StructLiteral") => Some(CompositeKind::Struct),
            Some("http://graphlaw.org/sql#ListLiteral") => Some(CompositeKind::List),
            Some("http://graphlaw.org/sql#IndexAccess") => Some(CompositeKind::Index),
            Some("http://graphlaw.org/sql#SliceAccess") => Some(CompositeKind::Slice),
            _ => None,
        }
    }

    /// Extracts a composite literal/accessor subtree. `comp_depth` counts
    /// composite nesting ONLY (so a 65-deep list refuses COMPOSITE_DEPTH, not
    /// the shallower generic expression-recursion cap); `depth` is the
    /// ordinary expression-recursion budget handed to non-composite leaf
    /// extraction.
    fn extract_composite(
        &self,
        node: &Self::Node,
        kind: CompositeKind,
        comp_depth: usize,
        depth: usize,
        pos: ScopePos,
    ) -> Result<SqlExpr, AstError> {
        if comp_depth > ast::MAX_COMPOSITE_DEPTH {
            return Err(AstError::new(
                RefusalCode::MalformedAst,
                refusal_arm::COMPOSITE_DEPTH,
            )
            .with_num(comp_depth));
        }
        // A member is either itself composite (recurse on the composite
        // depth counter) or a plain expression (ordinary extraction, one
        // recursion-budget tick).
        let member = |n: &Self::Node| -> Result<SqlExpr, AstError> {
            match self.composite_kind(n) {
                Some(k) => self.extract_composite(n, k, comp_depth + 1, depth, pos),
                None => self.extract_expr_scoped(n, depth + 1, pos),
            }
        };
        match kind {
            CompositeKind::Struct => {
                let head = self
                    .single_object_checked(node, "http://graphlaw.org/sql#structFields")?
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::COMPOSITE_MEMBER)
                            .with_ident(node)
                    })?;
                let field_nodes = self.unroll_rdf_list(&head, 64)?;
                let mut fields = Vec::with_capacity(field_nodes.len());
                for field in &field_nodes {
                    let (key, _) = self
                        .get_datatype_literal(field, "http://graphlaw.org/sql#fieldKey")
                        .ok_or_else(|| {
                            AstError::new(
                                RefusalCode::MalformedAst,
                                refusal_arm::COMPOSITE_MEMBER,
                            )
                            .with_ident(field)
                        })?;
                    let value_node = self
                        .single_object_checked(field, "http://graphlaw.org/sql#fieldValue")?
                        .ok_or_else(|| {
                            AstError::new(
                                RefusalCode::MalformedAst,
                                refusal_arm::COMPOSITE_MEMBER,
                            )
                            .with_ident(field)
                    })?;
                    let value = member(&value_node)?;
                    fields.push((key, value));
                }
                Ok(SqlExpr::StructLiteral { fields })
            }
            CompositeKind::List => {
                let head = self
                    .single_object_checked(node, "http://graphlaw.org/sql#listItems")?
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::COMPOSITE_MEMBER)
                            .with_ident(node)
                    })?;
                let item_nodes = self.unroll_rdf_list(&head, 64)?;
                let mut items = Vec::with_capacity(item_nodes.len());
                for item in &item_nodes {
                    items.push(member(item)?);
                }
                Ok(SqlExpr::ListLiteral { items })
            }
            CompositeKind::Index => {
                let base_node = self
                    .single_object_checked(node, "http://graphlaw.org/sql#indexBase")?
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::COMPOSITE_MEMBER)
                            .with_ident(node)
                    })?;
                let index_node = self
                    .single_object_checked(node, "http://graphlaw.org/sql#indexIndex")?
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::COMPOSITE_MEMBER)
                            .with_ident(node)
                    })?;
                let base = member(&base_node)?;
                let index = member(&index_node)?;
                Ok(SqlExpr::IndexAccess {
                    base: Box::new(base),
                    index: Box::new(index),
                })
            }
            CompositeKind::Slice => {
                let base_node = self
                    .single_object_checked(node, "http://graphlaw.org/sql#sliceBase")?
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::COMPOSITE_MEMBER)
                            .with_ident(node)
                    })?;
                let start_node = self
                    .single_object_checked(node, "http://graphlaw.org/sql#sliceStart")?
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::COMPOSITE_MEMBER)
                            .with_ident(node)
                    })?;
                let end_node = self.single_object_checked(node, "http://graphlaw.org/sql#sliceEnd")?;
                let base = member(&base_node)?;
                let start = member(&start_node)?;
                let end = match end_node {
                    Some(n) => Some(Box::new(member(&n)?)),
                    None => None,
                };
                Ok(SqlExpr::SliceAccess {
                    base: Box::new(base),
                    start: Box::new(start),
                    end,
                })
            }
        }
    }

    /// REFUSAL-03 gate: an aggregate is legal only in SELECT list, HAVING and
    /// ORDER BY; nested aggregates (COUNT(SUM(x))) refuse everywhere.
    fn check_aggregate_position(expr: SqlExpr, pos: ScopePos) -> Result<SqlExpr, AstError> {
        if !pos.allows_aggregate() {
            return Err(AstError::new(
                RefusalCode::ScopeIntegrity,
                refusal_arm::AGG_IN_PREDICATE,
            ));
        }
        Ok(expr)
    }

    /// Wraps an extracted expression in an output alias when the node carries
    /// `sql:alias` (section 7 attribute form; not an expression discriminator).
    fn finish_alias(
        this: &Self,
        node: &Self::Node,
        expr: SqlExpr,
        sql_ns: &str,
        pos: ScopePos,
    ) -> Result<SqlExpr, AstError> {
        if let Some((alias, _)) = this.get_datatype_literal(node, &format!("{sql_ns}alias")) {
            if matches!(
                pos,
                ScopePos::AggregateArgument | ScopePos::PredicateAggregateArgument
            ) {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::SQL_ALIAS_IN_AGG_ARG,
                )
                .with_ident(node));
            }

            return Ok(SqlExpr::Alias {
                expr: Box::new(expr),
                alias,
            });
        }
        Ok(expr)
    }

    /// Reads `sql:operator` as either a section 4 string token (xsd:string
    /// literal) or a legacy axiom IRI. Disallowed tokens are REFUSAL-01.
    fn read_operator(&self, node: &Self::Node) -> Result<Option<Operator>, AstError> {
        // D5: dual-carry (string token AND IRI operator) is REFUSAL-01 — the
        // string used to win silently and the IRI term laundered past review.
        let has_string_operator = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#operator")
            .is_some();
        // MINIMAL UNBLOCK (compile-freeze SLA, lane R1, disclosed): count
        // only IRI objects — `object_count` includes string-literal
        // operators, which made every literal-operator expression a false
        // dual-carry refusal. Repaired (repair lane): order-independent via
        // a dedicated `iri_object_count` trait method — `get_single_object`
        // only inspects the first term, so the string-first ordering slipped
        // past the dual-carry gate.
        let iri_operator_objects = self.iri_object_count(node, "http://graphlaw.org/sql#operator");
        if has_string_operator && iri_operator_objects > 0 {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::DUAL_OPERATOR_FORMS)
                    .with_ident(node),
            );
        }
        if let Some((token, _)) =
            self.get_datatype_literal(node, "http://graphlaw.org/sql#operator")
        {
            return Operator::from_token(&token).map(Some);
        }
        if let Some(op_node) = self.get_single_object(node, "http://graphlaw.org/sql#operator") {
            let iri = self.get_iri(&op_node).ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#operator".to_string(),
                    node.to_string(),
                )
            })?;
            return Operator::from_iri(&iri).map(Some);
        }
        Ok(None)
    }

    /// Reads an `sql:direction` token on an order item ("ASC" | "DESC",
    /// ASC default; present-but-IRI or malformed is REFUSAL-01, never a
    /// silent fold). Shared by the statement ORDER BY clause and window
    /// OVER ORDER BY lists (SQLRDF-25).
    fn read_direction(&self, item: &Self::Node, parent: &Self::Node) -> Result<bool, AstError> {
        if self.object_count(item, "http://graphlaw.org/sql#direction") > 0 {
            match self.get_datatype_literal(item, "http://graphlaw.org/sql#direction") {
                Some((d, _)) => {
                    if d.eq_ignore_ascii_case("DESC") {
                        Ok(true)
                    } else if d.eq_ignore_ascii_case("ASC") {
                        Ok(false)
                    } else {
                        Err(AstError::new(RefusalCode::MalformedAst, refusal_arm::DIRECTION_LEX)
                            .with_ident(d)
                            .with_aux_ident(parent))
                    }
                }
                None => Err(
                    AstError::new(RefusalCode::MalformedAst, refusal_arm::DIRECTION_IRI)
                        .with_ident(parent.to_string()),
                ),
            }
        } else {
            Ok(false)
        }
    }

    /// Reads a data source: either the legacy `sql:fromTable` alias (single
    /// TableRef node) or the normative `sql:from` rdf:List of table
    /// identifiers with optional `sql:schemaName` (rec 3.3).
    fn extract_table_ref(&self, node: &Self::Node) -> Result<TableRef, AstError> {
        // D1 (fuzz law, rec 1.3/3.3): sql:scope is defined on column and
        // output-expression nodes only. A scope marker on a table-reference
        // node (sql:fromTable / sql:joinTable target) is a term out of
        // place — REFUSAL-01, never silently ignored.
        let scope_marker = self.object_count(node, "http://graphlaw.org/sql#scope") > 0
            || self
                .get_datatype_literal(node, "http://graphlaw.org/sql#scope")
                .is_some();
        if scope_marker {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::SCOPE_ON_TABLE_REF)
                    .with_ident(node),
            );
        }
        let (name, _) = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#tableName")
            .ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#tableName".to_string(),
                    node.to_string(),
                )
            })?;
        let alias = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#tableAlias")
            .map(|(a, _)| a);
        let schema = self
            .get_datatype_literal(node, "http://graphlaw.org/sql#schemaName")
            .map(|(s, _)| s);
        Ok(TableRef {
            name,
            alias,
            schema,
        })
    }

    /// Reads the FROM clause: `sql:from` rdf:List first, `sql:fromTable`
    /// alias second. Neither present is REFUSAL-01.
    fn extract_from(&self, query_node: &Self::Node) -> Result<TableRef, AstError> {
        // Mixed-source documents are REFUSAL-01: the validated sql:fromTable
        // must not be a decoy for an unreviewed sql:from list (audit E2).
        let from_list = self.single_object_checked(query_node, "http://graphlaw.org/sql#from")?;
        let from_table =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#fromTable")?;
        if from_list.is_some() && from_table.is_some() {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::DUAL_FROM)
                    .with_ident(query_node),
            );
        }
        if let Some(from_head) = from_list {
            let items = self.unroll_rdf_list(&from_head, 64)?;
            let first = items.first().ok_or_else(|| {
                AstError::new(RefusalCode::MalformedAst, refusal_arm::EMPTY_FROM)
                    .with_ident(query_node)
            })?;
            return self.extract_table_ref(first);
        }
        if let Some(table_ref_node) = from_table {
            return self.extract_table_ref(&table_ref_node);
        }
        Err(AstError::new(RefusalCode::MalformedAst, refusal_arm::NO_FROM).with_ident(query_node))
    }

    /// Extracts a full `SelectStatement` AST from a target query node.
    /// Normative property names (rec 3.3: sql:select/from/where/having/
    /// orderBy/groupBy) are read first; the SQLRDF-01 aliases (selectList/
    /// fromTable/whereClause/havingClause) stay legal.
    fn extract_select_statement(
        &self,
        query_node: &Self::Node,
    ) -> Result<SelectStatement, AstError> {
        // D1/RUST-7 unknown-predicate gate: every sql:-namespaced predicate on
        // the statement node must be in the closed enum the reader knows how
        // to review. A renamed clause (sql:whereX) must refuse typed
        // REFUSAL-01, never be dropped on the floor and admit the statement
        // without the filter the author wrote.
        const SQL_NS: &str = "http://graphlaw.org/sql#";
        const KNOWN_SQL_PREDICATES: &[&str] = &[
            "statementIndex",
            "selectList",
            "select",
            "from",
            "fromTable",
            "joinClause",
            "whereClause",
            "where",
            "having",
            "havingClause",
            "groupBy",
            "orderBy",
            "limit",
            "offset",
            "withClause",
            "qualify",
        ];
        for pred in self.predicate_iris(query_node) {
            if let Some(local) = pred.strip_prefix(SQL_NS)
                && !KNOWN_SQL_PREDICATES.contains(&local)
            {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::UNKNOWN_STATEMENT_PREDICATE,
                )
                .with_ident(query_node)
                .with_aux_ident(&pred));
            }
        }
        let from = self.extract_from(query_node)?;

        // Joins (optional)
        let mut joins = Vec::new();
        if let Some(join_node) =
            self.get_single_object(query_node, "http://graphlaw.org/sql#joinClause")
        {
            let join_type = if let Some(type_node) =
                self.get_single_object(&join_node, "http://graphlaw.org/sql#joinType")
            {
                let type_iri = self.get_iri(&type_node).unwrap_or_default();
                // Unknown join-type IRI: typed REFUSAL-01 refusal (arm
                // JOIN_TYPE_IRI) — never a silent downgrade to Inner. A
                // MISSING sql:joinType predicate stays Inner (default), and
                // both behaviors are pinned by courts.
                JoinType::from_iri(&type_iri).map_err(|_| {
                    AstError::new(RefusalCode::MalformedAst, refusal_arm::JOIN_TYPE_IRI)
                        .with_ident(&type_iri)
                })?
            } else {
                JoinType::Inner
            };

            let join_table_node = self
                .get_single_object(&join_node, "http://graphlaw.org/sql#joinTable")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#joinTable".to_string(),
                        join_node.to_string(),
                    )
                })?;
            let join_table = self.extract_table_ref(&join_table_node)?;

            let join_cond_node = self
                .get_single_object(&join_node, "http://graphlaw.org/sql#joinCondition")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#joinCondition".to_string(),
                        join_node.to_string(),
                    )
                })?;

            let join_condition =
                self.extract_expr_scoped(&join_cond_node, 0, ScopePos::Predicate)?;

            joins.push(JoinClause {
                join_type,
                table: join_table,
                condition: join_condition,
            });
        }

        // Unroll projection list (rdf:List): sql:selectList or sql:select.
        // Carrying both is REFUSAL-01: the reviewer must see the list the
        // engine executes (audit E1).
        let select_legacy =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#selectList")?;
        let select_norm =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#select")?;
        if select_legacy.is_some() && select_norm.is_some() {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::DUAL_SELECT)
                    .with_ident(query_node),
            );
        }
        let select_head = select_legacy.or(select_norm);
        let select_head = select_head.ok_or_else(|| {
            AstError::new(RefusalCode::MalformedAst, refusal_arm::NO_PROJECTION)
                .with_ident(query_node)
        })?;
        let select_nodes = self.unroll_rdf_list(&select_head, 64)?;
        let mut select_list = Vec::with_capacity(select_nodes.len());
        for node in select_nodes {
            select_list.push(self.extract_select_item(&node, 0)?);
        }
        // D1: an empty projection list (rdf:nil head) is malformed, never
        // SELECT *: the ontology says projection is never zero-column
        // (both sql:selectList and sql:select paths funnel through here).
        if select_list.is_empty() {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::EMPTY_PROJECTION)
                    .with_ident(query_node),
            );
        }

        // Where clause (optional): sql:whereClause or sql:where; both is REFUSAL-01.
        let where_legacy =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#whereClause")?;
        let where_norm = self.single_object_checked(query_node, "http://graphlaw.org/sql#where")?;
        if where_legacy.is_some() && where_norm.is_some() {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::DUAL_WHERE)
                    .with_ident(query_node),
            );
        }
        let where_clause = if let Some(where_node) = where_legacy.or(where_norm) {
            Some(self.extract_expr_scoped(&where_node, 0, ScopePos::Predicate)?)
        } else {
            None
        };

        // Group by (optional rdf:List)
        let group_head =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#groupBy")?;
        let group_by = if let Some(group_head) = group_head {
            let group_nodes = self.unroll_rdf_list(&group_head, 64)?;
            let mut list = Vec::with_capacity(group_nodes.len());
            for node in group_nodes {
                list.push(self.extract_expr_scoped(&node, 0, ScopePos::GroupBy)?);
            }
            list
        } else {
            Vec::new()
        };

        // Having (optional): sql:having or sql:havingClause; both is REFUSAL-01.
        let having_norm =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#having")?;
        let having_alias =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#havingClause")?;
        if having_norm.is_some() && having_alias.is_some() {
            return Err(
                AstError::new(RefusalCode::MalformedAst, refusal_arm::DUAL_HAVING)
                    .with_ident(query_node),
            );
        }
        let having = if let Some(having_node) = having_norm.or(having_alias) {
            Some(self.extract_expr_scoped(&having_node, 0, ScopePos::Having)?)
        } else {
            None
        };

        // Order by (optional rdf:List): sql:orderBy; items carry optional
        // sql:direction ("ASC" | "DESC", ASC default).
        let order_head =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#orderBy")?;
        let order_by = if let Some(order_head) = order_head {
            let order_nodes = self.unroll_rdf_list(&order_head, 64)?;
            let mut list = Vec::with_capacity(order_nodes.len());
            for node in order_nodes {
                let expr = self.extract_expr_scoped(&node, 0, ScopePos::OrderBy)?;
                // A malformed direction is REFUSAL-01, never a silent fold to
                // ASC (same law as limit/offset below): only ASC/DESC admitted.
                // D4-adjacent: a present-but-IRI sql:direction is REFUSAL-01,
                // never a silent fold to ASC (distinguish present-but-IRI
                // from absent via object_count).
                let descending = if self.object_count(&node, "http://graphlaw.org/sql#direction")
                    > 0
                {
                    match self.get_datatype_literal(&node, "http://graphlaw.org/sql#direction") {
                        Some((d, _)) => {
                            if d.eq_ignore_ascii_case("DESC") {
                                true
                            } else if d.eq_ignore_ascii_case("ASC") {
                                false
                            } else {
                                return Err(AstError::new(
                                    RefusalCode::MalformedAst,
                                    refusal_arm::DIRECTION_LEX,
                                )
                                .with_ident(&d)
                                .with_aux_ident(query_node));
                            }
                        }
                        None => {
                            return Err(AstError::new(
                                RefusalCode::MalformedAst,
                                refusal_arm::DIRECTION_IRI,
                            )
                            .with_ident(query_node));
                        }
                    }
                } else {
                    false
                };
                list.push(OrderTerm { expr, descending });
            }
            list
        } else {
            Vec::new()
        };

        // Limit & offset: a malformed integer is REFUSAL-01, never a silent
        // clause drop (audit E4: unbounded scan from a SHACL-clean doc).
        let parse_clause_int = |lex: String, prop: &str| -> Result<usize, AstError> {
            let arm = if prop == "limit" {
                refusal_arm::LIMIT_MALFORMED
            } else {
                refusal_arm::OFFSET_MALFORMED
            };
            lex.parse::<usize>().map_err(|_| {
                AstError::new(RefusalCode::MalformedAst, arm)
                    .with_ident(&lex)
                    .with_aux_ident(query_node)
            })
        };
        // E4 (lane E1-court defect fix, disclosed): sql:limit/sql:offset are
        // literal-valued, and `get_single_object` only surfaces IRI objects —
        // a literal clause previously read as ABSENT, silently dropping LIMIT
        // and never refusing malformed integers. Drive both clauses off
        // `get_datatype_literal` + `object_count` instead.
        let limit_clause = |prop: &str| -> Result<Option<usize>, AstError> {
            let count = self.object_count(query_node, &format!("http://graphlaw.org/sql#{prop}"));
            if count > 1 {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::DUP_CLAUSE_PREDICATE,
                )
                .with_ident(prop)
                .with_aux_ident(query_node));
            }
            match self.get_datatype_literal(query_node, &format!("http://graphlaw.org/sql#{prop}"))
            {
                Some((lex, _)) => parse_clause_int(lex, prop).map(Some),
                None if count > 0 => {
                    let arm = if prop == "limit" {
                        refusal_arm::LIMIT_NOT_INT
                    } else {
                        refusal_arm::OFFSET_NOT_INT
                    };
                    Err(AstError::new(RefusalCode::MalformedAst, arm).with_ident(query_node))
                }
                None => Ok(None),
            }
        };
        let limit = limit_clause("limit")?;
        let offset = limit_clause("offset")?;

        // SQLRDF-25: WITH clause. sql:withClause is an rdf:List of CTE nodes;
        // each carries sql:cteName (required literal), optional
        // sql:cteColumns (rdf:List of name literals), sql:cteQuery (required
        // nested SelectStatement root — absent is REFUSAL-01, the rendered
        // SQL would be `name AS ()`), and optional sql:recursive true.
        let mut ctes = Vec::new();
        if let Some(with_head) =
            self.single_object_checked(query_node, "http://graphlaw.org/sql#withClause")?
        {
            for cte_node in self.unroll_rdf_list(&with_head, 64)? {
                let name = self
                    .get_datatype_literal(&cte_node, "http://graphlaw.org/sql#cteName")
                    .map(|(n, _)| n)
                    .ok_or_else(|| {
                        AstError::MissingProperty(
                            "http://graphlaw.org/sql#cteName".to_string(),
                            cte_node.to_string(),
                        )
                    })?;
                let columns = if let Some(col_head) =
                    self.single_object_checked(&cte_node, "http://graphlaw.org/sql#cteColumns")?
                {
                    self.unroll_rdf_list(&col_head, 64)?
                        .into_iter()
                        .map(|n| {
                            self.get_datatype_literal(&n, "http://graphlaw.org/sql#columnName")
                                .map(|(c, _)| c)
                                .ok_or_else(|| {
                                    AstError::MissingProperty(
                                        "http://graphlaw.org/sql#columnName".to_string(),
                                        n.to_string(),
                                    )
                                })
                        })
                        .collect::<Result<Vec<String>, AstError>>()?
                } else {
                    Vec::new()
                };
                let query_node = self
                    .get_single_object(&cte_node, "http://graphlaw.org/sql#cteQuery")
                    .ok_or_else(|| {
                        AstError::new(RefusalCode::MalformedAst, refusal_arm::CTE_NO_QUERY)
                            .with_ident(&cte_node)
                    })?;
                let query = Box::new(self.extract_select_statement(&query_node)?);
                let recursive = self
                    .get_datatype_literal(&cte_node, "http://graphlaw.org/sql#recursive")
                    .map(|(r, _)| r == "true")
                    .unwrap_or(false);
                ctes.push(Cte {
                    name,
                    columns,
                    query,
                    recursive,
                });
            }
        }

        // QUALIFY (SQLRDF-25, optional): window-filtering predicate, legal
        // position law for window functions and aggregates. A duplicate
        // sql:qualify is REFUSAL-01 (single-cardinality clause predicate).
        let qualify = match self.single_object_checked(query_node, "http://graphlaw.org/sql#qualify")?
        {
            Some(q_node) => Some(self.extract_expr_scoped(&q_node, 0, ScopePos::Qualify)?),
            None => None,
        };

        Ok(SelectStatement {
            ctes,
            qualify,
            from,
            joins,
            select_list,
            where_clause,
            group_by,
            having,
            order_by,
            limit,
            offset,
        })
    }

    /// SQLRDF-27 dialect gap #1: extracts a `sql:SetOperationStatement` —
    /// two full nested `sql:SelectStatement` operands, one required
    /// `sql:setOperator` token (`UNION` | `INTERSECT` | `EXCEPT`), and the
    /// optional `sql:setAll` / `sql:byName` xsd:boolean flags.
    ///
    /// Refusal law (typed, never a silent default or a dropped clause):
    /// - unknown `sql:` predicate on the statement node → REFUSAL-01
    ///   `UNKNOWN_STATEMENT_PREDICATE` (same closed-enum gate as
    ///   [`Self::extract_select_statement`]);
    /// - missing `sql:setLeft` / `sql:setOperator` → internal
    ///   `MissingProperty` (MERGE-clause precedent);
    /// - missing `sql:setRight` → REFUSAL-01 arm `SET_OP_NO_RIGHT` (the
    ///   dedicated appended arm — the rendered SQL would end at the
    ///   combinator with an empty right operand);
    /// - duplicate clause predicates → `DUP_CLAUSE_PREDICATE`;
    /// - unknown operator token → arm `SET_OP_KIND_TOKEN`;
    /// - non-boolean flag lexical → arm `SET_OP_FLAG_LEX`;
    /// - `sql:byName true` beside INTERSECT/EXCEPT → arm
    ///   `SET_OP_BY_NAME_KIND` (engine-witnessed: `INTERSECT BY NAME` is a
    ///   real frozen-duckdb parser error).
    fn extract_set_operation_statement(
        &self,
        query_node: &Self::Node,
    ) -> Result<SetOperationStatement, AstError> {
        const SQL_NS: &str = "http://graphlaw.org/sql#";
        const KNOWN_SQL_PREDICATES: &[&str] = &[
            "statementIndex",
            "setLeft",
            "setOperator",
            "setAll",
            "byName",
            "setRight",
        ];
        for pred in self.predicate_iris(query_node) {
            if let Some(local) = pred.strip_prefix(SQL_NS)
                && !KNOWN_SQL_PREDICATES.contains(&local)
            {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::UNKNOWN_STATEMENT_PREDICATE,
                )
                .with_ident(query_node)
                .with_aux_ident(&pred));
            }
        }

        let left_node = self
            .single_object_checked(query_node, "http://graphlaw.org/sql#setLeft")?
            .ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#setLeft".to_string(),
                    query_node.to_string(),
                )
            })?;
        let left = self.extract_select_statement(&left_node)?;

        let right_node = self
            .single_object_checked(query_node, "http://graphlaw.org/sql#setRight")?
            .ok_or_else(|| {
                AstError::new(RefusalCode::MalformedAst, refusal_arm::SET_OP_NO_RIGHT)
                    .with_ident(query_node)
            })?;
        let right = self.extract_select_statement(&right_node)?;

        // The combinator token: a string literal is the lawful form; an IRI
        // object falls through to `from_token` and refuses as an unknown
        // token (never a silent default to UNION). Single-cardinality like
        // every clause predicate (audit E3).
        if self.object_count(query_node, "http://graphlaw.org/sql#setOperator") > 1 {
            return Err(AstError::new(
                RefusalCode::MalformedAst,
                refusal_arm::DUP_CLAUSE_PREDICATE,
            )
            .with_ident("http://graphlaw.org/sql#setOperator")
            .with_aux_ident(query_node));
        }
        let op_token = match self
            .get_datatype_literal(query_node, "http://graphlaw.org/sql#setOperator")
        {
            Some((tok, _)) => tok,
            None => self
                .get_single_object(query_node, "http://graphlaw.org/sql#setOperator")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#setOperator".to_string(),
                        query_node.to_string(),
                    )
                })?
                .to_string(),
        };
        let kind = SetOpKind::from_token(&op_token)?;

        // Flags: E4-style literal reads (count-checked; a present-but-Iri
        // object is the malformed-lexical arm, never an absent flag).
        let mut flag_lex = |pred: &str| -> Result<bool, AstError> {
            let full = format!("{SQL_NS}{pred}");
            if self.object_count(query_node, &full) > 1 {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::DUP_CLAUSE_PREDICATE,
                )
                .with_ident(&full)
                .with_aux_ident(query_node));
            }
            match self.get_datatype_literal(query_node, &full) {
                Some((v, _)) if v == "true" => Ok(true),
                Some((v, _)) if v == "false" => Ok(false),
                Some((v, _)) => Err(
                    AstError::new(RefusalCode::MalformedAst, refusal_arm::SET_OP_FLAG_LEX)
                        .with_ident(pred)
                        .with_aux_ident(&v),
                ),
                None if self.object_count(query_node, &full) > 0 => Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::SET_OP_FLAG_LEX,
                )
                .with_ident(pred)
                .with_aux_ident(query_node)),
                None => Ok(false),
            }
        };
        let all = flag_lex("setAll")?;
        let by_name = flag_lex("byName")?;

        if by_name && kind != SetOpKind::Union {
            return Err(AstError::new(
                RefusalCode::MalformedAst,
                refusal_arm::SET_OP_BY_NAME_KIND,
            )
            .with_ident(query_node));
        }

        Ok(SetOperationStatement {
            left,
            kind,
            all,
            by_name,
            right,
        })
    }

    /// Extracts a section 7 `MergeIntoStatement`: target table identifier,
    /// grouped USING sub-select, merge ON condition, and WHEN MATCHED update
    /// assignments. Any other shape falls back to the legacy scalar template.
    fn extract_merge_statement(&self, node: &Self::Node) -> Result<MergeStatement, AstError> {
        let target_node = self
            .single_object_checked(node, "http://graphlaw.org/sql#targetTable")?
            .ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#targetTable".to_string(),
                    node.to_string(),
                )
            })?;
        let target = self.extract_table_ref(&target_node)?;

        let using_node = self
            .single_object_checked(node, "http://graphlaw.org/sql#usingSource")?
            .ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#usingSource".to_string(),
                    node.to_string(),
                )
            })?;
        let source = self.extract_select_statement(&using_node)?;

        let on_node = self
            .single_object_checked(node, "http://graphlaw.org/sql#mergeOn")?
            .ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#mergeOn".to_string(),
                    node.to_string(),
                )
            })?;
        let on = self.extract_expr_scoped(&on_node, 0, ScopePos::Predicate)?;
        // Scope-integrity (REFUSAL-03, lane K5b): the section 7 ON condition
        // is the join key between target and source. An ON expression that
        // references no target-qualified column (`$1 = $2`, constant-only)
        // matches every target row, so WHEN MATCHED fan-out is unbounded by
        // construction. Refuse typed before any SQL is emitted.
        if !expr_references_target(&on, &target) {
            return Err(AstError::new(
                RefusalCode::ScopeIntegrity,
                refusal_arm::MERGE_ON_NO_TARGET_COLUMN,
            )
            .with_ident(on_node.to_string()));
        }

        let when_node = self
            .single_object_checked(node, "http://graphlaw.org/sql#whenMatched")?
            .ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#whenMatched".to_string(),
                    node.to_string(),
                )
            })?;
        let assigns_head = self
            .get_single_object(&when_node, "http://graphlaw.org/sql#setAssignments")
            .ok_or_else(|| {
                AstError::MissingProperty(
                    "http://graphlaw.org/sql#setAssignments".to_string(),
                    when_node.to_string(),
                )
            })?;
        let mut assignments = Vec::new();
        for assign in self.unroll_rdf_list(&assigns_head, 64)? {
            let (column, _) = self
                .get_datatype_literal(&assign, "http://graphlaw.org/sql#targetColumn")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#targetColumn".to_string(),
                        assign.to_string(),
                    )
                })?;
            let value_node = self
                .get_single_object(&assign, "http://graphlaw.org/sql#valueExpression")
                .ok_or_else(|| {
                    AstError::MissingProperty(
                        "http://graphlaw.org/sql#valueExpression".to_string(),
                        assign.to_string(),
                    )
                })?;
            let value = self.extract_expr_scoped(&value_node, 0, ScopePos::Predicate)?;
            assignments.push((column, value));
        }

        Ok(MergeStatement {
            target,
            source,
            on,
            assignments,
        })
    }
}

/// True exactly when `expr` contains at least one column reference qualified
/// by the MERGE target (lane K5b scope-integrity check; lane U1 hardening).
/// A qualifier matches only when it equals the target's alias, the bare table
/// name, or the target's full rendered name `{schema}.{name}` when a schema is
/// declared. A dotted qualifier whose schema prefix is not the target's
/// declared schema (`main_v2.sales` vs target `sales`) names a DIFFERENT table
/// and never matches — the former `ends_with(".{name}")` suffix rule
/// false-positived on that collision (court e1). An unqualified column
/// reference is statically unresolvable against the join (a bare name could
/// bind to either side) and never matches: the ON guard refuses what it
/// cannot prove, so the source-only fan-out shape `ON flag = flag2` stays
/// typed-refused ([REFUSAL-03]) alongside the constant-only ON.
fn expr_references_target(expr: &SqlExpr, target: &TableRef) -> bool {
    // The target's full rendered name, exactly as the serializer emits it
    // (sql_ast/serializer.rs renders `schema.name`).
    let fully_qualified = target
        .schema
        .as_ref()
        .map(|s| format!("{s}.{}", target.name));
    let qualified_for_target = |alias: &Option<String>| match alias {
        None => false,
        Some(q) => {
            q == &target.name
                || target.alias.as_ref().is_some_and(|a| q == a)
                || fully_qualified.as_ref().is_some_and(|f| q == f)
        }
    };
    match expr {
        SqlExpr::ColumnRef {
            column: _,
            table_alias,
        } => qualified_for_target(table_alias),
        SqlExpr::Literal(_) => false,
        SqlExpr::Binary { left, right, .. } => {
            expr_references_target(left, target) || expr_references_target(right, target)
        }
        SqlExpr::FunctionCall { argument, .. } => expr_references_target(argument, target),
        // SQLRDF-25 Row 4 minimal unblock (lane sq1, compile-freeze SLA,
        // disclosed): composites reference the target exactly when their
        // children do.
        SqlExpr::StructLiteral { fields } => fields
            .iter()
            .any(|(_, v)| expr_references_target(v, target)),
        SqlExpr::ListLiteral { items } => {
            items.iter().any(|i| expr_references_target(i, target))
        }
        SqlExpr::IndexAccess { base, index } => {
            expr_references_target(base, target) || expr_references_target(index, target)
        }
        SqlExpr::SliceAccess { base, start, end } => {
            expr_references_target(base, target)
                || expr_references_target(start, target)
                || end
                    .as_ref()
                    .is_some_and(|e| expr_references_target(e.as_ref(), target))
        }
        SqlExpr::WindowFn {
            function,
            partition_by,
            order_by,
            frame,
        } => {
            expr_references_target(function, target)
                || partition_by.iter().any(|e| expr_references_target(e, target))
                || order_by.iter().any(|t| expr_references_target(&t.expr, target))
                || frame
                    .as_ref()
                    .is_some_and(|f| frame_references_target(f, target))
        }
        SqlExpr::Alias { expr, .. } => expr_references_target(expr, target),
        SqlExpr::Unary { expr, .. } => expr_references_target(expr, target),
        SqlExpr::Between { expr, low, high } => {
            expr_references_target(expr, target)
                || expr_references_target(low, target)
                || expr_references_target(high, target)
        }
    }
}

/// Frame bounds carry only offsets (integers), never column references —
/// the walk only descends through the frame struct for future-proofing.
fn frame_references_target(_frame: &FrameSpec, _target: &TableRef) -> bool {
    false
}

/// Validates an RDF AST dataset in Turtle syntax against the canonical SHACL rules in `ontologies/sql-ast.ttl`.
pub fn validate_ast_shacl(turtle_dataset: &str, shapes_ttl: &str) -> Result<(), AstError> {
    let ds = purrdf_rdf::parse_dataset(turtle_dataset.as_bytes(), "text/turtle", None)
        .map_err(|_| AstError::new(RefusalCode::MalformedAst, refusal_arm::PARSE_DATASET))?;

    let shapes = purrdf_shapes::engine::parse_shapes(shapes_ttl, None)
        .map_err(|_| AstError::new(RefusalCode::MalformedAst, refusal_arm::PARSE_SHAPES))?;

    let report = purrdf_shapes::engine::validate_dataset(&ds, &shapes)
        .map_err(|_| AstError::new(RefusalCode::MalformedAst, refusal_arm::SHACL_ENGINE))?;

    if !report.conforms {
        let first = report.results.first().ok_or_else(|| {
            AstError::new(RefusalCode::MalformedAst, refusal_arm::SHACL_VIOLATIONS)
        })?;
        let message = first
            .messages
            .first()
            .map(|l| l.value().to_string())
            .unwrap_or_else(|| "violation".into());
        let typed_arm = message
            .split("[REFUSAL-")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .and_then(|tag| tag.split_whitespace().nth(1))
            .and_then(refusal_arm::by_marker);
        let arm = typed_arm.unwrap_or(refusal_arm::SHACL_VIOLATIONS);
        return Err(
            AstError::new(RefusalCode::MalformedAst, arm)
                .with_ident(&first.focus_node)
                .with_aux_ident(message)
                .with_num(report.results.len()),
        );
    }

    Ok(())
}

/// SHACL-check `turtle` against the SQL AST shapes and compile its statement
/// into SQL. `SelectStatement` uses the expression compiler. Other admitted
/// statement classes compile from `sql:targetTable` and assignment literals.
/// `ConnectStatement` and `DisconnectStatement` refuse here, before any engine call.
/// Charter-A04 seam: process-wide `h_ast → Arc<CompiledQuery>` memoization.
/// A repeated identical admitted graph (same `h_ast = BLAKE3(Canon(G_AST))`)
/// must never re-run `serialize_expr` (or SHACL validation / RDFC-1.0
/// canonicalization for the re-render): the memo returns the already-admitted
/// compiled form. Arc-fed so the stored compiled query is never re-built —
/// callers clone out of the Arc. Capped (flush-on-overflow at the same 256
/// capacity class as the prepared-statement cache) and keyed by the digest,
/// so a flipped literal in the graph forks to a distinct entry by
/// construction.
static AST_COMPILE_MEMO: std::sync::OnceLock<
    std::sync::Mutex<HashMap<[u8; 32], std::sync::Arc<CompiledQuery>>>,
> = std::sync::OnceLock::new();

const AST_COMPILE_MEMO_CAPACITY: usize = 256;

fn ast_compile_memo() -> &'static std::sync::Mutex<HashMap<[u8; 32], std::sync::Arc<CompiledQuery>>>
{
    AST_COMPILE_MEMO.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Sole admission path for SQL-in-RDF: validates a Turtle description of a
/// SQL statement against the SHACL shapes in `ontologies/sql-ast.ttl` plus
/// the dialect pack (`ggen/sql-dialect/statement_classes.ttl`), parses the
/// statement graph, and compiles it to parameterized DuckDB SQL
/// (`$1, $2, ...` placeholders) pinned to the graph that produced it.
///
/// Returns the [`CompiledQuery`] carrying:
///
/// - `sql` + `params`: the parameterized statement text and its typed
///   parameter vector.
/// - `ast_graph_turtle` / `canon_graph_bytes`: the admitted graph and its
///   RDFC-1.0 canonical form (`Canon(G_AST)`), computed once here so the
///   eval/receipt path never re-parses.
/// - `ast_hash`: content-addressed BLAKE3 digest of the canonical AST graph.
///
/// # Refusals
/// Typed and stable-coded (`docs/refusals.md`), never a panic or generic DB
/// error:
///
/// - `[REFUSAL-01] MalformedAstRefusal` — shape violation: missing or
///   duplicated `sql:statementIndex`, non-integer or negative index,
///   invalid identifier characters.
/// - `[REFUSAL-02] UnimplementedStatementRefusal` — the statement class is
///   valid SHACL but outside the supported set (e.g. an unsupported
///   statement kind).
/// - `[REFUSAL-03] ScopeIntegrityRefusal` — a clause scope references the
///   wrong node or wrong cardinality.
///
/// # Examples
/// ```
/// use purrdf_sql::compile_turtle;
///
/// let doc = concat!(
///     "@prefix sql: <http://graphlaw.org/sql#> .\n",
///     "@prefix ex: <http://example.org/queries#> .\n",
///     "ex:S a sql:SelectStatement ;\n",
///     "    sql:statementIndex 1 ;\n",
///     "    sql:fromTable [ sql:tableName \"t\" ] ;\n",
///     "    sql:selectList ( [ sql:columnName \"c\" ] ) .\n",
/// );
/// let compiled = compile_turtle(doc)?;
/// assert!(compiled.sql.starts_with("SELECT"));
/// # Ok::<(), purrdf_sql::AstError>(())
/// ```
pub fn compile_turtle(turtle: &str) -> Result<CompiledQuery, AstError> {
    let shapes = format!(
        "{}\n{}",
        include_str!("../ontologies/sql-ast.ttl"),
        include_str!("../ontologies/statement_classes.ttl")
    );
    validate_ast_shacl(turtle, &shapes)?;
    let reader = DatasetReader::from_turtle(turtle)?;
    let (node, kind) = reader.statement_kind()?;
    require_statement_index(&reader, &node)?;
    // Fuzz D2: nested statement nodes are statements too — the §7 usingSource
    // sub-select carries its own index per §7's example graph.
    if let Some(using) = reader.get_single_object(&node, "http://graphlaw.org/sql#usingSource") {
        require_statement_index(&reader, &using)?;
    }
    // R1: canonicalize ONCE at compile time (not per receipt). Failure is a
    // typed REFUSAL-01 AstError, never a crash.
    let canon_bytes = canonical_graph_bytes(turtle)?;
    let h_ast = *blake3::hash(&canon_bytes).as_bytes();
    if kind.as_str() == "SelectStatement"
        && let Some(arc) = ast_compile_memo()
            .lock()
            .ok()
            .and_then(|m| m.get(&h_ast).cloned())
    {
        let mut cq = (*arc).clone();
        cq.ast_graph_turtle = Some(turtle.to_string());
        return Ok(cq);
    }
    let compiled = compile_statement_arm(&reader, &node, kind.as_str());
    let compiled = compiled.map(|mut cq| {
        cq.ast_hash = Some(h_ast);
        cq.ast_graph_turtle = Some(turtle.to_string());
        cq.canon_graph_bytes = Some(canon_bytes);
        cq.cacheable = kind.as_str() == "SelectStatement";
        cq
    })?;
    // Charter-A04 memoization store: only cacheable-class (SelectStatement)
    // compiles enter the memo. Flush-on-overflow at the pinned capacity so
    // the memo never grows unbounded.
    if kind.as_str() == "SelectStatement"
        && let Ok(mut memo) = ast_compile_memo().lock()
    {
        if memo.len() >= AST_COMPILE_MEMO_CAPACITY && !memo.contains_key(&h_ast) {
            memo.clear();
        }
        memo.insert(h_ast, std::sync::Arc::new(compiled.clone()));
    }
    Ok(compiled)
}

/// Section 4 (StatementBaseShape): every statement MUST carry exactly one
/// integer sql:statementIndex. Missing, duplicated, wrongly-typed, or
/// negative is REFUSAL-01 and refuses before any engine call (fuzz D1).
/// Extracted verbatim from `compile_turtle`'s former closure (SQLRDF-23) so
/// the document path enforces the identical check per member.
fn require_statement_index(reader: &DatasetReader, n: &String) -> Result<(), AstError> {
    match reader.object_count(n, "http://graphlaw.org/sql#statementIndex") {
        0 => Err(AstError::new(RefusalCode::MalformedAst, refusal_arm::INDEX_MISSING).with_ident(n)),
        c if c > 1 => Err(
            AstError::new(RefusalCode::MalformedAst, refusal_arm::INDEX_DUP)
                .with_ident(n)
                .with_num(c),
        ),
        _ => {
            let (index_lex, datatype) = reader
                .get_datatype_literal(n, "http://graphlaw.org/sql#statementIndex")
                .ok_or_else(|| {
                    AstError::new(RefusalCode::MalformedAst, refusal_arm::INDEX_NOT_INT)
                        .with_ident(n)
                })?;
            if datatype.as_deref() != Some("http://www.w3.org/2001/XMLSchema#integer") {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::INDEX_BAD_DTYPE,
                )
                .with_ident(n));
            }
            match index_lex.parse::<i64>() {
                Ok(v) if v >= 0 => Ok(()),
                _ => Err(
                    AstError::new(RefusalCode::MalformedAst, refusal_arm::INDEX_NEGATIVE)
                        .with_ident(&index_lex)
                        .with_aux_ident(n),
                ),
            }
        }
    }
}

/// Compiles one top-level statement node by its statement class. Extracted
/// verbatim from `compile_turtle`'s former inline `match` (SQLRDF-23) so the
/// document path's members take the full member ladder: REFUSAL-02
/// (Connect/Disconnect) and REFUSAL-05 (unbounded mutation) fire per member,
/// unchanged.
fn compile_statement_arm(
    reader: &DatasetReader,
    node: &String,
    kind: &str,
) -> Result<CompiledQuery, AstError> {
    match kind {
        "SelectStatement" => reader.extract_select_statement(node)?.compile(),
        // REFUSAL-02 (UnimplementedStatementRefusal): registered carrier is
        // the typed AstError::UnsupportedTerm carrying the statement-class
        // IRI, with the "compile refusal" marker the (c) court pins.
        "ConnectStatement" | "DisconnectStatement" => Err(AstError::new(
            RefusalCode::UnimplementedStatement,
            refusal_arm::UNIMPLEMENTED_STATEMENT,
        )
        .with_ident(kind)),
        "MergeIntoStatement"
            if reader
                .get_single_object(node, "http://graphlaw.org/sql#usingSource")
                .is_some() =>
        {
            reader.extract_merge_statement(node)?.compile_merge()
        }
        // SQLRDF-25 Row 3: PIVOT / UNPIVOT statement classes (shapes:
        // ontologies/sql-ast.ttl `sql:PivotStatementShape` /
        // `sql:UnpivotStatementShape`; rec section 4 statement-class ladder).
        "PivotStatement" => compile_pivot_statement(reader, node),
        "UnpivotStatement" => compile_unpivot_statement(reader, node),
        // SQLRDF-27 dialect gap #1: UNION [ALL] [BY NAME] / INTERSECT [ALL] /
        // EXCEPT [ALL] over two full nested SelectStatement operands. The
        // cacheable-class memo and `cacheable` bit keep excluding composite
        // arms (merge precedent): only the SelectStatement arm enters either.
        "SetOperationStatement" => reader.extract_set_operation_statement(node)?.compile_set_operation(),
        other => reader.compile_command(node, other),
    }
}

/// SQLRDF-25 Row 3 dispatch helpers (PIVOT / UNPIVOT compile arms). They live
/// beside [`compile_statement_arm`] so the dispatch stays the single review
/// surface; SHACL (`sql:PivotStatementShape` / `sql:UnpivotStatementShape`)
/// has already constrained cardinality and identifier shape — the arms below
/// extract and render, refusing typed REFUSAL-01 on the residual compiler-law
/// cases the shapes cannot express (empty IN-list, non-enum aggregate).
fn pivot_string_members(reader: &DatasetReader, node: &str, pred: &str) -> Vec<String> {
    reader
        .outgoing
        .get(node)
        .and_then(|preds| preds.get(pred))
        .map(|terms| {
            terms
                .iter()
                .filter_map(|t| match t {
                    RdfTerm::Literal { lexical, .. } => Some(lexical.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Typed REFUSAL-01 (MalformedAstRefusal) for the pivot arms, rendered via the
/// LEGACY_MESSAGE template (free-form tail) — codes are append-only and no
/// existing arm renumbers.
fn pivot_refusal(msg: String) -> AstError {
    AstError::new(RefusalCode::MalformedAst, refusal_arm::LEGACY_MESSAGE).with_ident(msg)
}

fn compile_pivot_statement(reader: &DatasetReader, node: &str) -> Result<CompiledQuery, AstError> {
    const NS: &str = "http://graphlaw.org/sql#";
    const AGGREGATES: [&str; 5] = ["sum", "count", "avg", "min", "max"];
    // Exactly one well-formed source table (shape: minCount/maxCount 1 +
    // ident pattern; ident() re-validates and refuses MissingProperty typed).
    let (table, _) = reader
        .get_datatype_literal(&node.to_string(), &format!("{NS}pivotTable"))
        .ok_or_else(|| AstError::MissingProperty(format!("{NS}pivotTable"), node.to_string()))?;
    let table = validate_identifier(&table)?;
    // ON column set: at least one (shape minCount 1), each a strict ident.
    let on_cols = pivot_string_members(reader, node, &format!("{NS}pivotColumn"));
    if on_cols.is_empty() {
        return Err(pivot_refusal(format!(
            "{REFUSAL_01_MALFORMED_AST} {node} carries no sql:pivotColumn literal; a PIVOT needs at least one ON column (never an implicit empty ON)"
        )));
    }
    let on_cols = on_cols
        .iter()
        .map(|c| validate_identifier(c).map(str::to_string))
        .collect::<Result<Vec<_>, _>>()?;
    // USING aggregate: exactly one token from the approved enum
    // (sum|count|avg|min|max); anything else refuses typed REFUSAL-01.
    let aggs = pivot_string_members(reader, node, &format!("{NS}pivotAggregate"));
    let agg = match aggs.as_slice() {
        [one] if AGGREGATES.contains(&one.as_str()) => one.clone(),
        [one, ..] => {
            return Err(pivot_refusal(format!(
                "{REFUSAL_01_MALFORMED_AST} sql:pivotAggregate '{one}' on {node} is not an approved aggregate (sum|count|avg|min|max)"
            )))
        }
        [] => {
            return Err(pivot_refusal(format!(
                "{REFUSAL_01_MALFORMED_AST} statement {node} is missing required sql:pivotAggregate (exactly one of sum|count|avg|min|max)"
            )))
        }
    };
    // Aggregate argument column (sql:pivotAggregateColumn, e.g. "amount").
    let (agg_col, _) = reader
        .get_datatype_literal(&node.to_string(), &format!("{NS}pivotAggregateColumn"))
        .ok_or_else(|| {
            AstError::MissingProperty(format!("{NS}pivotAggregateColumn"), node.to_string())
        })?;
    let agg_col = validate_identifier(&agg_col)?;
    // IN-list: at least one member (shape minCount 1). An empty IN-list
    // (`sql:pivotInValue ( )` — the rdf:nil IRI, zero literals) refuses typed
    // REFUSAL-01 here: the shape's minCount is satisfied by the rdf:nil triple,
    // so the compiler is the enforcement point, never a zero-column PIVOT.
    let in_vals = pivot_string_members(reader, node, &format!("{NS}pivotInValue"));
    if in_vals.is_empty() {
        return Err(pivot_refusal(format!(
            "{REFUSAL_01_MALFORMED_AST} statement {node} carries an empty sql:pivotInValue IN-list (rdf:nil, zero literals); a PIVOT renders at least one IN member"
        )));
    }
    let in_list = in_vals
        .iter()
        .map(|v| format!("'{}'", v.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let on_list = on_cols.join(", ");
    Ok(CompiledQuery::raw_compiled(
        format!("PIVOT {table} ON {on_list} IN ({in_list}) USING {agg}({agg_col})"),
        Vec::new(),
    ))
}

fn compile_unpivot_statement(
    reader: &DatasetReader,
    node: &str,
) -> Result<CompiledQuery, AstError> {
    const NS: &str = "http://graphlaw.org/sql#";
    let (table, _) = reader
        .get_datatype_literal(&node.to_string(), &format!("{NS}pivotTable"))
        .ok_or_else(|| AstError::MissingProperty(format!("{NS}pivotTable"), node.to_string()))?;
    let table = validate_identifier(&table)?;
    let on_cols = pivot_string_members(reader, node, &format!("{NS}unpivotColumn"));
    if on_cols.is_empty() {
        return Err(pivot_refusal(format!(
            "{REFUSAL_01_MALFORMED_AST} statement {node} carries no sql:unpivotColumn literal; an UNPIVOT needs at least one ON column"
        )));
    }
    let on_cols = on_cols
        .iter()
        .map(|c| validate_identifier(c).map(str::to_string))
        .collect::<Result<Vec<_>, _>>()?;
    // INTO NAME / VALUE: exactly one of each (shape minCount/maxCount 1).
    let (name_col, _) = reader
        .get_datatype_literal(&node.to_string(), &format!("{NS}unpivotNameColumn"))
        .ok_or_else(|| {
            AstError::MissingProperty(format!("{NS}unpivotNameColumn"), node.to_string())
        })?;
    let name_col = validate_identifier(&name_col)?;
    let (value_col, _) = reader
        .get_datatype_literal(&node.to_string(), &format!("{NS}unpivotValueColumn"))
        .ok_or_else(|| {
            AstError::MissingProperty(format!("{NS}unpivotValueColumn"), node.to_string())
        })?;
    let value_col = validate_identifier(&value_col)?;
    let on_list = on_cols.join(", ");
    Ok(CompiledQuery::raw_compiled(
        format!(
            "UNPIVOT {table} ON {on_list} INTO NAME {name_col} VALUE {value_col}"
        ),
        Vec::new(),
    ))
}

/// SQLRDF-23: compiles a Turtle AST document that may carry either a bare
/// single-root statement (delegates to [`compile_turtle`] semantics, with
/// `doc_hash` = the bare statement's own anchor so bare identity is
/// unchanged bit-for-bit) or a `sql:StatementSequence` root whose
/// `sql:statements` rdf:List members compile in spine order.
///
/// # Refusals (new REFUSAL-01 arms, append-only)
/// - `MIXED_DOCUMENT_ROOTS` — more than one top-level root, or both a bare
///   statement root and a sequence root (closes the silent lexicographic
///   pick).
/// - `SEQUENCE_EMPTY` — the sequence's `sql:statements` list has no members.
/// - `INDEX_SEQUENCE_DISORDER` — the `sql:statementIndex` cross-check over
///   the spine fails (non-integer, duplicate, gapped, or non-ascending).
/// Member-level refusals (REFUSAL-02/05) fire unchanged with the member's
/// 0-based position stamped into `ctx.predicate_idx`.
pub fn compile_turtle_document(turtle: &str) -> Result<CompiledDocument, AstError> {
    let shapes = format!(
        "{}\n{}",
        include_str!("../ontologies/sql-ast.ttl"),
        include_str!("../ontologies/statement_classes.ttl")
    );
    validate_ast_shacl(turtle, &shapes)?;
    let reader = DatasetReader::from_turtle(turtle)?;
    let (bare, seq) = reader.document_roots()?;
    // Domain separation: canonicalize ONCE over the whole document, exactly
    // as compile_turtle does for the bare form.
    let canon_bytes = canonical_graph_bytes(turtle)?;
    let h_doc = *blake3::hash(&canon_bytes).as_bytes();
    let stamp = |mut cq: CompiledQuery| -> CompiledQuery {
        cq.ast_hash = Some(h_doc);
        cq.ast_graph_turtle = Some(turtle.to_string());
        cq.canon_graph_bytes = Some(canon_bytes.clone());
        cq.cacheable = false;
        cq
    };
    match (bare, seq) {
        (Some((node, kind)), None) => {
            // Bare single root: unchanged member ladder; doc_hash = h_doc =
            // BLAKE3(Canon(G_AST)) — the same anchor compile_turtle pins as
            // receipt_state, so bare identity is bit-for-bit. Atomic mode is
            // a sequence-root-only feature; a bare root compiles non-atomic.
            require_statement_index(&reader, &node)?;
            let compiled = compile_statement_arm(&reader, &node, &kind)?;
            let mut statement = compiled;
            statement.ast_hash = Some(h_doc);
            statement.ast_graph_turtle = Some(turtle.to_string());
            statement.canon_graph_bytes = Some(canon_bytes);
            Ok(CompiledDocument {
                statements: vec![statement],
                doc_hash: h_doc,
                atomic: false,
            })
        }
        (None, Some(seq_node)) => {
            let statements_head = reader
                .get_single_object(&seq_node, "http://graphlaw.org/sql#statements")
                .ok_or_else(|| AstError::new(RefusalCode::MalformedAst, refusal_arm::SEQUENCE_EMPTY).with_ident(&seq_node))?;
            let members = reader.unroll_rdf_list(&statements_head, 1024)?;
            if members.is_empty() {
                return Err(AstError::new(
                    RefusalCode::MalformedAst,
                    refusal_arm::SEQUENCE_EMPTY,
                )
                .with_ident(&seq_node));
            }
            let mut statements = Vec::with_capacity(members.len());
            for (i, member) in members.iter().enumerate() {
                // Strictly-ascending 0-based index cross-check: the spine walk
                // order is the ordering source; the explicit index must agree.
                let (idx_lex, dtype) = reader
                    .get_datatype_literal(member, "http://graphlaw.org/sql#statementIndex")
                    .ok_or_else(|| {
                        AstError::new(
                            RefusalCode::MalformedAst,
                            refusal_arm::INDEX_SEQUENCE_DISORDER,
                        )
                        .with_ident(member)
                    })?;
                let ok = dtype.as_deref() == Some("http://www.w3.org/2001/XMLSchema#integer")
                    && idx_lex.parse::<i64>().map(|v| v == i as i64) == Ok(true);
                if !ok {
                    return Err(AstError::new(
                        RefusalCode::MalformedAst,
                        refusal_arm::INDEX_SEQUENCE_DISORDER,
                    )
                    .with_ident(member));
                }
                let kind = reader.member_kind(member)?;
                require_statement_index(&reader, member)?;
                let compiled = compile_statement_arm(&reader, member, &kind)
                    .map_err(|mut e| {
                        // Position attribution without changing the refusal
                        // payload family: predicate_idx is documented as the
                        // ordinal position the refusal attaches to.
                        e.ctx.predicate_idx = i as u16;
                        e
                    })
                    .map(&stamp)?;
                statements.push(compiled);
            }
            // Domain-separated fold: BLAKE3(0x00 || h1 || 0x00 || h2 || ...).
            let mut hasher = blake3::Hasher::new();
            for s in &statements {
                hasher.update(&[0x00]);
                hasher.update(s.ast_hash.as_ref().unwrap_or(&h_doc));
            }
            // SQLRDF-26: optional `sql:atomic true` on the sequence root.
            // Absent (or explicitly `false`) keeps SQLRDF-23 pipeline
            // semantics; `true` selects BEGIN/COMMIT/ROLLBACK execution.
            const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
            let atomic = match reader.get_datatype_literal(
                &seq_node,
                "http://graphlaw.org/sql#atomic",
            ) {
                None => false,
                Some((lex, dtype)) => {
                    if dtype.as_deref() != Some(XSD_BOOLEAN)
                        || !matches!(lex.as_str(), "true" | "false")
                    {
                        return Err(AstError::new(
                            RefusalCode::MalformedAst,
                            refusal_arm::ATOMIC_LITERAL,
                        )
                        .with_ident(&seq_node));
                    }
                    lex == "true"
                }
            };
            Ok(CompiledDocument {
                doc_hash: *hasher.finalize().as_bytes(),
                statements,
                atomic,
            })
        }
        (Some(_), Some(_)) => Err(mixed_roots()),
        (None, None) => Err(mixed_roots()),
    }
}

fn mixed_roots() -> AstError {
    AstError::new(RefusalCode::MalformedAst, refusal_arm::MIXED_DOCUMENT_ROOTS)
}

struct DatasetReader {
    outgoing: HashMap<String, HashMap<String, Vec<RdfTerm>>>,
    /// The admitted Turtle document this reader was parsed from
    /// (SQLRDF-17: lets document-backed callers compile via `compile_turtle`
    /// so every admission carries canon bytes).
    turtle: String,
}

#[derive(Clone)]
enum RdfTerm {
    Iri(String),
    Literal {
        lexical: String,
        datatype: Option<String>,
    },
}

impl DatasetReader {
    fn from_turtle(turtle: &str) -> Result<Self, AstError> {
        let dataset = purrdf_rdf::parse_dataset(turtle.as_bytes(), "text/turtle", None)
            .map_err(|_| AstError::new(RefusalCode::MalformedAst, refusal_arm::PARSE_DATASET))?;
        let mut outgoing: HashMap<String, HashMap<String, Vec<RdfTerm>>> = HashMap::new();
        for quad in dataset.quads() {
            let subj = match dataset.term_value(quad.s) {
                purrdf_core::TermValue::Iri(i) => i,
                purrdf_core::TermValue::Blank { label, scope } => format!("_:{label}_{}", scope.0),
                _ => continue,
            };
            let pred = match dataset.term_value(quad.p) {
                purrdf_core::TermValue::Iri(i) => i,
                _ => continue,
            };
            let term = match dataset.term_value(quad.o) {
                purrdf_core::TermValue::Iri(i) => RdfTerm::Iri(i),
                purrdf_core::TermValue::Blank { label, scope } => {
                    RdfTerm::Iri(format!("_:{label}_{}", scope.0))
                }
                purrdf_core::TermValue::Literal {
                    lexical_form,
                    datatype,
                    ..
                } => RdfTerm::Literal {
                    lexical: lexical_form,
                    datatype: Some(datatype),
                },
                _ => continue,
            };
            outgoing
                .entry(subj)
                .or_default()
                .entry(pred)
                .or_default()
                .push(term);
        }
        Ok(Self {
            outgoing,
            turtle: turtle.to_string(),
        })
    }

    /// Finds the top-level statement node. Nested statement-class nodes
    /// (e.g. the section 7 `sql:usingSource` sub-select) are objects of other
    /// subjects, so a candidate that is nobody's object wins; ties break on
    /// the lexicographically smallest subject for determinism.
    fn statement_kind(&self) -> Result<(String, String), AstError> {
        let ty = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        let prefix = "http://graphlaw.org/sql#";
        let is_object: HashSet<String> = self
            .outgoing
            .values()
            .flat_map(|preds| preds.values().flatten())
            .filter_map(|t| match t {
                RdfTerm::Iri(i) if i.starts_with("_:") => Some(i.clone()),
                _ => None,
            })
            .collect();
        let mut candidates: Vec<(String, String)> = Vec::new();
        for (subj, preds) in &self.outgoing {
            let Some(objs) = preds.get(ty) else { continue };
            for term in objs {
                let RdfTerm::Iri(iri) = term else { continue };
                if let Some(kind) = iri.strip_prefix(prefix)
                    && kind.ends_with("Statement")
                    && !is_object.contains(subj)
                {
                    candidates.push((subj.clone(), kind.to_string()));
                }
            }
        }
        candidates.sort();
        // SQLRDF-23: the former `into_iter().next()` silently picked the
        // lexicographically smallest root and DROPPED the rest. A document
        // with multiple bare roots (or roots of disjoint categories) refuses
        // typed now — never a silent pick.
        match candidates.len() {
            0 => {
                // Rec §2 namespace discipline: a root typed as a `*Statement`
                // class under a FOREIGN namespace refuses typed REFUSAL-01 —
                // the class IRI is present, so "missing rdf:type" would
                // misstate the defect. Free-form carrier arm (LEGACY_MESSAGE,
                // the pivot_refusal precedent); codes are append-only, no new
                // arm is minted.
                let foreign_class = self.outgoing.iter().find_map(|(subj, preds)| {
                    if is_object.contains(subj) {
                        return None;
                    }
                    preds.get(ty)?.iter().find_map(|t| match t {
                        RdfTerm::Iri(iri)
                            if iri.ends_with("Statement") && !iri.starts_with(prefix) =>
                        {
                            Some(iri.clone())
                        }
                        _ => None,
                    })
                });
                match foreign_class {
                    Some(iri) => Err(AstError::new(
                        RefusalCode::MalformedAst,
                        refusal_arm::LEGACY_MESSAGE,
                    )
                    .with_ident(format!(
                        "statement class '{iri}' is outside the sql-rdf namespace \
                         ({prefix}); only http://graphlaw.org/sql#*Statement \
                         classes are admissible"
                    ))),
                    None => Err(AstError::MissingProperty(ty.into(), "sql:*Statement".into())),
                }
            }
            1 => Ok(candidates.pop().expect("len == 1 guarded")),
            _ => Err(AstError::new(
                RefusalCode::MalformedAst,
                refusal_arm::MIXED_DOCUMENT_ROOTS,
            )),
        }
    }

    /// SQLRDF-23: classifies the document's top-level roots into bare
    /// `sql:*Statement` roots and the `sql:StatementSequence` root. A lawful
    /// document has exactly one of either; anything else (both, multiples of
    /// either, none) is `MIXED_DOCUMENT_ROOTS` — the silent lexicographic
    /// pick is closed.
    fn document_roots(
        &self,
    ) -> Result<(Option<(String, String)>, Option<String>), AstError> {
        let ty = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        let prefix = "http://graphlaw.org/sql#";
        // Unlike statement_kind's blank-label-only filter (kept untouched for
        // bit-for-bit bare-path compatibility), the document path treats a
        // node as somebody's object when ANY subject/predicate pair carries
        // it — an rdf:List member named as a full IRI is a member, not a
        // second root.
        let is_object: HashSet<String> = self
            .outgoing
            .values()
            .flat_map(|preds| preds.values().flatten())
            .filter_map(|t| match t {
                RdfTerm::Iri(i) => Some(i.clone()),
                _ => None,
            })
            .collect();
        let mut bare: Vec<(String, String)> = Vec::new();
        let mut seq: Vec<String> = Vec::new();
        for (subj, preds) in &self.outgoing {
            let Some(objs) = preds.get(ty) else { continue };
            for term in objs {
                let RdfTerm::Iri(iri) = term else { continue };
                if !is_object.contains(subj) {
                    if let Some(kind) = iri.strip_prefix(prefix) {
                        if kind == "StatementSequence" {
                            seq.push(subj.clone());
                        } else if kind.ends_with("Statement") {
                            bare.push((subj.clone(), kind.to_string()));
                        }
                    }
                }
            }
        }
        let total = bare.len() + seq.len();
        if total != 1 {
            return Err(mixed_roots());
        }
        Ok((bare.into_iter().next(), seq.into_iter().next()))
    }

    /// SQLRDF-23: the statement-class suffix (`"SelectStatement"` etc.) of a
    /// sequence member node — the member's own `rdf:type sql:*Statement` IRI.
    fn member_kind(&self, node: &str) -> Result<String, AstError> {
        let ty = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        let prefix = "http://graphlaw.org/sql#";
        let n = node.to_string();
        self.get_single_object(&n, ty)
            .and_then(|iri: String| iri.strip_prefix(prefix).map(str::to_string))
            .ok_or_else(|| AstError::MissingProperty(ty.into(), n))
    }

    fn ident(&self, node: &str, pred: &str) -> Result<String, AstError> {
        let node = node.to_string();
        let (lex, _) = self
            .get_datatype_literal(&node, pred)
            .ok_or_else(|| AstError::MissingProperty(pred.into(), node))?;
        validate_identifier(&lex).map(str::to_string)
    }

    /// All datatype literals on `(node, pred)` in reader order — the order
    /// `DatasetReader::from_turtle` pushed quads while walking
    /// `purrdf::parse_dataset(...).quads()`. SQLRDF-22 G4: multi-column
    /// INSERT/UPDATE assign pairs are matched positionally in this order
    /// (RDF graphs are unordered sets; no shared discriminator exists on the
    /// scalar template vocabulary, so reader-order is the lawful, documented
    /// pairing).
    fn all_lits(&self, node: &str, pred: &str) -> Vec<(String, Option<String>)> {
        self.outgoing
            .get(node)
            .and_then(|preds| preds.get(pred))
            .map(|terms| {
                terms
                    .iter()
                    .filter_map(|t| match t {
                        RdfTerm::Literal { lexical, datatype } => {
                            Some((lexical.clone(), datatype.clone()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn lit(&self, node: &str, pred: &str) -> Result<String, AstError> {
        let node = node.to_string();
        let (lex, dtype) = self
            .get_datatype_literal(&node, pred)
            .ok_or_else(|| AstError::MissingProperty(pred.into(), node))?;
        Ok(Self::render_scalar_literal(&lex, dtype.as_deref()))
    }

    /// All assign columns in reader order, each a validated identifier.
    fn all_idents(&self, node: &str, pred: &str) -> Result<Vec<String>, AstError> {
        self.all_lits(node, pred)
            .iter()
            .map(|(lex, _)| validate_identifier(lex).map(str::to_string))
            .collect()
    }

    /// All assign literals in reader order, each rendered with the same
    /// numeric-vs-quoted discipline as `lit`.
    fn all_assigns(&self, node: &str, pred: &str) -> Vec<String> {
        self.all_lits(node, pred)
            .iter()
            .map(|(lex, dtype)| Self::render_scalar_literal(lex, dtype.as_deref()))
            .collect()
    }

    /// SQLRDF-22 G4 (order recovery): the sql:assignColumn/sql:assignValue
    /// pairs of `node` in DOCUMENT (text) order, when the subject's
    /// statement can be located textually; `None` when it cannot (blank-node
    /// subjects, exotic serializations), in which case the caller falls back
    /// to reader-order pairing (unchanged behavior, same refusal family).
    ///
    /// Why this exists: purrdf's `quads()` does not preserve document order
    /// for repeated predicates (witnessed 2026-10-23: a two-pair
    /// all-`xsd:string` seed emitted its assignValue quads reversed relative
    /// to the document — `BOOK-A, ISIN-XX-0001` for a document ordering
    /// `ISIN-XX-0001, BOOK-A`), so the previous reader-order-only pairing
    /// silently crossed column/value pairs. RDF graphs are unordered sets;
    /// the ordering source is the admitted Turtle text itself.
    fn assign_pairs(
        &self,
        node: &str,
        col: &str,
        val: &str,
    ) -> Result<(Vec<String>, Vec<String>), AstError> {
        if let Some(pairs) = self.document_order_assigns(node, col, val) {
            let mut cols = Vec::with_capacity(pairs.len());
            let mut vals = Vec::with_capacity(pairs.len());
            for ((c, _), (v, d)) in pairs {
                cols.push(validate_identifier(&c).map(str::to_string)?);
                vals.push(Self::render_scalar_literal(&v, d.as_deref()));
            }
            return Ok((cols, vals));
        }
        // Fallback: reader-order pairing (behavior and refusal family
        // unchanged for subjects whose text form cannot be located).
        let cols = self.all_idents(node, col)?;
        let vals = self.all_assigns(node, val);
        Ok((cols, vals))
    }

    /// Document-order scan of the admitted Turtle text (quote-aware,
    /// statement-splitting tokenizer). Returns `None` when the subject's
    /// textual statement cannot be located (blank-node subjects, exotic
    /// serializations) — the caller falls back to reader order.
    fn document_order_assigns(
        &self,
        node: &str,
        col: &str,
        val: &str,
    ) -> Option<Vec<((String, Option<String>), (String, Option<String>))>> {
        // Minimal @prefix map over the document.
        let mut prefixes: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        for line in self.turtle.lines() {
            let l = line.trim();
            let Some(rest) = l.strip_prefix("@prefix") else { continue };
            let Some((pfx, iri_part)) = rest.trim().split_once(':') else { continue };
            let iri = iri_part
                .trim()
                .strip_prefix('<')
                .map(|s| s.trim_end().trim_end_matches('.').trim_end())
                .and_then(|s| s.strip_suffix('>'))
                .map(str::trim_end);
            if let (Some(p), Some(ns)) = (Some(pfx.trim()), iri) {
                prefixes.insert(p, ns);
            }
        }

        let expand = |tok: &str, prefixes: &std::collections::HashMap<&str, &str>| -> Option<String> {
            let tok = tok.trim_end_matches(';');
            if let Some(inner) = tok.strip_prefix('<') {
                return inner.strip_suffix('>').map(str::to_string);
            }
            if let Some((pfx, local)) = tok.split_once(':') {
                if let Some(ns) = prefixes.get(pfx) {
                    return Some(format!("{ns}{local}"));
                }
            }
            None
        };

        // Strip '#' comments (outside strings and <IRI>s) first: comment
        // text is not Turtle syntax and must not seed statement boundaries.
        let mut doc = String::with_capacity(self.turtle.len());
        for line in self.turtle.lines() {
            let lb = line.as_bytes();
            let mut q = false;
            let mut iri = false;
            let mut cut = lb.len();
            for (idx, ch) in lb.iter().enumerate() {
                match *ch {
                    b'"' if !iri => q = !q,
                    b'<' if !q => iri = true,
                    b'>' if !q => iri = false,
                    b'#' if !q && !iri => { cut = idx; break; }
                    _ => {}
                }
            }
            doc.push_str(&line[..cut]);
            doc.push('\n');
        }

        // Quote/IRI-aware statement splitting: a '.' outside strings and
        // <IRI>s followed by whitespace or EOF ends a statement.
        let b = doc.as_bytes();
        let n = b.len();
        let mut stmts: Vec<(usize, usize)> = Vec::new();
        let mut start = 0usize;
        let mut i = 0usize;
        let mut in_string = false;
        let mut in_iri = false;
        while i < n {
            let c = b[i];
            if in_string {
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == b'"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            match c {
                b'"' => { in_string = true; i += 1; }
                b'<' => { in_iri = true; i += 1; }
                b'>' => { in_iri = false; i += 1; }
                b'.' if !in_iri
                    && (i + 1 >= n || (b[i + 1] as char).is_ascii_whitespace()) =>
                {
                    stmts.push((start, i));
                    start = i + 1;
                    i += 1;
                }
                _ => { i += 1; }
            }
        }
        if i > start {
            stmts.push((start, i));
        }

        let mut pairs: Vec<((String, Option<String>), (String, Option<String>))> = Vec::new();
        let mut open_col: Option<(String, Option<String>)> = None;
        for (s0, s1) in stmts {
            let stmt = &doc[s0..s1];
            let mut tokens: Vec<&str> = Vec::new();
            let bs = stmt.as_bytes();
            let mut j = 0usize;
            let mut instr = false;
            let mut iniri = false;
            let mut tok = 0usize;
            while j < bs.len() {
                let c = bs[j];
                if instr {
                    if c == b'\\' {
                        j += 2;
                        continue;
                    }
                    if c == b'"' {
                        instr = false;
                    }
                    j += 1;
                    continue;
                }
                match c {
                    b'"' => { instr = true; j += 1; }
                    b'<' => { iniri = true; j += 1; }
                    b'>' => { iniri = false; j += 1; }
                    c if (c as char).is_ascii_whitespace() => {
                        if j > tok {
                            tokens.push(&stmt[tok..j]);
                        }
                        j += 1;
                        tok = j;
                    }
                    _ => { j += 1; }
                }
            }
            if bs.len() > tok {
                tokens.push(&stmt[tok..]);
            }
            if tokens.is_empty() {
                continue;
            }

            // Directive pseudo-statements are skipped; unknown subject
            // forms (blank nodes, collections) fall back to reader order at
            // the caller.
            if tokens[0].starts_with('@') {
                continue;
            }
            let Some(subj) = expand(tokens[0], &prefixes) else {
                // Unknown subject form anywhere in the document: the
                // document-order source is unreliable, so fall back wholesale.
                return None;
            };
            if subj != node {
                continue;
            }

            // Walk predicate/object tokens in text order.
            let mut k = 1usize;
            while k < tokens.len() {
                let t = expand(tokens[k], &prefixes);
                let is_col = t.as_deref() == Some(col);
                let is_val = t.as_deref() == Some(val);
                if is_col || is_val {
                    let otok = tokens.get(k + 1).map(|s| s.trim_end_matches(';'));
                    let Some(otok) = otok else { break };
                    let Some(lit) = Self::parse_turtle_literal_token(otok, &prefixes) else { break };
                    if is_col {
                        open_col = Some(lit);
                    } else if let Some(c) = open_col.take() {
                        pairs.push((c, lit));
        }
                    k += 2;
                    continue;
                }
                k += 1;
            }
        }
        // Integrity: the scan must have paired EVERY assignColumn and
        // assignValue literal the reader saw; any unpaired residue (a column
        // with no following value, a value with no preceding column, or an
        // extra literal the tokenizer missed) makes the document-order source
        // unreliable — fall back to reader order so the count-mismatch
        // refusal family fires on the full vectors.
        if open_col.is_some()
            || pairs.len() != self.all_lits(node, col).len()
            || pairs.len() != self.all_lits(node, val).len()
        {
            return None;
        }
        Some(pairs)
    }

    /// Parse one Turtle literal token: `"lex"` optionally `^^dtype`, or a
    /// plain numeric/boolean token (Turtle types bare integers as
    /// xsd:integer and decimals as xsd:decimal — mirror that).
    fn parse_turtle_literal_token(
        tok: &str,
        prefixes: &std::collections::HashMap<&str, &str>,
    ) -> Option<(String, Option<String>)> {
        if !tok.starts_with('"') {
            // Plain token: numeric or boolean keyword.
            let is_bool = tok == "true" || tok == "false";
            let all_digits = !tok.is_empty() && tok.chars().all(|c| c.is_ascii_digit());
            let is_decimal = !tok.is_empty()
                && tok.split('.').count() == 2
                && tok.chars().all(|c| c.is_ascii_digit() || c == '.');
            let dt = if is_bool {
                Some("http://www.w3.org/2001/XMLSchema#boolean".to_string())
            } else if all_digits {
                Some("http://www.w3.org/2001/XMLSchema#integer".to_string())
            } else if is_decimal {
                Some("http://www.w3.org/2001/XMLSchema#decimal".to_string())
            } else {
                None
            };
            return Some((tok.to_string(), dt));
        }
        // Find the closing quote honoring backslash escapes.
        let bytes = tok.as_bytes();
        let mut j = 1usize;
        let mut lex = String::new();
        while j < bytes.len() {
            match bytes[j] {
                b'\\' if j + 1 < bytes.len() => {
                    lex.push(bytes[j + 1] as char);
                    j += 2;
                }
                b'"' => break,
                c => {
                    lex.push(c as char);
                    j += 1;
                }
            }
        }
        let tail = &tok[j + 1..];
        let dtype = tail.strip_prefix("^^").map(|dt| {
            let dt = dt.trim();
            if let Some(inner) = dt.strip_prefix('<') {
                inner.trim_end_matches('>').to_string()
            } else if let Some((pfx, local)) = dt.split_once(':') {
                prefixes
                    .get(pfx)
                    .map(|ns| format!("{ns}{local}"))
                    .unwrap_or_else(|| dt.to_string())
            } else {
                dt.to_string()
            }
        });
        Some((lex, dtype))
    }
    /// SQLRDF-22 G1: an xsd:decimal literal with a lossless decimal
    /// lexical (digits, exactly one '.', no exponent) renders as an
    /// unquoted numeric SQL literal — DuckDB coerces it to DECIMAL. A
    /// decimal-TYPED literal whose lexical is not numeric stays a
    /// quoted string (typed-shape discipline: never fabricated, never
    /// a typed refusal for a well-formed AST).
    fn render_scalar_literal(lex: &str, dtype: Option<&str>) -> String {
        let decimal_lexical = {
            let mut parts = lex.split('.');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(i), Some(f), None)
                    if !i.is_empty()
                        && !f.is_empty()
                        && i.chars().all(|c| c.is_ascii_digit())
                        && f.chars().all(|c| c.is_ascii_digit()) =>
                {
                    true
                }
                _ => false,
            }
        };
        let numeric = dtype
            .is_some_and(|d| {
                d.ends_with("integer")
                    || d.ends_with("double")
                    || (d.ends_with("decimal") && decimal_lexical)
            })
            || lex.chars().all(|c| c.is_ascii_digit());
        if numeric {
            lex.to_string()
        } else {
            format!("'{}'", lex.replace('\'', "''"))
        }
    }

    /// Compiles the expression-level form of a command class from its
    /// structured graph (nested operand nodes, rdf:Lists, nested SELECT
    /// expressions). Returns `Ok(None)` when the statement does not use the
    /// expression-level vocabulary, so the scalar template vocabulary remains
    /// a lawful fallback (rec 3.2: both forms are the same admitted class).
    fn compile_expression_form(&self, node: &str, kind: &str) -> Result<Option<String>, AstError> {
        let node = node.to_string();
        let p = |name: &str| format!("http://graphlaw.org/sql#{name}");
        match kind {
            "CreateStatement" => {
                let Some(def) = self.get_single_object(&node, &p("tableDefinition")) else {
                    return Ok(None);
                };
                let table = self.ident(&def, &p("tableName"))?;
                let cols_head = self
                    .get_single_object(&def, &p("columns"))
                    .ok_or_else(|| AstError::MissingProperty(p("columns"), def.clone()))?;
                let mut cols = Vec::new();
                for col in self.unroll_rdf_list(&cols_head, 64)? {
                    let name = self.ident(&col, &p("columnName"))?;
                    let ty = self.column_type(&col)?;
                    cols.push(format!("{name} {ty}"));
                }
                Ok(Some(format!("CREATE TABLE {table} ({})", cols.join(", "))))
            }
            "DropStatement" => {
                let Some(target) = self.get_single_object(&node, &p("dropTarget")) else {
                    return Ok(None);
                };
                let table = self.ident(&target, &p("tableName"))?;
                let if_exists = self
                    .get_datatype_literal(&target, &p("ifExists"))
                    .map(|(v, _)| v == "true" || v == "1")
                    .unwrap_or(false);
                Ok(Some(if if_exists {
                    format!("DROP TABLE IF EXISTS {table}")
                } else {
                    format!("DROP TABLE {table}")
                }))
            }
            "AlterStatement" => {
                let Some(action) = self.get_single_object(&node, &p("alterAction")) else {
                    return Ok(None);
                };
                let table = self.ident(&action, &p("tableName"))?;
                let col = self.ident(&action, &p("columnName"))?;
                let ty = self.column_type(&action)?;
                Ok(Some(format!("ALTER TABLE {table} ADD COLUMN {col} {ty}")))
            }
            "TruncateStatement" => {
                let Some(target) = self.get_single_object(&node, &p("truncateTarget")) else {
                    return Ok(None);
                };
                let table = self.ident(&target, &p("tableName"))?;
                Ok(Some(format!("TRUNCATE {table}")))
            }
            "AnalyzeStatement" => {
                let Some(target) = self.get_single_object(&node, &p("analyzeTarget")) else {
                    return Ok(None);
                };
                let table = self.ident(&target, &p("tableName"))?;
                Ok(Some(format!("ANALYZE {table}")))
            }
            "CommentStatement" => {
                let Some(comment) = self.get_single_object(&node, &p("comment")) else {
                    return Ok(None);
                };
                let object = self
                    .get_single_object(&comment, &p("commentObject"))
                    .ok_or_else(|| {
                        AstError::MissingProperty(p("commentObject"), comment.clone())
                    })?;
                let object = match object.as_str() {
                    "http://graphlaw.org/sql#Table" => "TABLE",
                    other => {
                        return Err(AstError::UnsupportedTerm(other.to_string()));
                    }
                };
                let name = self.ident(&comment, &p("commentTargetName"))?;
                let text = self
                    .get_datatype_literal(&comment, &p("commentText"))
                    .ok_or_else(|| AstError::MissingProperty(p("commentText"), comment.clone()))?;
                Ok(Some(format!(
                    "COMMENT ON {object} {name} IS '{}'",
                    text.0.replace('\'', "''")
                )))
            }
            "CopyStatement" => {
                let Some(op) = self.get_single_object(&node, &p("copyOperation")) else {
                    return Ok(None);
                };
                let table = self.ident(&op, &p("tableName"))?;
                let (path, _) = self
                    .get_datatype_literal(&op, &p("filePath"))
                    .ok_or_else(|| AstError::MissingProperty(p("filePath"), op.clone()))?;
                let fmt = self
                    .get_datatype_literal(&op, &p("fileFormat"))
                    .map(|(f, _)| f)
                    .unwrap_or_else(|| "CSV".to_string());
                if !matches!(fmt.as_str(), "CSV" | "PARQUET" | "JSON") {
                    return Err(AstError::UnsupportedTerm(fmt));
                }
                Ok(Some(format!(
                    "COPY {table} TO '{}' (FORMAT {fmt})",
                    path.replace('\'', "''")
                )))
            }
            "ExportStatement" => {
                let Some((path, _)) = self.get_datatype_literal(&node, &p("exportPath")) else {
                    return Ok(None);
                };
                Ok(Some(format!(
                    "EXPORT DATABASE '{}'",
                    path.replace('\'', "''")
                )))
            }
            "ImportStatement" => {
                let Some((path, _)) = self.get_datatype_literal(&node, &p("importPath")) else {
                    return Ok(None);
                };
                Ok(Some(format!(
                    "IMPORT DATABASE '{}'",
                    path.replace('\'', "''")
                )))
            }
            "PrepareStatement" => {
                let Some(target) = self.get_single_object(&node, &p("prepareTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("statementName"))?;
                let query_node = self
                    .get_single_object(&target, &p("prepareQuery"))
                    .ok_or_else(|| AstError::MissingProperty(p("prepareQuery"), target.clone()))?;
                let query = self.inline_select(&query_node)?;
                Ok(Some(format!("PREPARE {name} AS {query}")))
            }
            "ExecuteStatement" => {
                let Some(target) = self.get_single_object(&node, &p("executeTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("statementName"))?;
                Ok(Some(format!("EXECUTE {name}")))
            }
            "DeallocateStatement" => {
                let Some(target) = self.get_single_object(&node, &p("deallocateTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("statementName"))?;
                Ok(Some(format!("DEALLOCATE {name}")))
            }
            "ExplainStatement" => {
                let Some(query_node) = self.get_single_object(&node, &p("explainQuery")) else {
                    return Ok(None);
                };
                let query = self.inline_select(&query_node)?;
                Ok(Some(format!("EXPLAIN {query}")))
            }
            "CallStatement" => {
                let Some(target) = self.get_single_object(&node, &p("callTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("routineName"))?;
                let args = match self.get_single_object(&target, &p("arguments")) {
                    Some(head) => {
                        let items = self.unroll_rdf_list(&head, 64)?;
                        let mut rendered = Vec::with_capacity(items.len());
                        for item in items {
                            rendered.push(self.inline_expr(&item)?);
                        }
                        rendered.join(", ")
                    }
                    None => String::new(),
                };
                Ok(Some(format!("CALL {name}({args})")))
            }
            "PragmaStatement" => {
                let Some(target) = self.get_single_object(&node, &p("pragmaTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("pragmaName"))?;
                Ok(Some(format!("PRAGMA {name}")))
            }
            "SetStatement" => {
                let Some(setting) = self.get_single_object(&node, &p("configSetting")) else {
                    return Ok(None);
                };
                let key = self.ident(&setting, &p("configKey"))?;
                let value_node = self
                    .get_single_object(&setting, &p("configValue"))
                    .ok_or_else(|| AstError::MissingProperty(p("configValue"), setting.clone()))?;
                let value = self.inline_expr(&value_node)?;
                Ok(Some(format!("SET {key} = {value}")))
            }
            "ResetStatement" => {
                let Some(target) = self.get_single_object(&node, &p("resetTarget")) else {
                    return Ok(None);
                };
                let key = self.ident(&target, &p("configKey"))?;
                Ok(Some(format!("RESET {key}")))
            }
            "TransactionStatement" => {
                // BEGIN/COMMIT/ROLLBACK compile to engine text; transaction
                // state is a connection-runtime property, so COMMIT/ROLLBACK
                // without an active BEGIN surfaces the engine's
                // TransactionContext error through the typed
                // EvaluatorError::DuckDb path at execution (documented
                // behavior, same class as a missing-table engine error).
                let action = self
                    .get_single_object(&node, &p("transactionAction"))
                    .unwrap_or_else(|| "http://graphlaw.org/sql#BeginTransaction".to_string());
                let sql = match action.as_str() {
                    "http://graphlaw.org/sql#BeginTransaction" => "BEGIN",
                    "http://graphlaw.org/sql#CommitTransaction" => "COMMIT",
                    "http://graphlaw.org/sql#RollbackTransaction" => "ROLLBACK",
                    other => return Err(AstError::UnsupportedTerm(other.to_string())),
                };
                Ok(Some(sql.to_string()))
            }
            "LoadStatement" => {
                let Some(target) = self.get_single_object(&node, &p("loadTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("extensionName"))?;
                Ok(Some(format!("LOAD {name}")))
            }
            "InstallStatement" => {
                let Some(target) = self.get_single_object(&node, &p("installTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("extensionName"))?;
                let from = match self.get_datatype_literal(&target, &p("repositoryPath")) {
                    Some((path, _)) => format!(" FROM '{}'", path.replace('\'', "''")),
                    None => String::new(),
                };
                Ok(Some(format!("INSTALL {name}{from}")))
            }
            "AttachStatement" => {
                let Some(target) = self.get_single_object(&node, &p("attachTarget")) else {
                    return Ok(None);
                };
                let (path, _) = self
                    .get_datatype_literal(&target, &p("databasePath"))
                    .ok_or_else(|| AstError::MissingProperty(p("databasePath"), target.clone()))?;
                let alias = self.ident(&target, &p("attachAlias"))?;
                Ok(Some(format!(
                    "ATTACH '{}' AS {alias}",
                    path.replace('\'', "''")
                )))
            }
            "DetachStatement" => {
                let Some(target) = self.get_single_object(&node, &p("detachTarget")) else {
                    return Ok(None);
                };
                let alias = self.ident(&target, &p("attachAlias"))?;
                Ok(Some(format!("DETACH {alias}")))
            }
            "UseStatement" => {
                let Some(target) = self.get_single_object(&node, &p("useTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("databaseName"))?;
                Ok(Some(format!("USE {name}")))
            }
            "ExtensionRepositoryStatement" => {
                let Some(target) = self.get_single_object(&node, &p("repositoryTarget")) else {
                    return Ok(None);
                };
                let name = self.ident(&target, &p("repositoryName"))?;
                let (prefix, _) = self
                    .get_datatype_literal(&target, &p("repositoryPrefix"))
                    .ok_or_else(|| {
                        AstError::MissingProperty(p("repositoryPrefix"), target.clone())
                    })?;
                let (key, _) = self
                    .get_datatype_literal(&target, &p("publicKey"))
                    .ok_or_else(|| AstError::MissingProperty(p("publicKey"), target.clone()))?;
                Ok(Some(format!(
                    "SET allow_extension_repositories='allowed'; CREATE OR REPLACE EXTENSION REPOSITORY {name} WITH PREFIX '{}' USING PUBLIC KEY '{}'",
                    prefix.replace('\'', "''"),
                    key.replace('\'', "''")
                )))
            }
            _ => Ok(None),
        }
    }

    /// Validates a column-type literal against the closed allowlist.
    fn column_type(&self, node: &str) -> Result<String, AstError> {
        let ty = "http://graphlaw.org/sql#columnType";
        let (lex, _) = self
            .get_datatype_literal(&node.to_string(), ty)
            .ok_or_else(|| AstError::MissingProperty(ty.to_string(), node.to_string()))?;
        let upper = lex.to_ascii_uppercase();
        match upper.as_str() {
            "INTEGER" | "BIGINT" | "VARCHAR" | "DOUBLE" | "BOOLEAN" | "DATE" | "DECIMAL" => {
                Ok(upper)
            }
            other => Err(AstError::UnsupportedTerm(format!("column type {other}"))),
        }
    }

    /// Compiles a nested SELECT expression graph into inline SQL (literals
    /// rendered in place, not as bind parameters) for embedding in EXPLAIN /
    /// PREPARE.
    fn inline_select(&self, query_node: &str) -> Result<String, AstError> {
        let stmt = self.extract_select_statement(&query_node.to_string())?;
        validate_identifier(&stmt.from.name)?;
        let mut sql = String::from("SELECT ");
        if stmt.select_list.is_empty() {
            sql.push('*');
        } else {
            for (idx, item) in stmt.select_list.iter().enumerate() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                match item {
                    SelectItem::Expr(expr) => sql.push_str(&self.inline_expr_node(expr)?),
                    SelectItem::Star { exclude, replace } => {
                        sql.push('*');
                        if !exclude.is_empty() {
                            sql.push_str(" EXCLUDE (");
                            for (i, col) in exclude.iter().enumerate() {
                                if i > 0 {
                                    sql.push_str(", ");
                                }
                                validate_identifier(col)?;
                                sql.push_str(col);
                            }
                            sql.push(')');
                        }
                        if !replace.is_empty() {
                            sql.push_str(" REPLACE (");
                            for (i, (expr, name)) in replace.iter().enumerate() {
                                if i > 0 {
                                    sql.push_str(", ");
                                }
                                sql.push_str(&self.inline_expr_node(expr)?);
                                sql.push_str(" AS ");
                                validate_identifier(name)?;
                                sql.push_str(name);
                            }
                            sql.push(')');
                        }
                    }
                    SelectItem::Columns(pattern) => {
                        sql.push_str("COLUMNS('");
                        sql.push_str(&pattern.replace('\'', "''"));
                        sql.push_str("')");
                    }
                }
            }
        }
        sql.push_str(" FROM ");
        sql.push_str(&stmt.from.name);
        if let Some(alias) = &stmt.from.alias {
            validate_identifier(alias)?;
            sql.push(' ');
            sql.push_str(alias);
        }
        for join in &stmt.joins {
            validate_identifier(&join.table.name)?;
            sql.push(' ');
            sql.push_str(join.join_type.as_sql());
            sql.push(' ');
            sql.push_str(&join.table.name);
            if let Some(alias) = &join.table.alias {
                validate_identifier(alias)?;
                sql.push(' ');
                sql.push_str(alias);
            }
            sql.push_str(" ON ");
            sql.push_str(&self.inline_expr_node(&join.condition)?);
        }
        if let Some(w) = &stmt.where_clause {
            sql.push_str(" WHERE ");
            sql.push_str(&self.inline_expr_node(w)?);
        }
        if !stmt.group_by.is_empty() {
            sql.push_str(" GROUP BY ");
            for (idx, g) in stmt.group_by.iter().enumerate() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                sql.push_str(&self.inline_expr_node(g)?);
            }
        }
        if let Some(limit) = stmt.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }
        if let Some(offset) = stmt.offset {
            sql.push_str(&format!(" OFFSET {offset}"));
        }
        Ok(sql)
    }

    /// Renders an already-extracted `SqlExpr` inline (literals in place).
    fn inline_expr_node(&self, expr: &SqlExpr) -> Result<String, AstError> {
        let mut sql = String::new();
        self.write_inline_expr(expr, &mut sql, 0)?;
        Ok(sql)
    }

    /// Extracts and renders an operand expression node from the graph.
    fn inline_expr(&self, node: &str) -> Result<String, AstError> {
        let expr = self.extract_expr(&node.to_string(), 0)?;
        self.inline_expr_node(&expr)
    }

    fn write_inline_expr(
        &self,
        expr: &SqlExpr,
        sql: &mut String,
        depth: usize,
    ) -> Result<(), AstError> {
        if depth > 32 {
            return Err(AstError::RecursionLimitExceeded(32));
        }
        match expr {
            SqlExpr::ColumnRef {
                column,
                table_alias,
            } => {
                validate_identifier(column)?;
                if let Some(alias) = table_alias {
                    validate_identifier(alias)?;
                    sql.push_str(alias);
                    sql.push('.');
                }
                sql.push_str(column);
            }
            SqlExpr::Literal(lit) => match lit {
                SqlLiteral::Integer(i) => sql.push_str(&i.to_string()),
                SqlLiteral::Float(f) => sql.push_str(&f.to_string()),
                SqlLiteral::Boolean(b) => sql.push_str(if *b { "TRUE" } else { "FALSE" }),
                SqlLiteral::Null => sql.push_str("NULL"),
                SqlLiteral::String(s) => {
                    sql.push('\'');
                    sql.push_str(&s.replace('\'', "''"));
                    sql.push('\'');
                }
            },
            SqlExpr::Binary { op, left, right } => {
                sql.push('(');
                self.write_inline_expr(left, sql, depth + 1)?;
                sql.push(' ');
                sql.push_str(op.as_sql());
                sql.push(' ');
                self.write_inline_expr(right, sql, depth + 1)?;
                sql.push(')');
            }
            SqlExpr::FunctionCall { function, argument } => {
                sql.push_str(function.as_sql());
                sql.push('(');
                if function == &AggregateFunction::RowNumber {
                    sql.push(')');
                    return Ok(());
                }
                self.write_inline_expr(argument, sql, depth + 1)?;
                sql.push(')');
            }
            SqlExpr::WindowFn {
                function,
                partition_by,
                order_by,
                frame,
            } => {
                self.write_inline_expr(function, sql, depth + 1)?;
                sql.push_str(" OVER (");
                let mut first = true;
                if !partition_by.is_empty() {
                    sql.push_str("PARTITION BY ");
                    for (idx, part) in partition_by.iter().enumerate() {
                        if idx > 0 {
                            sql.push_str(", ");
                        }
                        self.write_inline_expr(part, sql, depth + 1)?;
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
                        self.write_inline_expr(&term.expr, sql, depth + 1)?;
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
                }
                sql.push(')');
            }
            SqlExpr::Alias { expr, alias } => {
                validate_identifier(alias)?;
                self.write_inline_expr(expr, sql, depth + 1)?;
                sql.push_str(" AS ");
                sql.push_str(alias);
            }
            // SQLRDF-25 Row 4 minimal unblock (lane sq1, compile-freeze SLA,
            // disclosed): composite terms carry a typed carrier on the inline
            // (string-interpolating) path; the parameterized serializer is
            // the real Row 4 render site. Courts come later.
            // PENDING-LANE: typed unsupported carriers; feature completion
            // stays with the in-flight lane (compile-freeze SLA unblock).
            SqlExpr::StructLiteral { .. } => {
                return Err(AstError::UnsupportedTerm(
                    "SqlExpr::StructLiteral (inline path)".to_string(),
                ))
            }
            SqlExpr::ListLiteral { .. } => {
                return Err(AstError::UnsupportedTerm(
                    "SqlExpr::ListLiteral (inline path)".to_string(),
                ))
            }
            SqlExpr::IndexAccess { .. } => {
                return Err(AstError::UnsupportedTerm(
                    "SqlExpr::IndexAccess (inline path)".to_string(),
                ))
            }
            SqlExpr::SliceAccess { .. } => {
                return Err(AstError::UnsupportedTerm(
                    "SqlExpr::SliceAccess (inline path)".to_string(),
                ))
            }
            SqlExpr::Unary { op, expr } => {
                sql.push_str(op.as_sql());
                self.write_inline_expr(expr, sql, depth + 1)?;
            }
            SqlExpr::Between { expr, low, high } => {
                self.write_inline_expr(expr, sql, depth + 1)?;
                sql.push_str(" BETWEEN ");
                self.write_inline_expr(low, sql, depth + 1)?;
                sql.push_str(" AND ");
                self.write_inline_expr(high, sql, depth + 1)?;
            }
        }
        Ok(())
    }

    fn compile_command(&self, node: &str, kind: &str) -> Result<CompiledQuery, AstError> {
        if let Some(sql) = self.compile_expression_form(node, kind)? {
            return Ok(CompiledQuery::raw_compiled(sql, Vec::new()));
        }
        let table = "http://graphlaw.org/sql#targetTable";
        let col = "http://graphlaw.org/sql#assignColumn";
        let val = "http://graphlaw.org/sql#assignValue";
        let wcol = "http://graphlaw.org/sql#whereColumn";
        let wval = "http://graphlaw.org/sql#whereValue";
        // CTQ-4 / REFUSAL-05: a WHERE-less UPDATE/DELETE would render SQL that
        // mutates every row of the target table. Compiler law is stricter than
        // the SHACL xone (minCount 1 on either predicate): BOTH sql:whereColumn
        // and sql:whereValue are required, closing the guard/render asymmetry
        // (a whereValue-only doc passed the either-guard but the render arms
        // emit WHERE from whereColumn alone — unbounded SQL). Refuse at
        // compile time, before any engine call. Append-only code; see
        // `src/sql_ast/refusals.rs`.
        if matches!(kind, "UpdateStatement" | "DeleteStatement")
            && (self.get_datatype_literal(&node.to_string(), wcol).is_none()
                || self.get_datatype_literal(&node.to_string(), wval).is_none())
        {
            return Err(AstError::new(
                RefusalCode::UnboundedMutation,
                refusal_arm::UNBOUNDED_MUTATION,
            )
            .with_ident(node));
        }
        let sql = match kind {
            // SQLRDF-22 G4: ALL sql:assignColumn/sql:assignValue literals
            // are paired positionally, preferring DOCUMENT order (recovered
            // from the admitted Turtle text — see `assign_pairs`; purrdf's
            // quads() does not preserve document order for repeated
            // predicates) and falling back to reader order when the subject's
            // textual statement cannot be located. One pair renders exactly
            // as before (backward compatible); zero pairs refuse typed
            // REFUSAL-01 MissingProperty via `ident`; mismatched column/value
            // counts refuse typed the same way.
            "InsertStatement" => {
                let (cols, vals) = self.assign_pairs(node, col, val)?;
                if cols.is_empty() {
                    // Zero-pair INSERT: typed refusal, never a malformed
                    // `INSERT INTO t () VALUES ()`.
                    self.ident(node, col)?; // MissingProperty on sql:assignColumn
                }
                if cols.len() != vals.len() {
                    return Err(AstError::MissingProperty(
                        val.into(),
                        node.to_string(),
                    ));
                }
                format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    self.ident(node, table)?,
                    cols.join(", "),
                    vals.join(", ")
                )
            }
            "UpdateStatement" => {
                let (cols, vals) = self.assign_pairs(node, col, val)?;
                if cols.is_empty() {
                    self.ident(node, col)?; // MissingProperty on sql:assignColumn
                }
                if cols.len() != vals.len() {
                    return Err(AstError::MissingProperty(
                        val.into(),
                        node.to_string(),
                    ));
                }
                let assigns: Vec<String> = cols
                    .iter()
                    .zip(vals.iter())
                    .map(|(c, v)| format!("{c} = {v}"))
                    .collect();
                let mut sql = format!(
                    "UPDATE {} SET {}",
                    self.ident(node, table)?,
                    assigns.join(", ")
                );
                if self.get_datatype_literal(&node.to_string(), wcol).is_some() {
                    sql.push_str(&format!(
                        " WHERE {} = {}",
                        self.ident(node, wcol)?,
                        self.lit(node, wval)?
                    ));
                }
                sql
            }
            "DeleteStatement" => {
                let t = self.ident(node, table)?;
                if self.get_datatype_literal(&node.to_string(), wcol).is_some() {
                    format!(
                        "DELETE FROM {t} WHERE {} = {}",
                        self.ident(node, wcol)?,
                        self.lit(node, wval)?
                    )
                } else {
                    format!("DELETE FROM {t}")
                }
            }
            "CreateStatement" => format!(
                "CREATE TABLE {} ({} INTEGER)",
                self.ident(node, table)?,
                self.ident(node, col)?
            ),
            "DropStatement" => format!("DROP TABLE IF EXISTS {}", self.ident(node, table)?),
            "AlterStatement" => format!(
                "ALTER TABLE {} ADD COLUMN {} INTEGER",
                self.ident(node, table)?,
                self.ident(node, col)?
            ),
            "TruncateStatement" => format!("TRUNCATE {}", self.ident(node, table)?),
            "AnalyzeStatement" => format!("ANALYZE {}", self.ident(node, table)?),
            "ExplainStatement" => format!(
                "EXPLAIN SELECT {} FROM {}",
                self.ident(node, col)?,
                self.ident(node, table)?
            ),
            "CopyStatement" => {
                let t = self.ident(node, table)?;
                format!("COPY {t} TO '/tmp/graphlaw-rdf-{t}.csv' (FORMAT CSV)")
            }
            "ExportStatement" => {
                let n = self.ident(node, table)?;
                format!("EXPORT DATABASE '/tmp/graphlaw-rdf-export-{n}'")
            }
            "ImportStatement" => {
                let n = self.ident(node, table)?;
                format!(
                    "ATTACH '/tmp/graphlaw-rdf-{n}.duckdb' AS {n}; USE {n}; CREATE OR REPLACE TABLE only_src(id INTEGER); EXPORT DATABASE '/tmp/graphlaw-rdf-export-{n}'; USE memory; DETACH {n}; IMPORT DATABASE '/tmp/graphlaw-rdf-export-{n}'"
                )
            }
            "MergeIntoStatement" => {
                let t = self.ident(node, table)?;
                let c = self.ident(node, col)?;
                let v = self.lit(node, val)?;
                format!(
                    "MERGE INTO {t} USING (SELECT {v} AS {c}) s ON {t}.{c} = s.{c} WHEN MATCHED THEN UPDATE SET {c} = s.{c}"
                )
            }
            "VacuumStatement" => "VACUUM".into(),
            "CheckpointStatement" => "CHECKPOINT".into(),
            "TransactionStatement" => "BEGIN".into(),
            "ExternalResourceStatement" => "SHOW EXTERNAL RESOURCES".into(),
            "UpdateExtensionsStatement" => "UPDATE EXTENSIONS".into(),
            "SetStatement" => format!(
                "SET {} = {}",
                self.ident(node, table)?,
                self.lit(node, val)?
            ),
            "ResetStatement" => format!("RESET {}", self.ident(node, table)?),
            "PragmaStatement" => format!("PRAGMA {}", self.ident(node, table)?),
            "CallStatement" => format!("CALL {}()", self.ident(node, table)?),
            "PrepareStatement" => format!(
                "PREPARE {} AS SELECT {}",
                self.ident(node, table)?,
                self.lit(node, val)?
            ),
            "ExecuteStatement" => format!(
                "PREPARE {} AS SELECT {}; EXECUTE {}",
                self.ident(node, table)?,
                self.lit(node, val)?,
                self.ident(node, table)?
            ),
            "DeallocateStatement" => format!(
                "PREPARE {} AS SELECT 1; DEALLOCATE {}",
                self.ident(node, table)?,
                self.ident(node, table)?
            ),
            "CommentStatement" => format!(
                "COMMENT ON TABLE {} IS {}",
                self.ident(node, table)?,
                self.lit(node, val)?
            ),
            "LoadStatement" => format!("LOAD {}", self.ident(node, table)?),
            "InstallStatement" => format!(
                "INSTALL {} FROM {}",
                self.ident(node, table)?,
                self.lit(node, val)?
            ),
            "AttachStatement" => format!("ATTACH ':memory:' AS {}", self.ident(node, table)?),
            "UseStatement" => {
                let n = self.ident(node, table)?;
                format!("ATTACH ':memory:' AS {n}; USE {n}")
            }
            "DetachStatement" => {
                let n = self.ident(node, table)?;
                format!("ATTACH ':memory:' AS {n}; DETACH {n}")
            }
            "ExtensionRepositoryStatement" => format!(
                "SET allow_extension_repositories='allowed'; CREATE OR REPLACE EXTENSION REPOSITORY {} WITH PREFIX {} USING PUBLIC KEY {}",
                self.ident(node, table)?,
                self.lit(node, col)?,
                self.lit(node, val)?
            ),
            other => {
                return Err(AstError::UnsupportedTerm(format!(
                    "compile refusal: {other} has no SQL template"
                )));
            }
        };
        Ok(CompiledQuery::raw_compiled(sql, Vec::new()))
    }
}

impl RdfAstGraphReader for DatasetReader {
    type Node = String;

    fn query_document_turtle(&self) -> Option<String> {
        Some(self.turtle.clone())
    }

    fn object_count(&self, node: &String, predicate: &str) -> usize {
        self.outgoing
            .get(node)
            .and_then(|preds| preds.get(predicate))
            .map_or(0, Vec::len)
    }

    fn predicate_iris(&self, node: &String) -> Vec<String> {
        self.outgoing
            .get(node)
            .map(|preds| preds.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn iri_object_count(&self, node: &String, predicate: &str) -> usize {
        self.outgoing
            .get(node)
            .and_then(|preds| preds.get(predicate))
            .map_or(0, |terms| {
                terms
                    .iter()
                    .filter(|t| matches!(t, RdfTerm::Iri(_)))
                    .count()
            })
    }

    fn get_single_object(&self, subject: &Self::Node, predicate: &str) -> Option<Self::Node> {
        self.outgoing
            .get(subject)?
            .get(predicate)?
            .first()
            .and_then(|term| match term {
                RdfTerm::Iri(i) => Some(i.clone()),
                _ => None,
            })
    }

    fn get_datatype_literal(
        &self,
        subject: &Self::Node,
        predicate: &str,
    ) -> Option<(String, Option<String>)> {
        self.outgoing
            .get(subject)?
            .get(predicate)?
            .first()
            .and_then(|term| match term {
                RdfTerm::Literal { lexical, datatype } => Some((lexical.clone(), datatype.clone())),
                _ => None,
            })
    }

    fn get_iri(&self, node: &Self::Node) -> Option<String> {
        Some(node.clone())
    }
}


// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Embedded execution engine bridging PurRDF's AST compiler with `frozen-duckdb`.
//!
//! Evaluates compiled relational queries inside an embedded, memory-bounded DuckDB instance,
//! registers zero-copy 5-table Parquet views, enforces candidate envelopes (<= 64 rows),
//! and projects tabular results into interned RDF statements.

use std::fmt;
use std::path::Path;
use std::time::Instant;

use purrdf_core::TermValue;
use purrdf_sql::{CompiledQuery, SqlLiteral};

/// Default maximum number of result rows an evaluation materializes (Candidate Envelope Bound).
pub const DEFAULT_ROW_ENVELOPE: usize = 64;

/// ResourceLimit name carried by [`EvaluatorError::RowEnvelopeExceeded`].
pub const ROW_ENVELOPE_LIMIT: &str = "duckdb_result_rows";

/// Projected RDF triple from a tabular relational row.
#[derive(Debug, Clone, PartialEq)]
pub struct RdfTriple {
    pub subject: String,
    pub predicate: String,
    pub object: TermValue,
}

/// The result of evaluating a relational query.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionResult {
    /// Total number of rows emitted.
    pub rows_emitted: usize,
    /// Projected RDF statements.
    pub triples: Vec<RdfTriple>,
    /// Execution duration in nanoseconds.
    pub elapsed_ns: u64,
}

/// Typed evaluator errors.
#[derive(Debug)]
pub enum EvaluatorError {
    /// Underlying DuckDB error.
    DuckDb(frozen_duckdb::DuckDBError),
    /// The candidate envelope bound was exceeded during materialization.
    RowEnvelopeExceeded {
        limit: &'static str,
        observed: usize,
        max: usize,
    },
    /// Path conversion or IO error.
    PathError(String),
}

impl fmt::Display for EvaluatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuckDb(e) => write!(f, "duckdb execution error: {e}"),
            Self::RowEnvelopeExceeded { limit, observed, max } => write!(
                f,
                "candidate envelope exceeded for {limit}: observed {observed} rows, ceiling is {max}"
            ),
            Self::PathError(msg) => write!(f, "path error: {msg}"),
        }
    }
}

impl std::error::Error for EvaluatorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DuckDb(e) => Some(e),
            _ => None,
        }
    }
}

impl From<frozen_duckdb::DuckDBError> for EvaluatorError {
    fn from(e: frozen_duckdb::DuckDBError) -> Self {
        Self::DuckDb(e)
    }
}

impl purrdf_hash::error::StructuredError for EvaluatorError {
    fn visit<'error>(&'error self, visitor: &mut dyn purrdf_hash::error::ErrorVisitor<'error>) {
        match self {
            Self::DuckDb(e) => {
                visitor.variant("duckdb-error");
                let msg = match e {
                    frozen_duckdb::DuckDBError::QueryReturnedNoRows => "query returned no rows",
                    _ => "duckdb execution error",
                };
                visitor.value(
                    Some("message"),
                    purrdf_hash::error::ErrorValue::Message(msg),
                );
            }
            Self::RowEnvelopeExceeded { limit, observed, max } => {
                visitor.variant("duckdb-row-envelope-exceeded");
                visitor.value(
                    Some("limit"),
                    purrdf_hash::error::ErrorValue::Code(limit),
                );
                visitor.value(
                    Some("observed"),
                    purrdf_hash::error::ErrorValue::Unsigned(*observed as u128),
                );
                visitor.value(
                    Some("max"),
                    purrdf_hash::error::ErrorValue::Unsigned(*max as u128),
                );
            }
            Self::PathError(msg) => {
                visitor.variant("duckdb-path-error");
                visitor.value(
                    Some("message"),
                    purrdf_hash::error::ErrorValue::Message(msg.as_str()),
                );
            }
        }
    }
}

/// An embedded in-memory DuckDB query engine for PurRDF.
#[derive(Debug)]
pub struct PurRdfDuckDB {
    conn: frozen_duckdb::Connection,
}

impl PurRdfDuckDB {
    /// Opens an in-memory execution instance with configured memory ceilings (Wasm memory safety).
    pub fn open_in_memory() -> Result<Self, EvaluatorError> {
        let conn = frozen_duckdb::Connection::open_in_memory()?;
        // Configure memory ceiling to prevent starving linear memory in Wasm sandboxes
        let _ = conn.execute_batch("PRAGMA max_memory = '10MB';");
        Ok(Self { conn })
    }

    /// Access the underlying DuckDB connection.
    pub fn raw_connection(&self) -> &frozen_duckdb::Connection {
        &self.conn
    }

    /// Registers a single Parquet file as a virtual table view.
    pub fn register_parquet_view(
        &self,
        table_name: &str,
        parquet_path: &str,
        glob: bool,
    ) -> Result<(), EvaluatorError> {
        let sql = if glob {
            format!(
                "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_parquet(['{}'])",
                table_name, parquet_path
            )
        } else {
            format!(
                "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_parquet('{}')",
                table_name, parquet_path
            )
        };
        self.conn.execute_batch(&sql)?;
        Ok(())
    }

    /// Registers the canonical 5-table PurRDF Parquet views (`terms`, `quads`, `reifiers`, `annotations`, `blobs`).
    pub fn register_purrdf_parquet_views(
        &self,
        directory: &Path,
    ) -> Result<(), EvaluatorError> {
        for table in purrdf_columnar::Table::ALL {
            let file_path = directory.join(table.file_name());
            let path_str = file_path
                .to_str()
                .ok_or_else(|| EvaluatorError::PathError("invalid unicode in path".into()))?;
            self.register_parquet_view(table.name(), path_str, false)?;
        }
        Ok(())
    }

    /// Evaluates a compiled AST query with the default candidate envelope (<= 64 rows).
    pub fn eval_ast_query(
        &self,
        query_iri: &str,
        compiled: &CompiledQuery,
        subject_iri_prefix: &str,
        predicate_iri_prefix: &str,
    ) -> Result<ExecutionResult, EvaluatorError> {
        self.eval_ast_query_bounded(
            query_iri,
            compiled,
            subject_iri_prefix,
            predicate_iri_prefix,
            DEFAULT_ROW_ENVELOPE,
        )
    }

    /// Evaluates a compiled AST query with a caller-specified row envelope.
    pub fn eval_ast_query_bounded(
        &self,
        _query_iri: &str,
        compiled: &CompiledQuery,
        subject_iri_prefix: &str,
        predicate_iri_prefix: &str,
        max_rows: usize,
    ) -> Result<ExecutionResult, EvaluatorError> {
        let start = Instant::now();

        let mut stmt = self.conn.prepare(&compiled.sql)?;
        let duck_params: Vec<Box<dyn frozen_duckdb::ToSql>> = compiled
            .params
            .iter()
            .map(|p| -> Box<dyn frozen_duckdb::ToSql> {
                match p {
                    SqlLiteral::String(s) => Box::new(s.clone()),
                    SqlLiteral::Integer(i) => Box::new(*i),
                    SqlLiteral::Float(f) => Box::new(*f),
                    SqlLiteral::Boolean(b) => Box::new(*b),
                    SqlLiteral::Null => Box::new(Option::<String>::None),
                }
            })
            .collect();

        let param_refs: Vec<&dyn frozen_duckdb::ToSql> =
            duck_params.iter().map(|b| b.as_ref()).collect();

        let mut rows = stmt.query(param_refs.as_slice())?;
        let col_count = rows.as_ref().map(|r| r.column_count()).unwrap_or(0);
        let column_names: Vec<String> = (0..col_count)
            .map(|i| {
                rows.as_ref()
                    .and_then(|r| r.column_name(i).ok().map(|s| s.to_string()))
                    .unwrap_or_else(|| format!("col_{i}"))
            })
            .collect();

        let mut result_triples = Vec::new();
        let mut row_index = 0;

        while let Some(row) = rows.next()? {
            row_index += 1;
            // Candidate Envelope Bound circuit breaker: refuses mid-stream if exceeding envelope
            if row_index > max_rows {
                return Err(EvaluatorError::RowEnvelopeExceeded {
                    limit: ROW_ENVELOPE_LIMIT,
                    observed: row_index,
                    max: max_rows,
                });
            }

            let row_subject = format!("{}/row_{}", subject_iri_prefix, row_index);

            for (col_idx, col_name) in column_names.iter().enumerate() {
                let predicate = format!("{}/{}", predicate_iri_prefix, col_name);

                let term: Option<TermValue> = match row.get_ref(col_idx) {
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Null) => None,
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Boolean(b)) => {
                        Some(TermValue::Literal {
                            lexical_form: b.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#boolean".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::TinyInt(i)) => {
                        Some(TermValue::Literal {
                            lexical_form: i.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#integer".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::SmallInt(i)) => {
                        Some(TermValue::Literal {
                            lexical_form: i.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#integer".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Int(i)) => {
                        Some(TermValue::Literal {
                            lexical_form: i.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#integer".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::BigInt(i)) => {
                        Some(TermValue::Literal {
                            lexical_form: i.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#integer".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::HugeInt(i)) => {
                        Some(TermValue::Literal {
                            lexical_form: i.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#integer".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Float(f)) => {
                        Some(TermValue::Literal {
                            lexical_form: f.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#float".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Double(d)) => {
                        Some(TermValue::Literal {
                            lexical_form: d.to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#double".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Text(bytes)) => {
                        let text = String::from_utf8_lossy(bytes).into_owned();
                        Some(TermValue::Literal {
                            lexical_form: text,
                            datatype: "http://www.w3.org/2001/XMLSchema#string".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Timestamp(unit, val)) => {
                        Some(TermValue::Literal {
                            lexical_form: format!("{val} ({unit:?})"),
                            datatype: "http://www.w3.org/2001/XMLSchema#dateTime".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    _ => None,
                };

                if let Some(object) = term {
                    result_triples.push(RdfTriple {
                        subject: row_subject.clone(),
                        predicate,
                        object,
                    });
                }
            }
        }

        Ok(ExecutionResult {
            rows_emitted: row_index,
            triples: result_triples,
            elapsed_ns: start.elapsed().as_nanos() as u64,
        })
    }
}

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

use frozen_duckdb::duckdb::types::TimeUnit;
use purrdf_core::TermValue;
use purrdf_sql::{CompiledQuery, SqlLiteral};
use purrdf_xsd::numeric::{xpath_double_text, xpath_float_text};
use purrdf_xsd::temporal::{civil_from_days, datetime_from_unix_seconds};

/// Default maximum number of result rows an evaluation materializes (Candidate Envelope Bound).
pub const DEFAULT_ROW_ENVELOPE: usize = 64;

/// ResourceLimit name carried by [`EvaluatorError::RowEnvelopeExceeded`].
pub const ROW_ENVELOPE_LIMIT: &str = "duckdb_result_rows";

/// Renders a DuckDB timestamp as a canonical `xsd:dateTime` lexical form,
/// honoring the value's time unit instead of Debug-printing it.
fn timestamp_lexical(unit: TimeUnit, value: i64) -> String {
    let (secs, nanos) = match unit {
        TimeUnit::Second => (value, 0),
        TimeUnit::Millisecond => (value.div_euclid(1_000), value.rem_euclid(1_000) * 1_000_000),
        TimeUnit::Microsecond => (
            value.div_euclid(1_000_000),
            value.rem_euclid(1_000_000) * 1_000,
        ),
        TimeUnit::Nanosecond => (
            value.div_euclid(1_000_000_000),
            value.rem_euclid(1_000_000_000),
        ),
    };
    let base = datetime_from_unix_seconds(secs).canonical_lexical();
    if nanos == 0 {
        return base;
    }
    let frac = format!("{nanos:09}");
    let frac = frac.trim_end_matches('0');
    match base.strip_suffix('Z') {
        Some(stripped) => format!("{stripped}.{frac}Z"),
        None => format!("{base}.{frac}"),
    }
}

/// Renders a DuckDB date (days since 1970-01-01) as a canonical `xsd:date`
/// lexical form via Howard Hinnant's civil-from-days algorithm.
fn date_lexical(days: i64) -> String {
    let (y, m, d) = civil_from_days(i128::from(days));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Projected RDF triple from a tabular relational row.
#[derive(Debug, Clone, PartialEq)]
pub struct RdfTriple {
    /// Projected subject IRI (or blank-node label) of the triple.
    pub subject: String,
    /// Projected predicate IRI of the triple.
    pub predicate: String,
    /// Projected object term of the triple.
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
    /// Cells skipped because the value was SQL NULL.
    pub null_cells: usize,
    /// Cells skipped because the value had no representable RDF term.
    pub unrepresentable_cells: usize,
}

/// Typed evaluator errors.
#[derive(Debug)]
pub enum EvaluatorError {
    /// Underlying DuckDB error.
    DuckDb(frozen_duckdb::DuckDBError),
    /// The candidate envelope bound was exceeded during materialization.
    RowEnvelopeExceeded {
        /// Resource-limit name that was exceeded (always [`ROW_ENVELOPE_LIMIT`]).
        limit: &'static str,
        /// Number of rows actually materialized when the bound tripped.
        observed: usize,
        /// Candidate envelope ceiling that was exceeded.
        max: usize,
    },
    /// Path conversion or IO error.
    PathError(String),
    /// The projected row carried a duplicated column name, making per-column
    /// predicate IRIs ambiguous.
    DuplicateColumn {
        /// The duplicated column name.
        column: String,
    },
}

impl fmt::Display for EvaluatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuckDb(e) => write!(f, "duckdb execution error: {e}"),
            Self::RowEnvelopeExceeded {
                limit,
                observed,
                max,
            } => write!(
                f,
                "candidate envelope exceeded for {limit}: observed {observed} rows, ceiling is {max}"
            ),
            Self::PathError(msg) => write!(f, "path error: {msg}"),
            Self::DuplicateColumn { column } => {
                write!(f, "duplicate column in projection: {column}")
            }
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
            Self::RowEnvelopeExceeded {
                limit,
                observed,
                max,
            } => {
                visitor.variant("duckdb-row-envelope-exceeded");
                visitor.value(Some("limit"), purrdf_hash::error::ErrorValue::Code(limit));
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
            Self::DuplicateColumn { column } => {
                visitor.variant("duckdb-duplicate-column");
                visitor.value(
                    Some("column"),
                    purrdf_hash::error::ErrorValue::Message(column.as_str()),
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
    pub fn register_purrdf_parquet_views(&self, directory: &Path) -> Result<(), EvaluatorError> {
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

        let mut seen = std::collections::BTreeSet::<&str>::new();
        for name in &column_names {
            if !seen.insert(name.as_str()) {
                return Err(EvaluatorError::DuplicateColumn {
                    column: name.clone(),
                });
            }
        }

        let mut result_triples = Vec::new();
        let mut row_index = 0;
        let mut null_cells = 0usize;
        let mut unrepresentable_cells = 0usize;

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
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Null) => {
                        null_cells += 1;
                        None
                    }
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
                            lexical_form: xpath_float_text(f).as_str().to_string(),
                            datatype: "http://www.w3.org/2001/XMLSchema#float".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Double(d)) => {
                        Some(TermValue::Literal {
                            lexical_form: xpath_double_text(d).as_str().to_string(),
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
                            lexical_form: timestamp_lexical(unit, val),
                            datatype: "http://www.w3.org/2001/XMLSchema#dateTime".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Date32(days)) => {
                        Some(TermValue::Literal {
                            lexical_form: date_lexical(i64::from(days)),
                            datatype: "http://www.w3.org/2001/XMLSchema#date".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    Ok(frozen_duckdb::duckdb::types::ValueRef::Blob(bytes)) => {
                        Some(TermValue::Literal {
                            lexical_form: purrdf_xsd::canonical_hex(bytes),
                            datatype: "http://www.w3.org/2001/XMLSchema#hexBinary".to_string(),
                            language: None,
                            direction: None,
                        })
                    }
                    // Every remaining structural value (Interval, List, Struct,
                    // Decimal, ...) has no faithful xsd mapping on this path
                    // yet: counted, never silently dropped.
                    Ok(_) => {
                        unrepresentable_cells += 1;
                        None
                    }
                    Err(_) => {
                        unrepresentable_cells += 1;
                        None
                    }
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
            null_cells,
            unrepresentable_cells,
        })
    }
}

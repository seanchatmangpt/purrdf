// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! PurRDF embedded DuckDB execution engine and relational-to-RDF row projection.

pub mod eval;

pub use eval::{
    DEFAULT_ROW_ENVELOPE, EvaluatorError, ExecutionResult, PurRdfDuckDB, ROW_ENVELOPE_LIMIT,
    RdfTriple,
};

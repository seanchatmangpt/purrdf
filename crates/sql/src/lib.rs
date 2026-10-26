// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! PurRDF declarative SQL-in-RDF AST representation, validation and deterministic compiler.
//!
//! Provides in-memory AST terms matching the `ontologies/sql-ast.ttl` ontology,
//! identifier validation eliminating structural injection, parameterized SQL serialization,
//! declarative candidate envelope pushdown (LIMIT <= 64), and recursive bounded RDF collection unrolling.

pub mod ast;
pub mod compiler;
pub mod error;
pub mod serializer;

pub use ast::*;
pub use compiler::*;
pub use error::{refusal_arm, AstError, RefusalCode};
pub use serializer::*;

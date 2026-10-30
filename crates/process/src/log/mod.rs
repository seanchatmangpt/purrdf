// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! OCEL 2.0 event logs: typed model, tolerant ingestion, validation,
//! canonical identifiers, deterministic writer, and hash-chained integrity.
//!
//! Provenance: IR shapes follow wasm4pm `models.rs` (dual OCEL 1.0/2.0
//! dialects, exact-number attributes as lexemes) + ex4pm `ocel.ex` (object
//! attribute time series, schema-tolerant key aliases); validation rules
//! follow ex4pm `validate_references/2` + `validate_envelope/1` and beam4pm
//! `Ocel.validate_envelope/2`; the integrity chain follows beam4pm
//! `zcode_ocel_verifier.ex` with BLAKE3 in place of sha256; the writer's
//! fixed key order follows ex4pm `ocel_writer.ex`.

pub mod columnar;
pub mod integrity;
pub mod model;
pub mod parse;
pub mod validate;
pub mod write;

pub use columnar::{LogTables, ValueKind, to_batches};
pub use model::{
    AttrValue, EventObjectRef, ObjectAttribute, ObjectChange, OcelEvent, OcelLog, OcelObject,
    OcelType, OcelTypeAttribute, ValueAtTime,
};
pub use parse::{MAX_INPUT_BYTES, from_json_str, from_ndjson_str};
pub use validate::validate;
pub use write::to_json_string;

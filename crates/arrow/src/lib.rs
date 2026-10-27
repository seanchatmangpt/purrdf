// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! PurRDF Canonical Arrow Columnar Egress Protocol.
//!
//! Provides zero-dependency, cache-line aligned (64-byte aligned) 128-byte wire headers
//! and binary serializers for streaming query results into Arrow IPC buffers.

pub mod header;

pub use header::{ArrowEgressHeader, ArrowEgressHeaderError, canon_arrow_empty_v1};

// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! PurRDF CanonArrow columnar egress framing.
//!
//! Provides a self-describing, cache-line aligned (64-byte aligned) 128-byte wire
//! header and binary serializers, designed for downstream Arrow IPC encapsulation.
//! This crate does **not** implement the Arrow IPC container formats: the framing is
//! a proprietary CanonArrow protocol (no flatbuffers, no `ARROW1` magic, no
//! encapsulation prefix), not readable by Arrow IPC implementations.
//!
//! Single dependency (`purrdf-hash`); no third-party crates.
//!
//! Note: `FLAG_BIG_ENDIAN` is declared in [`header::ArrowEgressHeader`] but is not
//! interpreted by `to_bytes`/`from_bytes`; byte order handling is the caller's
//! responsibility.

pub mod header;

pub use header::{ArrowEgressHeader, ArrowEgressHeaderError, canon_arrow_empty_v1};

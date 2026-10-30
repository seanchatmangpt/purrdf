// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The conformance turnstile.
//!
//! Provenance: token replay follows wasm4pm `conformance.rs` (bitmask fast
//! path, minimal invisible-enable BFS) and beam4pm `art72_conformance.ex`
//! (underfed-fire accounting + the van der Aalst fitness formula);
//! order-preserving shuffle membership follows ex4pm `shuffle.ex` semantics
//! with bcinr `scheduler_v2.rs`'s pred-mask readiness gating; alignment
//! follows wasm4pm `alignments.rs` (h=0 Dijkstra, integer costs,
//! deterministic expansion).

pub mod alignment;
pub mod replay;
pub mod shuffle;

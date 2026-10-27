// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Cache-friendly, branchless 1-based Eytzinger search layout and traversal.
//!
//! Single home: [`purrdf_branchless::eytzinger`] owns the implementation
//! (layout builder, branchless search, subtree-size lower bound, layout
//! permutation, generic-key `search_by`); this module re-exports it so
//! existing `purrdf_core::eytzinger` paths keep resolving.

pub use purrdf_branchless::eytzinger::*;

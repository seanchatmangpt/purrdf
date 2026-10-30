// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Process models: the POWL 2.0 AST, workflow nets, and decomposition.

pub mod decompose;
pub mod petri;
pub mod powl;

pub use petri::{PetriNet, SoundnessReport, Transition};
pub use powl::{Endpoint, Limits, Powl};

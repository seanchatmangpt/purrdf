// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Planning synthesis over PurRDF primitives.
//!
//! Provenance: the FOND domain/policy/fixpoint contracts, typed refusals,
//! and counterexample classes follow ash_pplan `fond.ex`/`synthesis.ex`/
//! `counterexample.ex`/`policy_switch.ex`; the HTN network type, its
//! validation refusals, and the POWL bridge follow beam4pm `hddl_powl.ex`;
//! receipts and the standing ladder follow ash_pplan
//! `execution_receipt.ex` + `standing/ladder.ex` (BLAKE3 in place of the
//! donors' sha256 hex).

pub mod fond;
pub mod htn;
pub mod receipt;

pub use fond::{CounterexampleClass, FondDomain, FondPolicy, Mode, classify, policy_switch};
pub use htn::{HtnNetwork, HtnWorkflow, network_of, workflow_to_powl};
pub use receipt::{ExecutionReceipt, RunStatus, Standing};

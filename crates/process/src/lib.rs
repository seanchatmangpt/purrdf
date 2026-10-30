// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Process mining and planning over PurRDF's branchless columnar primitives.
//!
//! One engine, four modules, each the consolidation of the surveyed process
//! lineages ([`powl 2.0`], [`ocel 2.0`], HTN/FOND) into deterministic Rust:
//!
//! | Module | Capability | Primary donors |
//! |---|---|---|
//! | [`model`] | POWL 2.0 AST with bounded language semantics, workflow nets
//! with structural validation and soundness, and top-down SESE-style
//! decomposition (PartitionMG/PartitionSM) | ex4pm `wf_net_to_powl.ex`
//! (PETRI25 Algorithms 1-3), smalt `Powl2.Compiler`, wasm4pm `powl_arena` |
//! | [`log`] | OCEL 2.0 event logs: schema-tolerant JSON/NDJSON ingestion
//! (dual 1.0/2.0 dialects), referential validation, canonical identifiers,
//! deterministic writer, hash-chained integrity | ex4pm `lib/ex4pm/ocel.ex` +
//! `ocel_writer.ex`, wasm4pm `models.rs`, beam4pm `zcode_ocel_verifier.ex` |
//! | [`synth`] | HTN workflows with dependency-ordered POWL projection, FOND
//! policy validation/synthesis (strong + strong-cyclic fixpoints), typed
//! refusals, execution receipts | ash_pplan `fond.ex`/`synthesis.ex` +
//! `counterexample.ex`, beam4pm `hddl_powl.ex` |
//! | [`conformance`] | Token replay (pm4py-convention fitness, bitmask fast
//! path), polynomial order-preserving shuffle membership, deterministic A\*
//! alignment | wasm4pm `conformance.rs`, ex4pm `alignment.ex` +
//! `shuffle.ex`, bcinr `scheduler_v2.rs` readiness masks |
//!
//! # Boundaries (deliberate)
//!
//! - POWL → Petri-net compilation is deferred: conformance runs against
//!   [`petri::PetriNet`] values built directly. The lattice of conversions
//!   lands after CI confirms this crate compiles.
//! - Language enumeration is bounded and refuses loudly
//!   ([`ProcessError::LanguageBudgetExceeded`]) rather than exploring
//!   unbounded products.
//! - [`AttrValue`] keeps numbers as exact lexemes
//!   ([`purrdf_lex::json::Number`]) — no float rounding enters a log, a
//!   digest, or a comparison.
//!
//! # Determinism contract
//!
//! Every output path is deterministic: maps on serialized surfaces are
//! [`BTreeMap`], identifiers are BLAKE3 digests over canonical key-ordered
//! JSON written by `purrdf_lex::json`, iteration follows sorted or document
//! order, and there is no clock, RNG, or thread anywhere in the crate. Hot
//! membership sets use `purrdf_core::FastMap`/`FastSet` (fixed-key hasher,
//! never iterated for output).
//!
//! # Provenance
//!
//! Surveyed 2026-10-29 across `ex4pm` (26.10.17), `ash_ex4pm`, `beam4pm`,
//! `ash_pplan`, `wasm4pm`, `smalt`, and `bcinr`; per-kernel provenance lives
//! in each module's header. Digests use BLAKE3 + `purrdf_hash::hex` in place
//! of the donors' ad-hoc sha256 hex.
//!
//! [`powl 2.0`]: https://arxiv.org/abs/2505.07052
//! [`ocel 2.0`]: https://ocel-standard.org

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod conformance;
pub mod error;
pub mod log;
pub mod model;
pub mod synth;

pub use conformance::alignment::{AlignmentCosts, AlignmentResult, Move};
pub use conformance::replay::{BitmaskNet, LogReplay, TraceReplay, replay_log, replay_trace};
pub use conformance::shuffle::{ShuffleSpec, ShuffleVerdict};
pub use error::ProcessError;
pub use log::model::{
    AttrValue, EventObjectRef, ObjectAttribute, ObjectChange, ObjectRelationship, OcelEvent,
    OcelLog, OcelObject, OcelType, OcelTypeAttribute, ValueAtTime,
};
pub use model::decompose::decompose;
pub use model::petri::PetriNet;
pub use model::powl::{Endpoint, Limits, Powl};
pub use synth::htn::{HtnNetwork, HtnTaskDef, HtnWorkflow, workflow_to_powl};
pub use synth::{
    CounterexampleClass, ExecutionReceipt, FondDomain, FondPolicy, Mode, RunStatus, Standing,
    classify, policy_switch,
};

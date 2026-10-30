// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Execution receipts and the standing ladder.
//!
//! Provenance: fields follow ash_pplan `execution_receipt.ex` (plan/run
//! identity, status, outcome digest, subject digests); the ladder follows
//! ash_pplan `standing/ladder.ex` (ten monotone states).

use purrdf_hash::{blake3, hex};
use purrdf_lex::json::{Object as Json, Value, write_compact};

/// One plan run's receipt.
#[derive(Clone, Debug)]
pub struct ExecutionReceipt {
    /// The plan's id.
    pub plan_id: String,
    /// The run's id.
    pub run_id: String,
    /// Terminal status.
    pub status: RunStatus,
    /// Duration as data (microseconds); this crate never reads a clock.
    pub duration_us: u64,
    /// BLAKE3 short digest of the canonical outcome JSON.
    pub outcome_digest: String,
    /// The subject digest the run enacted.
    pub subject_digest: String,
    /// The base digest it was planned against.
    pub base_digest: String,
    /// The steps claimed to correspond to this run.
    pub corresponds_to_steps: Vec<String>,
}

/// Terminal run status (ash_pplan's four states).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    /// Completed successfully.
    Succeeded,
    /// Halted by policy.
    Halted,
    /// Failed.
    Failed,
    /// Not yet known.
    Unknown,
}

/// The standing ladder (ash_pplan `ladder.ex`'s ten monotone states).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    /// No observation.
    Unknown,
    /// Observed once.
    Observed,
    /// Validated.
    Validated,
    /// Derived.
    Derived,
    /// A candidate.
    Candidate,
    /// Experimentally supported.
    ExperimentallySupported,
    /// Admitted.
    Admitted,
    /// Manufactured.
    Manufactured,
    /// Actuated with a receipt.
    Actuated,
    /// Verified.
    Verified,
}

impl Standing {
    /// Whether `self` may transition to `next` (monotone ladder: forward
    /// only).
    #[must_use]
    pub fn can_transition_to(self, next: Standing) -> bool {
        next >= self
    }
}

impl ExecutionReceipt {
    /// Mint a receipt over canonical outcome data.
    #[must_use]
    pub fn mint(
        plan_id: impl Into<String>,
        run_id: impl Into<String>,
        status: RunStatus,
        duration_us: u64,
        outcome: &Value,
        subject_digest: &str,
        base_digest: &str,
        corresponds_to_steps: Vec<String>,
    ) -> Self {
        let mut record = Json::new();
        record.push("status", Value::from(status.code()));
        record.push("outcome", outcome.clone());
        let digest = blake3::hash(write_compact(&Value::Object(record)).as_bytes());
        let full = hex::encode(digest.as_bytes());
        let outcome_digest: String = full.chars().take(16).collect();
        Self {
            plan_id: plan_id.into(),
            run_id: run_id.into(),
            status,
            duration_us,
            outcome_digest,
            subject_digest: subject_digest.to_owned(),
            base_digest: base_digest.to_owned(),
            corresponds_to_steps,
        }
    }
}

impl RunStatus {
    /// The stable status code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Halted => "halted",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

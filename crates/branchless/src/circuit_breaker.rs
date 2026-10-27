//! Branchless state latches and fixed-budget step limiters.
//!
//! Ported from BranchlessCInRust (`bcinr`): mask predicates from
//! `autonomic/policy_guard.rs`, the fixed-budget fiber shape from
//! `patterns/wcet_fiber.rs` + `abstractions/resumable_fiber.rs`, and the
//! done-mask/tick + typed-outcome scheduler shape from
//! `bcinr-powl/src/scheduler_v2.rs` (reimplemented `no_std` under honest
//! names). There is no dedicated latch/tripwire module upstream — the role is
//! filled by these composable mask/saturating primitives.
//!
//! Alignment with purrdf OPT-04: the [`latch_stop`] shape maps directly onto
//! the planned `StopSignal` → `AtomicU64` design — bit 0 is the stop-latch,
//! bits 1..2 encode the cause, and the hot-loop test is one load plus one ALU
//! op (`stop_bits & 1 != 0`) with no vtable hop.

/// General bool → all-ones/zero select mask.
///
/// The canonical form every mask predicate in this module reduces to:
/// `0u64.wrapping_sub(b as u64)` converts a boolean into a full mask with no
/// branch (`false → 0`, `true → u64::MAX`). `saturation::CanonicalMask` and
/// `simd_fallback`'s `ct_*` predicates are the same bool→mask idea at other
/// altitudes (signed widths, ct-compare forms).
#[must_use]
#[inline]
pub const fn mask_of(b: bool) -> u64 {
    0u64.wrapping_sub(b as u64)
}

/// Returns `u64::MAX` if `val > threshold`, else `0`. All-ones/0 select mask.
// from bcinr crates/bcinr-logic/src/autonomic/policy_guard.rs:16
#[must_use]
#[inline]
pub fn mask_gt(val: u64, threshold: u64) -> u64 {
    mask_of(val > threshold)
}

/// Returns `u64::MAX` if `val < threshold`, else `0`.
// from bcinr crates/bcinr-logic/src/autonomic/policy_guard.rs:30
#[must_use]
#[inline]
pub fn mask_lt(val: u64, threshold: u64) -> u64 {
    mask_of(val < threshold)
}

/// Returns `u64::MAX` if `val == threshold`, else `0`.
// from bcinr crates/bcinr-logic/src/autonomic/policy_guard.rs:38
#[must_use]
#[inline]
pub fn mask_eq(val: u64, threshold: u64) -> u64 {
    mask_of(val == threshold)
}

/// Branchless stop latch: bit 0 is the stop bit, bits 1..2 the cause.
///
/// Setting a cause while the latch is clear raises bit 0 and records the
/// cause; once latched, the bits never change. The hot-path check is
/// `latch_stop(bits)` — one mask test, no branch on the latch state.
#[must_use]
#[inline]
pub fn latch_stop(bits: u64, cause: StopCause) -> u64 {
    let cause_bits = (cause as u64) << 1;
    let fresh = 1 | cause_bits;
    let already = bits & 1;
    // Mask-select: keep existing bits when latched, write fresh bits when clear.
    bits | (fresh & 0u64.wrapping_sub(already ^ 1))
}

/// Stop cause encoded in bits 1..2 of the latch word (OPT-04 layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    /// No cause recorded (latch clear).
    None = 0,
    /// Cancelled by an external signal.
    Cancelled = 1,
    /// Wall-clock deadline elapsed.
    Deadline = 2,
}

impl StopCause {
    /// Decodes the cause bits from a latch word (masked, so any word decodes).
    #[must_use]
    #[inline]
    pub fn from_bits(bits: u64) -> Self {
        match (bits >> 1) & 0b11 {
            1 => Self::Cancelled,
            2 => Self::Deadline,
            _ => Self::None,
        }
    }
}

/// Minimal resumable fiber state: advances by `event & 0xFF` when the event is
/// non-zero, holds otherwise. Returns `(new_state, all-ones/0 success mask)`.
// from bcinr crates/bcinr-logic/src/abstractions/resumable_fiber.rs:13
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FiberState {
    /// Current accumulated state word.
    pub state: u32,
}

impl FiberState {
    /// A fresh fiber at state 0.
    #[must_use]
    pub const fn new() -> Self {
        Self { state: 0 }
    }

    /// Advances branchlessly; returns `(new_state, success_mask)`.
    #[must_use]
    #[inline]
    pub fn advance(&mut self, event: u32) -> (u32, u32) {
        let old = self.state;
        let next = old.wrapping_add(event & 0xFF);
        let success_mask = 0u32.wrapping_sub(u32::from(event != 0));
        self.state = (next & success_mask) | (old & !success_mask);
        (self.state, success_mask)
    }
}

/// Fixed-budget fiber: advances exactly `TICKS` times per call regardless of
/// outcomes, accumulating a per-tick success bitmask.
///
/// Contract (from the bcinr WCET contract): execution is O(TICKS) no matter
/// what the events are; `TICKS` must be ≤ 64 so each tick owns one distinct
/// result bit; zero heap; constant-shape instruction-pointer updates
/// (`ip += mask & 1`).
// from bcinr crates/bcinr-logic/src/patterns/wcet_fiber.rs:28
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WcetFiber<const TICKS: usize> {
    /// The running fiber state.
    pub state: FiberState,
    /// Number of successful ticks so far (constant-shape updates only).
    pub instruction_pointer: usize,
}

impl<const TICKS: usize> Default for WcetFiber<TICKS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const TICKS: usize> WcetFiber<TICKS> {
    /// A fresh fiber at state 0, pointer 0.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: FiberState::new(),
            instruction_pointer: 0,
        }
    }

    /// Advances by exactly `TICKS` steps; bit `i` of the result is set iff
    /// tick `i` succeeded.
    ///
    /// # Panics
    /// Panics when `TICKS > 64` (the result bitmask cannot hold the ticks).
    #[must_use]
    #[inline]
    pub fn execute_budget_fixed(&mut self, events: &[u32; TICKS]) -> u64 {
        const {
            assert!(TICKS <= 64, "TICKS must be <= 64 for the success bitmask");
        }
        let mut success_mask = 0u64;
        let mut i = 0;
        while i < TICKS {
            let event = events[i];
            let (_, mask) = self.state.advance(event);
            success_mask |= u64::from(mask & 1) << (i & 0x3F);
            // Constant-shape update: never branch on success.
            self.instruction_pointer += (mask & 1) as usize;
            i += 1;
        }
        success_mask
    }
}

/// One bounded run: which slots are done and how many ticks elapsed.
///
/// Reimplements (honest rename, `no_std`) the shape of bcinr-powl's
/// `PowlV2RunState`: readiness = unfinished ∧ predecessors-done, everything as
/// bitmask algebra.
// from bcinr crates/bcinr-powl/src/scheduler_v2.rs:16
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BudgetRunState {
    /// Slots that have completed.
    pub done_mask: u64,
    /// Ticks executed so far.
    pub tick: u32,
}

/// Outcome of one bounded-budget tick.
// from bcinr crates/bcinr-powl/src/scheduler_v2.rs:47 (PowlV2TickOutcome)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetOutcome {
    /// At least one slot fired; carries the fired-slot mask.
    Fired(u64),
    /// Every slot had already completed.
    Complete,
    /// The budget is incomplete but nothing can fire.
    Deadlock {
        /// Slots still outstanding.
        remaining_mask: u64,
    },
}

impl BudgetRunState {
    /// A fresh run state with nothing done.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            done_mask: 0,
            tick: 0,
        }
    }

    /// True when every valid slot has completed.
    #[must_use]
    #[inline]
    pub fn is_complete(&self, valid_mask: u64) -> bool {
        self.done_mask & valid_mask == valid_mask
    }

    /// Unfinished slots whose predecessor set is complete.
    ///
    /// `pred_masks[i]` is the predecessor mask of slot `i`; slot `i` is ready
    /// iff it is unfinished and `pred_masks[i] & !done_mask == 0`.
    #[must_use]
    pub fn ready_mask(&self, pred_masks: &[u64], valid_mask: u64) -> u64 {
        let mut ready = 0u64;
        let mut index = 0;
        let len = pred_masks.len().min(64);
        while index < len {
            let bit = 1u64 << index;
            let in_scope = mask_neq_zero(bit & valid_mask);
            let unfinished = mask_eq_zero(self.done_mask & bit);
            let preds_done = mask_eq_zero(pred_masks[index] & !self.done_mask);
            ready |= bit & in_scope & unfinished & preds_done;
            index += 1;
        }
        ready
    }

    /// Executes one tick: fires `admitted` slots (caller-selected from
    /// [`BudgetRunState::ready_mask`]) and reports the typed outcome.
    pub fn tick_once(&mut self, valid_mask: u64, admitted: u64) -> BudgetOutcome {
        self.tick = self.tick.wrapping_add(1);
        let fired = admitted & valid_mask & !self.done_mask;
        self.done_mask |= fired;
        if self.is_complete(valid_mask) {
            BudgetOutcome::Complete
        } else if fired == 0 {
            BudgetOutcome::Deadlock {
                remaining_mask: valid_mask & !self.done_mask,
            }
        } else {
            BudgetOutcome::Fired(fired)
        }
    }
}

/// `u64::MAX` when `x != 0`, else `0` (mask form, no branch).
#[must_use]
#[inline]
pub fn mask_neq_zero(x: u64) -> u64 {
    mask_of(x != 0)
}

/// `u64::MAX` when `x == 0`, else `0` (mask form, no branch).
#[must_use]
#[inline]
pub fn mask_eq_zero(x: u64) -> u64 {
    mask_of(x == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent reference oracle: plain if/else.
    fn mask_gt_reference(val: u64, threshold: u64) -> u64 {
        if val > threshold { !0 } else { 0 }
    }

    #[test]
    fn mask_predicates_match_oracle_and_boundaries() {
        for &(val, threshold) in &[(10, 5), (5, 10), (5, 5), (0, 0), (u64::MAX, u64::MAX)] {
            assert_eq!(mask_gt(val, threshold), mask_gt_reference(val, threshold));
        }
        assert_eq!(mask_gt(u64::MAX, u64::MAX - 1), !0);
        assert_eq!(mask_gt(0, 0), 0); // strict: equal is not greater
        assert_eq!(mask_lt(4, 5), !0);
        assert_eq!(mask_lt(5, 5), 0);
        assert_eq!(mask_eq(5, 5), !0);
        assert_eq!(mask_eq(4, 5), 0);
    }

    // Divergence mutants: intentionally-flawed variants the tests must reject.
    fn mutant_mask_gt_inclusive(val: u64, threshold: u64) -> u64 {
        0u64.wrapping_sub(u64::from(val >= threshold))
    }
    fn mutant_mask_gt_inverted(val: u64, threshold: u64) -> u64 {
        0u64.wrapping_sub(u64::from(val < threshold))
    }

    #[test]
    fn mask_predicates_kill_flawed_mutants() {
        // mutant inclusive fires on (5,5) where the real predicate does not.
        assert_ne!(mask_gt(5, 5), mutant_mask_gt_inclusive(5, 5));
        assert_ne!(mask_gt(10, 5), mutant_mask_gt_inverted(10, 5));
    }

    #[test]
    fn latch_stop_is_sticky_and_encodes_cause() {
        let fresh = latch_stop(0, StopCause::Cancelled);
        assert_eq!(fresh & 1, 1);
        assert_eq!(StopCause::from_bits(fresh), StopCause::Cancelled);

        let latched = latch_stop(fresh, StopCause::Deadline);
        assert_eq!(latched, fresh, "latch must be sticky; cause never changes");

        let deadline = latch_stop(0, StopCause::Deadline);
        assert_eq!(StopCause::from_bits(deadline), StopCause::Deadline);
        assert_eq!(StopCause::from_bits(0), StopCause::None);
    }

    #[test]
    fn fiber_advance_holds_on_zero_event() {
        let mut fiber = FiberState::new();
        let (state, mask) = fiber.advance(0);
        assert_eq!(state, 0);
        assert_eq!(mask, 0, "zero event must not advance");
        let (state, mask) = fiber.advance(7);
        assert_eq!(state, 7);
        assert_eq!(mask, u32::MAX);
        // Masked to the low byte.
        let (state, _) = fiber.advance(0x1FF);
        assert_eq!(state, 7 + 0xFF);
    }

    #[test]
    fn wcet_budget_executes_exactly_ticks_and_masks_success() {
        fn oracle(events: &[u32]) -> u64 {
            let mut mask = 0u64;
            for (i, &e) in events.iter().enumerate() {
                if e != 0 {
                    mask |= 1 << i;
                }
            }
            mask
        }

        let mut fiber = WcetFiber::<4>::new();
        let events = [3, 0, 9, 1];
        assert_eq!(fiber.execute_budget_fixed(&events), oracle(&events));
        assert_eq!(fiber.instruction_pointer, 3);

        // TICKS = 1 boundary.
        let mut one = WcetFiber::<1>::new();
        assert_eq!(one.execute_budget_fixed(&[0]), 0);
        assert_eq!(one.execute_budget_fixed(&[5]), 1);
    }

    #[test]
    fn budget_run_state_outcomes_are_typed_and_exhaustive() {
        let mut run = BudgetRunState::new();
        // Slot 0 depends on nothing; slot 1 on slot 0; slot 2 on itself (unreachable).
        let preds = [0u64, 1, 4];
        let valid = 0b111u64;

        let ready = run.ready_mask(&preds, valid);
        assert_eq!(ready, 0b001);
        match run.tick_once(valid, ready) {
            BudgetOutcome::Fired(mask) => assert_eq!(mask, 0b001),
            other => panic!("expected Fired, got {other:?}"),
        }

        let ready = run.ready_mask(&preds, valid);
        assert_eq!(ready, 0b010);
        let _ = run.tick_once(valid, ready);

        // Slot 2 can never fire (pred = itself, unfinished) → deadlock with
        // remaining mask, then any further tick stays a deadlock until the
        // caller admits nothing.
        match run.tick_once(valid, 0) {
            BudgetOutcome::Deadlock { remaining_mask } => {
                assert_eq!(remaining_mask, 0b100);
            }
            other => panic!("expected Deadlock, got {other:?}"),
        }
        assert!(!run.is_complete(valid));

        // Complete path.
        let mut done = BudgetRunState {
            done_mask: valid,
            tick: 9,
        };
        assert!(done.is_complete(valid));
        assert!(matches!(done.tick_once(valid, 0), BudgetOutcome::Complete));
    }
}

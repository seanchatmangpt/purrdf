# [OPT-16] Saturating Arithmetic: LIMIT-Loop Counters, Cost-Loop Panic Removal, Floored-Clamp Helper

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/modifier.rs:540-556`; `crates/sparql-eval/src/bgp.rs:1458-1460,1588-1596,4030,1646`; `crates/gts/src/codec.rs:115,130`
- **Subsystem:** Relational Sieve (cost model / slice modifiers) · Memory Arena (capacity clamps)
- **Inspection Agent:** Agent 16 (Branchless Saturation Arithmetic)

Workspace context: `[profile.dev] overflow-checks = true` runs in dev AND test (`Cargo.toml`, commented "overflow checks are what keep an arithmetic bug in a byte-deterministic codec from becoming a wrong-but-green run"); clippy pedantic+nursery `-D warnings` gates merges. The codebase is already saturation-heavy by design (`saturating_add` folds, `checked_add(...).ok_or(EvalError::WorkspaceBoundOverflow)` everywhere). No `clamped_scaling_u64`/`clamp_i64` helpers exist anywhere (grep-verified); they would be new.

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  ```rust
  // modifier.rs:540-556, finish_slice — two checked increments PER ROW in the LIMIT loop:
  selected = selected.checked_add(1).ok_or(EvalError::WorkspaceBoundOverflow)?;
  position = position.checked_add(1).ok_or(EvalError::WorkspaceBoundOverflow)?;
  ```
  ```rust
  // bgp.rs:1588-1596, cost_base — panic branch per access probe:
  work = work.checked_add(u128::from(cost.work())).expect("physically representable graph work sum fits u128");
  ```
  ```rust
  // bgp.rs:1458-1460, axis_domains — branchy floor-clamp, duplicated at :4030:
  known.and_then(|values| values[axis]).map_or(fallback, |value| (value.value as f64).min(rows).max(1.0))
  ```
- **Hardware-Level Impact:** two `Result` branches per row in the slice loop (both statically non-overflowing); a panic path inside the per-pattern cost loop; duplicated clamp spellings that drift.
- **Baseline Cost:** ~2 predictable-but-live `Result` branches per LIMIT row; `expect` unwind machinery compiled into the hot planning loop.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. **F3** — `position = position.saturating_add(1); selected = selected.saturating_add(1);` — zero semantic change (position bounded by in-memory `seq.rows.len()`; selected only incremented behind `selected < limit`, so `selected ≤ limit < usize::MAX`).
  2. **F4** — `work = work.saturating_add(u128::from(cost.work()));` + `debug_assert!` monotonicity. Behavior-identical: even `u64::MAX` summed over every page of a 64-bit address space fits u128 — the invariant is impossible to violate.
  3. **F1/F2** — extract one helper beside `step_cost` (bgp.rs:1646): `fn floored_domain(value: f64, ceiling: f64) -> f64 { value.min(ceiling).max(1.0) }` — one spelling, both call sites. **CRITICAL correctness note: `f64::clamp(1.0, rows)` PANICS when `rows < 1.0` (base.rows can be 0.0 for an empty pattern). Never use bare `clamp` here.**
  4. **F7** — `codec.rs:115/130` capacity chains encode `clamped_scaling_u64` semantics (ceiling wins by op order when `limit < 4096`); rewrite via the helper, which bakes in the required `ceiling.max(floor)` guard against `clamp`'s min>max panic.
- **Hard Bounds Applied:** floor 1.0 on domain estimates (divisor protection, `join_divisor` bgp.rs:1496-1507); 32 MiB arena ceiling via `CapacityError`-style clamps; all helpers `#[inline] const`-able.
- **Proposed Type Layout & Signatures** (home: purrdf-core, re-exported to sparql-eval/retrieval/hnsw/gts; keep the overflow-checked core proven by the gate profile while saturating wrappers stay branchless):
  ```rust
  /// Saturating scale-then-clamp; `ceiling` floored so clamp's min>max panic is unreachable.
  #[inline] const fn clamped_scaling_u64(value: u64, scale: u64, floor: u64, ceiling: u64) -> u64 {
      value.saturating_mul(scale).clamp(floor, ceiling.max(floor))
  }
  #[inline] const fn clamp_i64(value: i64, lo: i64, hi: i64) -> i64 { value.clamp(lo, hi.max(lo)) }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** no `Result`-carrying increment in the slice row loop; no `expect`/panic branch in the cost loop; min/max chains compile to `minsd/maxsd` + cmov.
- **Allocation Invariant:** 0 allocations (pure arithmetic change).
- **Benchmark Target:** LIMIT/OFFSET slice path removes 2 branches/row; cost-model planning loop removes unwind machinery; verify via existing bench suite (report-only).
- **Layer Boundary:** helpers live in purrdf-core only; re-exported, never duplicated (helpers-ledger compliance).

**Explicit no-action list (do NOT convert):**
- `finish_slice` `checked_mul` for sort-key cells (modifier.rs:678), `cost_columns` `checked_add(1)` (bgp.rs:1316), DP `checked_shl` (bgp.rs:2000): the `Result`/`WorkspaceBoundOverflow` IS the contract; saturating would mask real bound bugs.
- Governor ceiling early-exits (~15 sites, e.g. modifier.rs:160-163, path.rs:1009-1012, binop.rs): control flow that terminates iteration and emits a governed truncation ending — converting changes refusal semantics.
- **BM25F `crates/text/src/ranking.rs`**: entirely `checked_*` fixed-point with explicit saturation ceiling; converting breaks the byte-exact `purrdf-bm25f-fixed-v2` scoring profile. NEVER touch.
- F5 RRF sign branch (`retrieval/src/reciprocal_rank.rs:187-189`): low priority, report-only (signs don't mix for rank contributions; branch is cheap).
- Exemplars already correct (the model to copy): `governor/mod.rs:1475` `headroom / cost.max(1)` + `committed.saturating_add`; governor ledger saturating folds; `paged/query.rs:938-956` per-page cardinality fold; `bgp.rs:1658-1683` base cardinality sums.

Over-refusal doctrine: clamp-never-refuse is the house mitigation for cost estimators (`docs/design/purrdf-embedding-knn.md:246` — "Over-refusal from a cost estimator is a live defect class in this repository"; `hnsw/src/relation.rs:878-882` doc — clamp, never refuse, so the planner never requests unreachable rows).

**Implemented in `purrdf::branchless::{saturation, scaling}`** (crate `crates/branchless`, ported
from BranchlessCInRust): `mul_sat_u64` u128 wide-mul mask-select, `div_sat_u64`, `clamp_i64/u64`,
mask-form saturating i64 ops, mask-select kernels, Q16 `fp_mul/fp_div` wide-mul scaling, and
`muldiv` — the shared home these sites re-export instead of duplicating.

**Addendum (OPT-42 scan, 2026-10-26): Q16 toolkit is a landed prerequisite + consumer pair.**
The Q16 fixed-point surface in `crates/branchless/src/scaling.rs` is ALIVE and is both a
prerequisite and the sanctioned consumer home for this ticket's helper work:
- `pub const fn fp_mul_q16(a: u64, b: u64) -> u64` (scaling.rs:16) — Q16 wide-mul
  multiply, `(a * b) >> 16` with overflow masked.
- `pub const fn fp_div_q16(a: u64, b: u64) -> u64` (scaling.rs:26) — Q16 wide-mul
  divide.
- `pub const fn muldiv(a: u64, b: u64, c: u64) -> u64` (scaling.rs:117) — the
  saturating `a*b/c` this ticket's F7 sites need; quotient-overflow contract
  documented at the definition (OPT-42 scan item 3, muldiv contract note).
Adjacent `scaling.rs` surface the cost-model sites may consume directly instead of
new locals: `modular_mul_u64` (:40), `norm_u32` (:54), `smoothstep_u32` (:82),
`dequantize_u32` (:95).

Rule for implementers: F1/F2 `floored_domain` and F7 `clamped_scaling_u64` (ticket
§3) live **beside** this Q16 family in the single `scaling` home — no re-derivation
of wide-mul or clamp-guard logic in `sparql-eval`/`gts`. F3/F4 `saturating_add`
call-site rewrites consume `purrdf::branchless::saturation` masks, not new
arithmetic. Any new `clamped_scaling_u64` body in `purrdf-core` must delegate to
`branchless::scaling`, or the helpers census (`check-shared-helpers.py`) fails.

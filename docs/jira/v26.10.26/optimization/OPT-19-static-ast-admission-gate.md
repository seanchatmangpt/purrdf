# [OPT-19] Static AST Admission Gate (LIMIT>64, Unindexed Cartesian, Unbounded Path) ≤42 ns

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/engine.rs:1056` (`prepare_keyed_admitted`, reached from `engine.rs:815` / `prepare_query` `:1712`); refusal enums at `crates/sparql-eval/src/error.rs:98-137,175`; walk template at `crates/sparql-eval/src/basic_profile.rs:224`
- **Subsystem:** Ingestion (query admission) / Relational Sieve boundary
- **Inspection Agent:** Agent 19 (Static AST Capability Gating)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `LIMIT` parsed with no ceiling — `crates/sparql-algebra/src/parser.rs:1749-1754` and `:1796-1801` wrap the pattern in `GraphPattern::Slice { start, length: modifiers.limit }` with `limit: Option<usize>` (parser.rs:3744); `LIMIT 1000000000` parses fine. `Query::validate`'s Slice arm is an explicit no-op (`validate.rs:748` — `G::Slice { .. } => {}`). Unbounded user-function recursion guarded only at eval (`MAX_UDF_DEPTH: u32 = 32`, eval.rs:940, checked :3217-3219); query nesting depth is not statically bounded. Cartesian products are handled in the **planner**, not admission (`join_plan.rs:131,276` `connected_join`; `bgp.rs:1201,2105` connected-scheduling) — a syntactic `Join` of two variable-disjoint patterns **evaluates**, bounded only by runtime governors. `ZeroOrMore`/`OneOrMore` (`algebra.rs:1323`) execute the transitive closure at eval (`path.rs:542-544, 1588-1596`) with **no admission-time rejection of unbounded paths anywhere**.
- **Hardware-Level Impact:** structurally unbounded shapes (huge LIMIT, unindexed cartesian, open-endpoint transitive closure) reach the evaluator and consume fuel/rows/intermediate cells until a runtime governor trips — the cost is discovered by execution, not refused at admission.
- **Baseline Cost:** admission currently O(1) per query at `admit_version`; the proposed gate must stay ≤42 ns typical.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** one flat, non-recursive worklist walk over the borrowed `GraphPattern`, placed inside `prepare_keyed_admitted` (engine.rs:1056) immediately after parse, next to `admit_version` (`eval.rs:5815`, called :5851 for queries / update.rs:425 for updates) — the chokepoint the repo's own doc names for admission-time policy (`error.rs:392-398`). Reuse `walk.rs`/`worklist.rs` style; mirror `basic_profile::check_pattern` (basic_profile.rs:224, an exhaustive non-recursive pattern walk). Per node: integer compare for Slice, fixed-width word variable-set intersection for Join, variant check + boundness test for Path. O(nodes), branch-predictable, **no allocation on the accept path**; aborts on first refusal.

**Amendment (OPT-42 scan, 2026-10-26): the Join variable-set test is `SmallVec`-free.**
Do not build a `SmallVec` var-set intersection for the `UnindexedCartesian` check —
var-sets are packed into fixed `u64` words and tested with the landed kernels in
`crates/branchless/src/bitset.rs` (ALIVE, `Implemented in purrdf::branchless::bitset`):
- intersection: `pub const fn intersect_u64(x: u64, y: u64) -> u64` — `x & y`
  (bitset.rs:107-109, ported from bcinr `algorithms/set_intersection_branchless.rs:33`).
- subset predicate: `pub const fn is_subset_u64(x: u64, y: u64) -> u64` —
  `((x & !y) == 0) as u64`, 0/1 flag without a branch (bitset.rs:126-128, from
  bcinr `algorithms/is_subset_mask_u64.rs:24`). This is the anchored-arm /
  shared-variable containment test — it returns a `u64` mask, not `bool`; the
  gate's `if flag != 0` comparison is the only branch.
- sets wider than 64 vars: `intersect_u64_slices(a: &mut [u64], b: &[u64])`
  (bitset.rs:225) on fixed-capacity scratch, still allocation-free — but the
  admission cap (depth ≤ 32, LIMIT ≤ 64) means the u64-word path always wins in
  practice.

This keeps the admission gate's "0 heap allocations on the accept path" invariant
mechanical: two `u64` words per pattern side, one AND, one residual-flag test —
no `SmallVec` spill slot, no growth branch.
- **Typed refusals** (new `PolicyKind` following `UnsupportedKind` at error.rs:98-137, stable diagnostic codes error.rs:175 style `sparql-policy-*`):
  - `LimitExceeded` — `Slice { length: Some(n) } if n > 64` (strict `>`, never `>=`).
  - `UnindexedCartesian` — `Join(l, r)` with disjoint variable sets where **neither arm is anchored** (no BGP/path triple with a concrete term shared through an outer scope).
  - `UnboundedPath` — `ZeroOrMore`/`OneOrMore` where an endpoint is unbound at that point. **Gate on boundness, not the operator**: `p*`/`p+` with both ends bound is a bounded, cheap closure; plain `p` shares the same AST `Path` node and must pass.
  - `DepthExceeded` — static walk depth counter on `Child` descent, cap sized to the `MAX_UDF_DEPTH = 32` precedent.
- **Hard Bounds Applied:** LIMIT ≤ 64; no unanchored variable-disjoint joins; no unbound-endpoint transitive closures; nesting ≤ 32.
- **Proposed Type Layout & Signatures:** `fn admit_pattern(pattern: &GraphPattern, outer_bound: &VarSet) -> Result<(), PolicyRefusal>` with `#[repr(u8)] enum PolicyKind { LimitExceeded, UnindexedCartesian, UnboundedPath, DepthExceeded }`; refusal carries `construct: &'static str` + exact observed/limit numbers (allocation-free counts, `CapacityError` style).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** accept path is O(nodes) with zero conditional branches beyond the per-node predictable checks; no allocation; abort-on-first-refusal.
- **Allocation Invariant:** 0 heap allocations on the accept path.
- **Benchmark Target:** ≤ 42 ns for a typical 4-triple BGP query; measured in the OPT-20 bench suite.
- **Over-refusal guardrails (mandatory, CLAUDE.md "a refusal is a claim too"):** every gate is a strict inequality or boundness predicate with an immediately adjacent legal twin, and each gate ships a **paired test executing both cases**:
  - `LIMIT 64` and `LIMIT 63` pass (plus sub-`SELECT` with `LIMIT 10` inside an aggregate query).
  - The same disjoint join with one shared variable passes; `{ VALUES ?a { 1 } } { ?s ?p ?a }` passes (planner already proves this connected at bgp.rs:2105 — the static set-intersection must agree with it).
  - `p+` with both endpoints bound passes; plain `p` passes; `p*` inside `FILTER EXISTS` over a bound subject passes.
  - Nesting one level below the cap (depth 31 if capped at 32) parses and prepares identically to today.
- **Layer Boundary:** runtime governors stay untouched (`governor/mod.rs`; slice pushdown eval.rs:4648; `join_plan.rs` connectivity normalization; `path.rs` closure machinery) — the gate sits strictly **before** them and rejects only structurally unbounded shapes; governed truncation remains the mechanism for data-dependent cost. Zero `check-layers.py` violations.

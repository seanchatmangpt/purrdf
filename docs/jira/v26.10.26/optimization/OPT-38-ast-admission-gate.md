# [OPT-38] Static AST Capability Gate at Admission (LIMIT ≤ 64, No Unindexed Cartesian, Bounded Paths)

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/engine.rs:1056` (`prepare_keyed_admitted`, reached from `prepare` :815 / `prepare_query` :1712); template `crates/sparql-eval/src/basic_profile.rs:224` (`check_pattern`); gap evidence `crates/sparql-algebra/src/parser.rs:1749-1754,1796-1801`, `validate.rs:748`
- **Subsystem:** Ingestion (admission)
- **Inspection Agent:** Agent 19 (Static AST Capability Gating)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** LIMIT is parsed with no ceiling — `GraphPattern::Slice { start, length: modifiers.limit }` with `limit: Option<usize>` never range-checked; `LIMIT 1000000000` parses fine (parser.rs:1749-1801; structural validation explicitly no-ops Slice, validate.rs:748 `G::Slice { .. } => {}`). Admission-time refusal is the repo's stated policy ("refused here, at evaluation admission, rather than at parse time", error.rs:392-398) via the single chokepoint `admit_version` (eval.rs:5815, called :5851 / update.rs:425) — but no static cap on LIMIT value, query nesting depth, unindexed cartesian, or unbounded property paths exists: `p+`/`p*` execute the transitive closure at eval with visit-tracking only (path.rs:542-544,1588-1596); syntactically disjoint `Join` arms evaluate (bounded only by runtime governors); UDF recursion bounded only at eval (`MAX_UDF_DEPTH = 32`, eval.rs:940,3217-3219); LIMIT cost bounded lazily via slice pushdown (eval.rs:4648, 853-857).
- **Hardware-Level Impact:** structurally unbounded shapes reach the evaluator and burn fuel before a runtime governor trips; a static compare at prepare is nanoseconds vs seconds of wasted execution.
- **Baseline Cost:** 0 static checks today; wasted eval work bounded only by caller-supplied governors (default UNBOUNDED per OPT-07).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** one flat, non-recursive worklist walk over the borrowed `GraphPattern` inside `prepare_keyed_admitted` (engine.rs:1056) immediately after parse, next to `admit_version` — the lane every query already passes and the site the repo's own doc names. Mirror `basic_profile::check_pattern`'s exhaustive pattern-walk structure (basic_profile.rs:224) and the iterative `walk.rs`/`worklist.rs` style. Per node: an integer compare for `Slice`, a u128/`SmallVec` variable-set intersection for `Join`, a variant+boundness check for `Path`; O(nodes), branch-predictable, zero allocation on the accept path, aborts on first refusal. Typed refusals following the `UnsupportedKind` pattern (error.rs:98-137) with stable `sparql-policy-*` diagnostic codes (error.rs:175 style): `LimitExceeded` (`Slice.length > 64`), `UnindexedCartesian` (disjoint-variable `Join` where neither arm is anchored to a concrete term through outer scope), `UnboundedPath` (`ZeroOrMore`/`OneOrMore` with an unbound endpoint), `DepthExceeded` (static depth counter, cap sized to the `MAX_UDF_DEPTH` precedent).
- **Over-refusal guardrails (hard requirement — "a refusal is a claim too"):** every gate is a strict inequality or boundness predicate with an immediately adjacent legal twin, each shipped as a paired test executing both cases: `LIMIT 64` (and 63, and a sub-SELECT `LIMIT 10`) must pass; the same Join with one shared variable (and `{ VALUES ?a { 1 } } { ?s ?p ?a }`) must pass — the static set-intersection must agree with the planner's connectivity proof (bgp.rs:1201,2105; join_plan.rs:131,276); `p+` with both endpoints bound (and plain `p`, and `p*` inside `FILTER EXISTS` over a bound subject) must pass; nesting one level below the cap must prepare identically to today.
- **What must NOT move:** runtime governors (governor/mod.rs) stay — the static gate rejects only structurally unbounded shapes; slice pushdown (eval.rs:4648), connectivity normalization (join_plan.rs:131), and path closure machinery untouched; the gate sits strictly before them.
- **Hard Bounds Applied:** LIMIT ≤ 64; traversal endpoints bound or refused; static depth cap = 32 (matches MAX_UDF_DEPTH); gate budget ≤ 42 ns typical queries.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub const MAX_SLICE_LENGTH: usize = 64;
  #[repr(u8)] pub enum PolicyKind { LimitExceeded, UnindexedCartesian, UnboundedPath, DepthExceeded }
  fn check_capability_walk(pattern: &GraphPattern) -> Result<(), PolicyRefusal>;  // flat worklist, no alloc on accept
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** per-node checks are integer compares + set-intersection; walk is iterative (no stack recursion).
- **Allocation Invariant:** 0 heap allocations on the accept path (SmallVec-bound variable sets).
- **Benchmark Target:** ≤ 42 ns per typical prepared query (bench in OPT-20 suite); every paired valid-neighbor test green — the gate must never refuse the adjacent legal query.
- **Layer Boundary:** gate lives in `sparql-eval` admission; `check-layers.py` clean; no parser (`sparql-algebra`) changes needed — admission-time per repo policy.

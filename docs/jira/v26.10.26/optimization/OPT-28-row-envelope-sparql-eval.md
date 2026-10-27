# [OPT-28] Plumb the ≤64 Row Envelope Into sparql-eval; Close the Unbounded Parallel Join Arm

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/binop.rs:2234,2247-2283,2312-2343,2375-2388`; `crates/sparql-eval/src/governor/mod.rs:651-667`; reference implementation `crates/duckdb/src/eval.rs:18,21,48-52,237-246`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 06 (Topological Envelope Bound ≤ 64)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** the tripwire already exists in duckdb exactly as specified — `DEFAULT_ROW_ENVELOPE: usize = 64` (eval.rs:18), typed `EvaluatorError::RowEnvelopeExceeded { limit, observed, max }` (:48-52), fail-closed trip inside the materialization loop *before* conversion/push (:237-246), contract locked by tests (`row_projection_isolation.rs:65-81`, `chicago_sql_edge_cases_court.rs:538`). **The gap is sparql-eval**: `let cell_ceiling = ctx.cell_row_ceiling(out_len).unwrap_or(usize::MAX);` (binop.rs:2234) — governor disengaged = unbounded; sequential hash join soft-breaks (`if rows.len() >= cell_ceiling { …; break 'left }`, binop.rs:2247-2283 — stops emitting, not a typed halt); the **parallel arm is the unguarded hole**: `reserve_join_rows(acc, 1, usize::MAX)` hardcoded at binop.rs:2312/2325/2343; optional/union arms repeat the soft pattern (binop.rs:1556-1573, 1937-1963); JoinIndex build-side materialization not envelope-checked (binop.rs:2108-2117).
- **Hardware-Level Impact:** unbounded intermediate join bag growth → allocator churn, cache eviction, and OOM exposure on data-dependent cardinality blowups.
- **Baseline Cost:** intermediate bags grow to governor limits which default to UNBOUNDED (caller-supplied only, governor/mod.rs:651-667).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** plumb `cell_row_ceiling` to a non-`None` default of 64; convert the soft `break 'left` breaks into a typed `Err` following the `WorkspaceBoundOverflow`/`StackExhausted` naming (`error.rs:366, 641`); replace `usize::MAX` in `reserve_join_rows` (binop.rs:2375-2388 — the existing `try_reserve_exact` typed-growth seam) with the same ceiling; envelope-check the JoinIndex build side. Keep `DEFAULT_ROW_ENVELOPE = 64` a `pub const` local to the owning crate with re-export (repo convention: constants are local + re-exported, precedent `sink.rs:97 DRAIN_BUFFER_BYTES`; `[workspace.dependencies]` is dep-versions only; cross-crate home, if needed, is `purrdf-core`). Keep the check a compare+branch — the trip path returns `Result::Err` and unwinds; a cmov buys nothing, and control-flow halt is what makes it fail-closed. Semantics: inclusive ceiling, checked BEFORE conversion/push, so row 65 is observed-but-never-stored (matches the duckdb contract, `observed: 65, max: 64`). Runtime governors stay: the static envelope rejects structurally unbounded intermediate materializations; governed truncation (AnswerRows/IntermediateCells, governor/mod.rs:651-667) remains the data-dependent mechanism; slice pushdown (eval.rs:4648) and join connectivity normalization untouched.
- **Hard Bounds Applied:** ≤ 64 materialized rows per intermediate bag (all join arms, parallel included); build-side index bounded likewise.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub const DEFAULT_ROW_ENVELOPE: usize = 64;                 // purrdf-core home if shared
  EvalError::RowEnvelopeExceeded { limit: usize, observed: usize }  // new variant, error.rs naming family
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** single compare-branch per push on the hot emit path (predictable not-taken); trip path is a typed unwind.
- **Allocation Invariant:** `try_reserve_exact` growth bounded by the ceiling; no geometric reallocation past 64 rows.
- **Benchmark Target:** trip fires the exact nanosecond row 65 is touched; boundary rerun of `duckdb/tests/row_projection_isolation.rs` first, then a new sparql-eval court asserting the parallel arm equals the sequential arm's bound and `observed: 65, max: 64`.
- **Layer Boundary:** `check-layers.py` clean; existing governor conformance tests unchanged.

# [OPT-06] Plumb the ≤64 Row Envelope Into sparql-eval and Close the Unbounded Parallel Join Arm

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/binop.rs:2247-2283,2313-2343,2108-2117,2375-2388`; `crates/sparql-eval/src/solution.rs:1156-1158`; `crates/sparql-eval/src/governor/mod.rs:651-667`; reference implementation `crates/duckdb/src/eval.rs:18,21,48-52,237-246,178-192`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 06 (Topological Envelope Bound ≤ 64)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** Parallel join arm: `reserve_join_rows(acc, 1, usize::MAX)?; acc.push(merge_admitted(...))` at `binop.rs:2313,2326,2343` — the ceiling is hardcoded `usize::MAX`, i.e. **UNBOUNDED**. Sequential arm stops with a soft break: `if rows.len() >= cell_ceiling { let _ = ctx.observe_cells(rows.len().saturating_add(1), out_len); break 'left; }` (`binop.rs:2247-2283`) — emission stops silently, no typed error. Join-index build side (`entry.1.push(idx)?`, `bucket.push(idx)?`, `wild.push(idx)?` at `binop.rs:2108-2117`) is not envelope-checked. Governor dimensions default to disengaged: `let cell_ceiling = ctx.cell_row_ceiling(out_len).unwrap_or(usize::MAX);` (`binop.rs:2234`) — `None` means unlimited.
- **Hardware-Level Impact:** unbounded intermediate join-bag growth: cache/TLB churn and allocator pressure scale with join fan-out rather than the candidate envelope; the 64-row candidate envelope invariant (`duckdb/src/eval.rs:18` `DEFAULT_ROW_ENVELOPE = 64`) is enforced on the duckdb egress but not on the SPARQL-side materialization that feeds it.
- **Baseline Cost:** unbounded rows × columns per join stage (governor-engaged callers excepted); worst case grows with data-dependent join selectivity with no fail-closed trip point.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Extend the already-proven duckdb tripwire to sparql-eval rather than inventing a new mechanism:
  1. **Default the ceiling.** Plumb `cell_row_ceiling` to a non-`None` default of 64 (matching `DEFAULT_ROW_ENVELOPE`) so `binop.rs:2234`'s `unwrap_or(usize::MAX)` fallback disappears; callers may still raise the ceiling explicitly via `QueryGovernors` (`governor/mod.rs:651-667` `with_max_answers` / `with_max_intermediate_cells`).
  2. **Typed halt, not soft break.** Replace the `break 'left` soft-stops at `binop.rs:2247-2283` with `return Err(EvalError::RowEnvelopeExceeded { ... })`, a new variant following the existing typed-failure naming (`WorkspaceBoundOverflow` / `StackExhausted`, `error.rs:366,641`), carrying `{ limit, observed, max }` mirroring the duckdb variant (`duckdb/src/eval.rs:48-52`).
  3. **Close the parallel hole.** Replace `reserve_join_rows(acc, 1, usize::MAX)` at `binop.rs:2313,2326,2343` with the same ceiling; the existing `try_reserve_exact` guard (`binop.rs:2375-2388`) already provides the fail-closed growth seam — reuse it with the real bound.
  4. **Bound the build side.** JoinIndex construction (`binop.rs:2108-2117`) gets the same inclusive-64 check before each `push`.
- **Hard Bounds Applied:** ≤ 64 rows per intermediate join bag (inclusive ceiling: rows 1..=64 materialize; row 65 is observed in the error but **never stored** — checked before conversion/push, matching duckdb's pre-push trip point at `eval.rs:237-246`).
- **Proposed Type Layout & Signatures:**
  ```rust
  pub const DEFAULT_ROW_ENVELOPE: usize = 64;          // per-crate (duckdb precedent, eval.rs:18)
  pub const ROW_ENVELOPE_LIMIT: &str = "sparql_join_rows";
  EvalError::RowEnvelopeExceeded { limit: &'static str, observed: usize, max: usize }
  ```
  Constant home: per-crate `pub const`, re-exported via the crate's `lib.rs` (duckdb precedent, `duckdb/src/lib.rs:8`). **Not** `[workspace.dependencies]` — that table is for dependency versions only; if a shared constant is ever needed, `purrdf-core` is the conventional home (precedent: `sink.rs:97` `DRAIN_BUFFER_BYTES`).
  The bound check stays a **control-flow branch** (compare + Err unwind), not a cmov: the trip path must halt and fail closed, so a conditional-move formulation buys nothing and would weaken the fail-closed semantics.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A — the envelope check is deliberately a fail-closed control-flow halt (typed `Err` + unwind), mirroring the duckdb circuit breaker; a "branchless" envelope would not be fail-closed.
- **Allocation Invariant:** join-bag growth never exceeds 64 rows × column width without a typed refusal; `try_reserve_exact` remains the only growth path (`binop.rs:2375-2388`).
- **Benchmark Target:** trip latency ≤ duckdb parity (sub-microsecond check per row; error path unbounded but off the hot path); sequential and parallel join arms both trip at exactly row 65 with `observed: 65, max: 64`.
- **Layer Boundary:** zero cyclic or layer violations verified via `check-layers.py` (error variant lives in `sparql-eval`; no new cross-crate dependency). Tests mirror `duckdb/tests/row_projection_isolation.rs:65-81`: 64 rows pass, 65 trips typed; parallel arm parity-tested against the sequential arm. Runtime governors stay authoritative for data-dependent truncation — the static AST gate is a separate ticket (OPT-19), not this one.

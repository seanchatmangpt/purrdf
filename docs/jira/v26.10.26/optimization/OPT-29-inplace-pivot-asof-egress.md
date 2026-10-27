# [OPT-29] In-Place PIVOT/ASOF Sieve and Zero-Allocation duckdb Egress

## 1. TARGET CONTEXT
- **Crate & File:** `crates/duckdb/src/eval.rs:18,211-236,241-351,344-348`; `crates/columnar/src/projection.rs:52-53,115-124`; `crates/columnar/src/writer.rs:111-114,374-396,441-457`; `crates/columnar/src/reader.rs:112,169,293,440-441,454,482`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 07 (In-Place Analytical PIVOT/ASOF Sieve)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** PIVOT/ASOF have NO in-Rust execution — they are delegated to frozen-duckdb as SQL text (`compile_pivot_statement` renders `format!("PIVOT {table} ON {on_list} IN ({in_list}) USING {agg}({agg_col})")`, compiler.rs:2209; `JoinType::Asof as_sql() => "ASOF JOIN"`, sql/src/ast.rs:594-626), so all intermediate materialization happens inside opaque DuckDB operators. The Rust egress loop is where the allocation cost lives: `duck_params: Vec<Box<dyn frozen_duckdb::ToSql>>` (one boxed alloc per parameter per call, eval.rs:211-223), `column_names: Vec<String>` (:230-236), `let mut result_triples = Vec::new();` with zero capacity hint (:238) growing via per-row push (:344-348), `format!`-built `row_subject`/predicate strings **per cell** (:252,255), per-text-cell `from_utf8_lossy().into_owned()` (:324), and fresh datatype-IRI `String`s re-allocated thousands of times per run (:262-338). Columnar: `ProjectionRows::next` returns a fresh `Vec<ProjectionCell>` per yielded row (projection.rs:115-124, doc admits it at :52-53); writer quadruple-materializes (`quads`/`reifiers`/`annotations` `collect()` + BTreeSet, writer.rs:111-114) with one `to_vec()` per term payload (:441,450,452,457); reader re-materializes with per-term `value.clone()` (reader.rs:440-441).
- **Hardware-Level Impact:** ~64×N×3 + heap allocations per 64-row eval; geometric `Vec` regrowth; per-term payload copies.
- **Baseline Cost:** allocation count per 64-row, 5-col eval ≈ 64×5×3 + 3 boxed/Strings + per-call `Vec<Box<dyn ToSql>>`.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  - **D3 (highest ROI, mechanical):** kill per-cell string allocation in eval.rs — `OnceLock`/static table of the 8 datatype IRIs (:262-338); `row_subject` built into a reusable `String` with `write!` + `clear()` per row (:252); params passed as `&[SqlLiteral]` or a fixed `[Option<Box<dyn ToSql>>; 16]` (:211-226); pre-capacity `result_triples` from the envelope bound. Allocation count per 64-row 5-col eval drops from ~64×5×3 + 3 to ~3.
  - **D1 (bounded-envelope pivot):** in-place PIVOT into caller-owned columnar buffers for the dense small-IN-list case — pre-allocate `Int64Column::with_capacity(max_rows)` per output column (column.rs:199-204; `max_rows` = DEFAULT_ROW_ENVELOPE 64, eval.rs:18); group-key → slot via fixed `[Option<u32>; 64]` or a pre-allocated `Vec<(u64, u32)>::with_capacity(64)`; the closed aggregate set (`sum|count|avg|min|max`, compiler.rs:2172-2182) is one accumulator per output column; rotation is a scatter (`out_col[slot].values[cursor] = acc`, presence bit set, cursor advance). Invariant: `values.len() == presence.count()` assertable at every step (column.rs:208 precedent).
  - **D2 (bounded ASOF):** two-pointer merge over two pre-sorted dense key arrays — sort each side once in place into a reused scratch `Vec<(i64 ts, u32 idx)>::with_capacity(n)`; probe row *i* advances *j* while `ts_b[j+1] <= ts_a[i]`; output `join_idx[i] = j` into a pre-allocated `Vec<u32>`. O(n+m), zero per-row allocation. Probe side truncated to the envelope via the existing `RowEnvelopeExceeded` breaker before the merge (eval.rs:244-250). `JoinType::Asof` keeps its SQL path for the general case; the in-place path serves `max_rows <= 64`.
  - **D4:** reuse one scratch `Vec<ProjectionCell>` inside `ProjectionRows` — iterator item becomes a borrowed `&[ProjectionCell]`, removing row_count allocations per traversal (API change, callers updated).
  - Writer/reader intermediates (quadruple materialization, per-term `to_vec`/`clone`) are a follow-on: intern term payloads into one arena instead of per-term owned `Vec<u8>`.
- **Hard Bounds Applied:** rows ≤ 64 (envelope); pivot IN-list explicitly validated non-empty (compiler.rs:2196); aggregate set closed (5 kinds); scratch buffers single-allocated per call.
- **Proposed Type Layout & Signatures:**
  ```rust
  struct PivotScratch { slots: [Option<u32>; 64], keys: Vec<(u64, u32)>, cursors: Vec<u32> }
  fn asof_merge_sorted(a: &[(i64, u32)], b: &[(i64, u32)], out: &mut Vec<u32>);
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** pivot scatter and ASOF merge are allocation-free linear scans; aggregate folds are branchless saturating ops (cross-ref OPT-22).
- **Allocation Invariant:** D3 target ≤ 3 allocations per 64-row eval; D1/D2 zero beyond the single pre-allocation; D4 removes per-row `Vec`.
- **Benchmark Target:** `crates/duckdb/benches/sieve_bench.rs` (PIVOT→ASOF→QUALIFY pattern) allocation-count and throughput regression; boundary rerun of `row_projection_isolation.rs` first.
- **Layer Boundary:** duckdb/columnar edges per `layers.toml`; `check-layers.py` clean; `check-shared-helpers.py` — no new duplicate scratch helpers (see README note on the pre-existing 323-failure state).

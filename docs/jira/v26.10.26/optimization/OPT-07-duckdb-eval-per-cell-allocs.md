# [OPT-07] Kill Per-Cell String Allocation in the DuckDB Evaluator Row Loop

## 1. TARGET CONTEXT
- **Crate & File:** `crates/duckdb/src/eval.rs:211-256,262-344`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 07 (In-Place Analytical PIVOT/ASOF Sieve)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - eval.rs:211-223 — `duck_params: Vec<Box<dyn frozen_duckdb::ToSql>>`: one boxed heap allocation per query parameter, per call; eval.rs:225-226 builds a second `Vec<&dyn ToSql>` vtable-fat-pointer array over them.
  - eval.rs:230-236 — `column_names: Vec<String>`: one `String` allocation per column, per call.
  - eval.rs:238 — `let mut result_triples = Vec::new();`: zero capacity hint; grows via `result_triples.push(RdfTriple {...})` (eval.rs:344-348) inside the row loop with repeated geometric reallocation.
  - eval.rs:252, 255 — `format!` allocates `row_subject` and `predicate` strings **per cell** (64 rows × N cols `String` allocs per eval).
  - eval.rs:324 — `String::from_utf8_lossy(bytes).into_owned()` — per-text-cell allocation.
  - eval.rs:262-338 — every literal builds fresh datatype `String`s (`"http://www3.org/2001/XMLSchema#integer".to_string()` and siblings) per cell — the same 8 constant IRIs re-allocated thousands of times per run.
- **Hardware-Level Impact:** allocator lock contention per cell, heap fragmentation, and cache pollution from thousands of tiny `String` blocks per eval; every `RdfTriple` field is a separate heap block.
- **Baseline Cost:** ~64 rows × 5 cols × 3+ allocations ≈ 1,000 heap allocations per single bounded eval, plus per-call param/column setup.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. `static`/`OnceLock` table of the 8 datatype IRIs (eval.rs:262-338); every literal arm borrows the constant `&'static str` instead of `.to_string()`.
  2. `row_subject` built into a single reusable `String` with `write!` + `clear()` per row (eval.rs:252); predicate likewise per column.
  3. `param_refs` replaced by a fixed `[Option<Box<dyn ToSql>>; 16]` inline array, or by passing `&[SqlLiteral]` directly if frozen-duckdb permits — removing one boxed alloc per parameter and the second vtable array.
  4. `result_triples = Vec::with_capacity(envelope)` (envelope already bounded at 64, see OPT-06) — no geometric regrowth.
  5. Text cells: keep `from_utf8_lossy` but write into the reusable subject buffer where the lifetime permits, else the single remaining `into_owned()` per text cell stands (bounded by envelope).
- **Hard Bounds Applied:** result rows ≤ 64 (`DEFAULT_ROW_ENVELOPE`, eval.rs:18); params ≤ 16 (fixed array, typed failure beyond); datatype IRIs drawn from a closed 8-element constant set.
- **Proposed Type Layout & Signatures:**
  ```rust
  const DATATYPE_IRIS: [&str; 8] = [/* xsd:integer, xsd:decimal, ..., rdf:langString */];
  params: [Option<Box<dyn ToSql>>; 16],          // inline, no Vec
  row_subject: String (reused, cleared per row),
  result_triples: Vec<RdfTriple> with_capacity(64)
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation-focused); bounds checks are single predictable compares against the envelope.
- **Allocation Invariant:** heap allocations per 64-row/5-col eval drop from ~64×5×3+3 to ≤ ~3, measured by `alloc-probe` (real counter, not assertion).
- **Benchmark Target:** extend the existing duckdb eval bench/sieve bench; allocation-count receipt and ≥20% wall-clock improvement on the 64-row envelope case.
- **Layer Boundary:** changes confined to `crates/duckdb`; zero violations via `python3 scripts/check-layers.py`.

# [OPT-21] Columnar: Kill Per-Row Projection Vecs and Quadruple Writer Materialization

## 1. TARGET CONTEXT
- **Crate & File:** `crates/columnar/src/projection.rs:52-53,115-124`; `crates/columnar/src/writer.rs:111-114,234,246-247,295,374-396,441-457`; `crates/columnar/src/reader.rs:112,169,293,440-441,454,482`
- **Subsystem:** Relational Sieve (columnar container build/decode)
- **Inspection Agent:** Agent 07 (In-Place Analytical PIVOT/ASOF Sieve)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - Projection: `Some(self.columns.iter_mut().map(ColumnRows::next_cell).collect())` — one `Vec<ProjectionCell>` heap allocation **per yielded row**; the module doc admits it at `projection.rs:52-53` ("One small cell vector is allocated per yielded row").
  - Writer gather: `SourceRows::collect` (`writer.rs:111-114`) materializes **four whole-dataset intermediates simultaneously** — `let quads: Vec<_> = view.quads().collect(); let reifiers: Vec<_> = ...; let annotations: Vec<_> = ...;` plus a `BTreeSet` gather of named graphs.
  - Term table: `build_terms_table` (`writer.rs:374-396`) builds 12 parallel `Vec`s where each term payload does `iri.as_bytes().to_vec()` (`:441,450,452,457`) — one heap alloc per term, zero dictionary/interning reuse; nested-term resolution uses a `frames` stack with `resolved: Vec::new()` + push per value (`:234,246-247,295`).
  - Reader decode: `Vec::with_capacity(bounded_row_capacity(...))` chains for named_graphs/records/`QuadRow`/`ReifierRow` (`reader.rs:112,169,454,482`); `resolve_term_records` (`:293`) produces a full `Vec<TermValue>` with per-term `value.clone()` (`:440-441`) plus `parent.resolved.push(value)`.
- **Hardware-Level Impact:** geometric reallocation + memcpy per row yielded; 4× dataset residency at write time; allocator lock per term on both encode and decode paths; cache-cold clones per resolved term.
- **Baseline Cost:** `row_count` allocations per projection traversal; ~4N intermediate elements + N term-payload allocs per write; N clones per decode.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. **Projection (D4):** store `cells: Vec<ProjectionCell<'a>>` in the `ProjectionRows` struct; fill in place per row and return `&[ProjectionCell]` — iterator item becomes a borrowed slice. Allocation count per traversal: `row_count → 1`.
  2. **Writer:** replace `SourceRows::collect` with sequential drain into single-pass column builders (or one reusable scratch drained per source), eliminating simultaneous whole-dataset residency. Intern term payloads once (reuse the `purrdf-core` interner / arena, cross-ref OPT-02) or write bytes into a shared arena instead of `to_vec()` per term; the frames stack's `resolved` buffer becomes a reusable scratch cleared per frame.
  3. **Reader:** resolve term records into a caller-provided/arena-backed buffer; replace per-term `value.clone()` with range/index-based resolution into `StrRange`-style slices over the existing payload bytes (mirroring `purrdf-core`'s `Interner` arena, `ir/builder.rs:320-325`).
  - Companion runtime tickets (in-place PIVOT into caller-owned buffers, two-pointer ASOF merge — Agent 07 D1/D2) sit on top of these containers; this ticket covers container-side materialization only.
- **Hard Bounds Applied:** presence invariant `values.len() <= presence.count() <= max_rows` assertable at every step (existing `debug_assert_eq!(presence.count(), values.len())` precedent, `column.rs:208`); all buffers pre-allocated via `Int64Column::with_capacity` (`column.rs:199-204`) bounded by the row envelope (64, `duckdb/eval.rs:18`).
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct ProjectionRows<'a> {
      columns: [ColumnRows<'a>; N],
      cells: Vec<ProjectionCell<'a>>,   // reused; filled in place per row
  }
  impl<'a> ProjectionRows<'a> {
      pub fn next_row(&mut self) -> Option<&[ProjectionCell<'a>]>;  // borrowed, zero alloc after first
  }
  ```

**Amendment (OPT-42 scan, 2026-10-26): prerequisite — straddling field kernels.**
Items 2 and 3 (arena-backed term payload writes, range-based decode resolution)
depend on moving byte runs that **straddle** the fixed-width lanes the arena
writes in. Two kernels are required and **do not exist yet in
`crates/branchless`** (grep over `crates/branchless/src/`: zero matches for
`field_extract2`/`field_deposit2`/`funnel`):
- `field_extract2(hi: u64, lo: u64, shift: u128) -> u64` — pull a >64-bit-field
  window out of a `(hi, lo)` register pair.
- `field_deposit2(base: u64, val: u64, shift: u128) -> u64` — deposit it back.

These are the funnel-shift family from BranchlessCInRust, read-only provenance:
- `~/bcinr/crates/bcinr-logic/src/algorithms/funnel_shift_left_u64.rs:22-27` —
  `(val.wrapping_shl(shift)) | (aux.wrapping_shr((64u32.wrapping_sub(shift)) & 0x3F) ...)`
- `~/bcinr/crates/bcinr-logic/src/algorithms/funnel_shift_right_u64.rs:22-27` —
  the mirrored right-rotation-acarry form.

Port order: land the two kernels in `purrdf-branchless` (one home, next to the
packing/swar families in `helpers-ledger.toml`) **before** this ticket's
arena-backed writer/reader work starts; the projection work (item 1) is not
blocked by them. No duplicate port may live in `crates/columnar` or
`crates/rdf-core` — consumers re-export from `purrdf::branchless`.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation-focused); presence bitmap ops remain branchless bit tests.
- **Allocation Invariant:** 0 heap allocations per row after the first in projection; ≤1 arena block per column build/decode; alloc-probe shows O(columns), not O(rows × cells).
- **Benchmark Target:** allocations per 64-row decode via `purrdf-alloc-probe` — target ≤ number of columns; writer peak residency ≤ 1× dataset (not 4×).
- **Layer Boundary:** columnar stays above `purrdf-core` per `layers.toml`; no interner duplication — `check-shared-helpers.py` / `check-layers.py` clean.

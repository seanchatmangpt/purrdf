# [OPT-49] Datalog Solution Source-Row Small-N Sorts to Inline Networks

## 1. TARGET CONTEXT
- **Crate & File:** `crates/datalog/src/seminaive/factors/matches.rs:135`;
  `crates/datalog/src/seminaive/join.rs:392-395`; container
  `crates/datalog/src/seminaive.rs:1936` (`sources: Vec<SourceRow<I, N>>`)
- **Subsystem:** Relational Sieve
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5, second half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `matches.rs:135`
    `solution.sources.sort_by_key(|source| source.body_index);` — per
    solution, inside the factor-match output loop; `sort_by_key` (stable
    sort) allocates a merge buffer per call
  - `join.rs:395`
    `.sort_unstable_by_key(|source| source.body_index);` — per solution
    in the anchor-sequence output loop, again per-call
  - Container: `seminaive.rs:1936` `sources: Vec<SourceRow<I, N>>` — heap
    `Vec` per solution even though N (the rule body arity, the `const N`
    in `SourceRow<I, N>`) is a compile-time constant typically 2-4.
- **Hardware-Level Impact:** stable `sort_by_key` on a 2-4 element
  `Vec` is an insertion sort behind a capacity branch plus an allocator
  round-trip per solution per fixpoint iteration — alloc + call overhead
  dominating a 1-2 swap workload. Sorting by `body_index` (a u32-class
  key) over a const-generic small N is exactly the fixed-bound
  comparator-network case: depth-2/depth-6 branch-free networks, register
  resident, zero alloc.
- **Baseline Cost:** UNVERIFIED — `crates/datalog/tests/factor_allocation.rs`
  counts allocations for factor paths; extend it for the sorted-output
  path baseline before/after.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Two moves, in order.
  1. Container: replace `sources: Vec<SourceRow<I, N>>` with a
     const-N inline buffer (`[Option<SourceRow<I, N>>; N]` + len, or
     `heapless::Vec`-style inline storage already in-tree — check
     `purrdf_branchless`/core for an existing inline-vec home first; do
     NOT add a dependency). With N <= 8 inline, the sort becomes a fixed
     network.
  2. Sort: sort the inline buffer by packed `body_index` key using
     `purrdf_branchless::sort_small::{sort5, sort6, sort7, sort8}`
     (choose by N at compile time via the const generic). Keep semantic
     `sort_by_key` stability irrelevant: `body_index` values are unique
     per solution (one source row per body atom), so a total order on
     the key makes stable/unstable observationally identical — pin this
     with a test.
- **Hard Bounds Applied:** N is `const` per `SourceRow<I, N>`; network
  depth fixed by N; zero heap allocation in the hot loop.
- **Proposed Type Layout & Signatures:**
  ```rust
  fn sort_sources<const N: usize>(src: &mut [SourceRow<I, N>]) {
      // keys extracted to [u32; N], network-sorted with permutation
      // tracking, written back — or a cswap-composed network over the
      // (key, payload) pairs if payload move is cheap
  }
  ```
- **Reuse-First:** one home per job — `purrdf_branchless::sort_small`
  (ledger row `branchless-compare-exchange`, helpers-ledger.toml:3932) is
  the only sanctioned comparator-network home; `factor_allocation.rs` is
  the existing allocation-count home. Duplicate sort bodies refused.
  - **Status:** LANDED (divergent form) — the network landed as
    `sort_sources_by_body_index` / `sort_small_by_key` in
    `crates/datalog/src/seminaive/join.rs:495-545` (same graph as
    `purrdf_branchless::sort_small::sort8`, depth 6; datalog-local edge per
    the file's own DIVERGENCE note, join.rs:495) — not a direct
    `purrdf_branchless::sort_small` call. Both call sites swapped:
    `crates/datalog/src/seminaive/factors.rs:364,850` (was scalar
    `sort_by_key`). Landed, UNVERIFIED (no-build machine).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** sorted output equals the std
  `sort_unstable_by_key` oracle over randomized solutions; uniqueness of
  `body_index` pinned by a test that a duplicate key panics/refuses at
  construction.
- **Allocation Invariant:** `factor_allocation.rs` extended: the
  sorted-factor output path performs ZERO heap allocations per solution
  (currently >= 1 from the merge buffer / Vec container).
- **Benchmark Target:** chase/seminaive fixpoint bench
  (`purrdf_testkit::bench` report-only) before/after; no asserted
  speedup without it.
- **Layer Boundary:** `make check` green; datalog → branchless edge
  passes `scripts/check-layers.py`.

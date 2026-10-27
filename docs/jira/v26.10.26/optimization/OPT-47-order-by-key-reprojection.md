# [OPT-47] ORDER BY Per-Comparison Sort-Key Re-Projection to Precomputed Rank Envelope

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/modifier.rs:1957-1971`
  (`order_values` per-comparison `project` call at :1964);
  `modifier.rs:1671-1680` (`SortKey::rank` match tower);
  `modifier.rs:1885-1929` (`total_order_admitted` /
  `shallow_order_admitted` match tower)
- **Subsystem:** Relational Sieve
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5,
  first half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  ```rust
  let mut order: Vec<usize> = (0..values.len()).collect();
  order.sort_by(|&i, &j| {
      let ord = total_order(&project(Some(&values[i])),
                            &project(Some(&values[j])));
  ```
  (modifier.rs:1962-1965). `project` builds a full `SortKey` — for
  triple terms a work-list walk with `AdmittedVec`/`Vec` staging
  (modifier.rs:1685-1710 area) — and it runs **inside the comparator**,
  i.e. O(n log n) times per sort. `total_order` then re-walks the kind
  dispatch: `rank()`'s six-arm match (modifier.rs:1671-1680) plus
  `shallow_order_admitted`'s pair-typed match tower
  (modifier.rs:1907-1929).
- **Hardware-Level Impact:** (a) the comparator is not a comparator —
  each invocation performs a fresh key construction (heap staging,
  admission charging, work-list pushes) before any comparison happens,
  so cache-resident sort work is interleaved with allocator traffic;
  (b) the kind dispatch is a two-level match tower per comparison —
  six-arm discriminant match + pair-type match — when a precomputed
  `u8` rank makes the first stage a single branch-free
  `ct_min`-style compare; (c) LLVM cannot hoist `project` out of the
  comparator (it takes `&TermValue` and allocates), so nothing is
  reuse-cached across the ~2·n·log n invocations.
- **Baseline Cost:** UNVERIFIED — no ORDER BY bench cited in
  `purrdf_testkit::bench`; add one before landing.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** sort an index permutation against a
  **precomputed key envelope**: project each `TermValue` exactly once
  into a `SortKey` (as the file's own `ORDER BY` node already does —
  the comment at modifier.rs:1958-1961 explains keys borrow the values,
  which an envelope of `(rank: u8, key)` rows preserves), store
  `rank()` per row, and in the comparator compare the two `u8` ranks
  first (branch-free ordinal compare, `ct_min`-form), descending only
  into `total_order` on rank equality. Triple-term deep comparison
  keeps the existing work-list path — it is the cold rank-tie case,
  not the hot path. The `order_values` public contract (stable sort,
  same ordering) is unchanged.
- **Hard Bounds Applied:** exactly one projection per value (n
  projections total, not 2·n·log n); rank bytes are a fixed u8
  domain (6 kinds); zero new allocation beyond one envelope row per
  value.
- **Proposed Type Layout & Signatures:**
  ```rust
  struct SortEnvelope<'a, V> { rank: u8, key: SortKey<'a, V> }
  fn envelopes<'a, V>(values: &'a [TermValue]) -> Vec<SortEnvelope>;
  // comparator: envelope[i].rank.cmp(&envelope[j].rank)
  //   .then_with(|| total_order(&key_i, &key_j))
  ```
- **Reuse-First:** no new ordering home — `total_order` /
  `shallow_order_admitted` / `SortKey::rank` remain the single
  comparison home; this ticket only hoists their inputs out of the
  comparator. The `rank()` match stays the one kind-rank definition
  (declaration order, modifier.rs:1672) — no duplicate rank table may
  be written. The test-side permutation sort at modifier.rs:7398-7403
  (`sorted_sample_lexicals`) already projects keys once and sorts the
  permutation — it is the shape this ticket brings to the production
  path, not a second implementation to reconcile.
- **Status:** NOT yet implemented — pure sparql-eval restructure; no
  new branchless kernel required (the u8 rank compare is already
  branch-cheap once hoisted; do not add a kernel for it).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** identical total order vs. the current
  `order_values` on: mixed-kind bags (all 6 ranks), equal-rank
  literal towers (numeric vs. opaque vs. composite), nested triple
  terms compared componentwise, unbound-vs-bound, and the
  `total_order_reference` test oracle at modifier.rs:9152.
- **Allocation Invariant:** projection count is exactly n
  (assertable via the existing admission/charging counters), down
  from O(n log n); no allocation inside the comparator closure.
- **Benchmark Target:** ORDER BY bench (n = 10³/10⁴ mixed-kind
  values, sort-only rung) added to the testkit bench suites;
  report-only.
- **Layer Boundary:** sparql-eval-internal change; no new layer edge;
  `python3 scripts/check-layers.py` exit 0; `make check` green.
- **Valid-neighbor (refusal mirror):** no refusals added; the mirror
  risk is a silently WRONG order — required neighbor test: a bag
  where two values share a rank but differ in value space (two
  numerics, two IRIs of different lexical forms) still orders
  exactly as the reference comparator does, and stability (equal
  keys keep input order) is preserved.

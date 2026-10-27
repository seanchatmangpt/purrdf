# [OPT-45] SPARQL-Eval Small-N Position Scans to Lane-Pack + Trailing-Zeros

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/substitute.rs:873, 906, 912`
  (`probes.iter().position(...)` + `probed.contains(&index)` in
  `probe_positions_with_memory`); `crates/sparql-eval/src/solution.rs:90`
  (`Schema::push` linear fallback), `solution.rs:353`
  (`SolutionSchemaRef::index_of` linear fallback)
- **Subsystem:** Relational Sieve
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5,
  first half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  ```rust
  let Some(index) = probes.iter().position(|(candidate, _)| candidate == var)
  ```
  (substitute.rs:873, repeated at :906 and the predicate arm :912 area),
  plus `!probed.contains(&index)` membership re-scans (:881) — executed
  per matched variable position while a quoted-triple-aware probe
  worklist walks a query's pattern positions. On the schema side,
  `self.columns.iter().position(|column| column == &variable)`
  (solution.rs:90) and `data.cols.iter().position(|column| column ==
  var)` (solution.rs:353) answer per-row column lookup for schemas below
  the `INDEXED_ABOVE` threshold.
- **Hardware-Level Impact:** `Variable` equality descends to string
  identity; each `.position()` is a scalar compare loop with a
  data-dependent branch per column/probe, re-issuing the same scan once
  per row (solution.rs:353 runs per lookup across the whole evaluation).
  For small N (SPARQL queries have 1-8 probes; schemas 1-8 columns below
  the threshold) the whole scan is one 64-bit lane pack: compare N lane
  values, `trailing_zeros`, done — no loop, no per-element branch.
- **Baseline Cost:** UNVERIFIED — `INDEX_OF_CALLS` instrumentation
  exists (solution.rs:347 test counter) but no bench rung; add an
  OPT-20 bench before landing.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** small-N (N ≤ 8) lookups become a
  **lane-pack**: stage the N keys into `[u64; 8]` (interned-variable
  ordinal, or u64 hash of the variable id where identity is not
  ordinal-for-ordinal — hash equality must be confirmed-equal via a
  single scalar re-check on the found lane to keep it a heuristic, not
  an oracle), compare all lanes against the target with
  `purrdf_branchless::simd_fallback::ct_eq_u64` (or the swar
  `find_byte_in_word` 8×u8 form for N ≤ 8 byte-comparable keys), take
  `trailing_zeros()` as the index. `probed.contains` becomes a u8
  membership mask updated by `1 << index` — O(1), branch-free. The
  schema-side scans (solution.rs:90/:353) pack the column identity
  hashes once at schema build and answer `index_of` with one compare
  + tz per lookup.
- **Hard Bounds Applied:** fixed bound N ≤ 8 per pack (fall back to the
  existing scan above 8 — the fallback is required, not optional);
  zero allocation (stack `[u64; 8]`); one confirming scalar equality
  per hit to preserve exact identity semantics.
- **Proposed Type Layout & Signatures:**
  ```rust
  // one home in purrdf-branchless (extend search.rs)
  pub fn first_equal_lane_u64x8(lanes: [u64; 8], target: u64) -> Option<u32>;
  ```
  Call sites keep their existing `Option<usize>` contract; no public
  signature changes.
- **Reuse-First:** single home is `purrdf_branchless::search` /
  `simd_fallback` (ct_eq_u64 exists, simd_fallback.rs:209; search.rs
  has `first_match_byte_u64` at search.rs:93 as the tz formulation
  pattern). No per-crate scan body may be duplicated; ledger rows
  updated for the new lane-pack job.
- **Status:** LANDED — the 8-lane OR-accumulate pack kernel is
  `purrdf_branchless::search::first_equal_lane_u64x8`
  (`crates/branchless/src/search.rs:133`), re-exported and wired in
  `crates/sparql-eval/src/solution.rs:187`. Landed, UNVERIFIED (no-build
  machine — cargo not run on this lane; gate rung still owed).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** lane-pack result equals the `.position()`
  oracle for N = 0..=8 with duplicate keys, miss, and first-hit
  semantics; `probed` mask membership equals `Vec::contains` on the
  same op sequence.
- **Allocation Invariant:** zero new allocations at all six call sites
  (staging arrays stack-resident; `probed` keeps its existing
  admitted allocation).
- **Benchmark Target:** `index_of` per-lookup bench (N = 1..=8
  columns, hit/miss) and a probe-position bench added to the OPT-20
  bench suite; report-only.
- **Layer Boundary:** sparql-eval→branchless is a NEW edge — add the
  layers.toml row and pass `python3 scripts/check-layers.py
  --home-for`; `make check` green.
- **Valid-neighbor (refusal mirror):** no refusals added; the
  confirming scalar re-check after a hash lane-hit is mandatory —
  without it a hash collision silently binds the WRONG variable, the
  exact mirror of the over-refusal bug. Required neighbor test: two
  variables with colliding u64 hashes in one probe set still resolve
  to their own ordinals.

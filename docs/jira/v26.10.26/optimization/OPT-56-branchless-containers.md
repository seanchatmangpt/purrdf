# [OPT-56] Port BitonicPriorityQueue8 and SwarQuotientFilter Containers

## 1. TARGET CONTEXT
- **Crate & File:** `crates/branchless/src/sort16.rs` (sorting-network home; ledger
  job `branchless-compare-exchange`, home `purrdf_branchless::sort_small::cswap`,
  `helpers-ledger.toml:3927`) and `crates/branchless/src/filter.rs` (filter home;
  `quotient_add` at `crates/branchless/src/filter.rs:106`)
- **Subsystem:** Memory Arena
- **Inspection Agent:** OPT-42 ticket-writer lane (bcinr handoff §3.5)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** bcinr's `BitonicPriorityQueue8` keeps an always-sorted
  `[u32; 8]`, `push` writes into lane 7 and re-sorts with a fixed bitonic network,
  `pop` reads lane 0 (`~/bcinr/crates/bcinr-logic/src/patterns/bitonic_pq.rs:28`,
  push `:49`, pop `:59`); `SwarQuotientFilter<const N>` packs 8x 8-bit fingerprints
  per u64 word with Mycroft-match `contains` (`swar_quotient.rs:26`, contains
  `:46`, insert `:57`).
- **Hardware-Level Impact:** heap-based top-k (the pattern these replace) carries
  pointer-chase sift-down branches and per-op bounds checks; the fixed-8 bitonic
  network is a straight-line cmov pipeline fully in one cache line. The SWAR
  quotient filter tests 8 candidate slots per word with zero memory loads beyond
  the word itself.
- **Baseline Cost:** 0 measured; the top-k bench in section 4 is the measurement.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Two containers, both as extensions of existing homes:
  1. `purrdf_branchless::sort16` gains a `FixedTopK8` type (bitonic-pq semantics,
     renamed to PurRDF naming) built over the ledger-sanctioned compare-exchange
     kernel — no second sorting-network body.
  2. `purrdf_branchless::filter` gains `SwarTagFilter<const N: usize>` (8x u8-tag
     quotient filter) whose `contains` reuses the Mycroft match kernel from
     `purrdf_branchless::swar` (zero-byte/match idiom) and whose `insert` uses the
     first-empty-lane mask; `quotient_add:106` stays the single-slot entry point.
- **Hard Bounds Applied:** capacity fixed at construction (`const N: usize` / fixed
  8 lanes), zero alloc after construction, all slot dispatch mask-selected, pop on
  empty returns a typed sentinel rather than panicking.
- **Proposed Type Layout & Signatures:**
  - `pub struct FixedTopK8 { data: [u32; 8] }` with
    `pub fn push(&mut self, p: u32)`, `pub fn pop(&mut self) -> Option<u32>`,
    `pub const fn len(&self) -> usize`
  - `pub struct SwarTagFilter<const N: usize> { table: [u64; N] }` with
    `pub fn contains(&self, idx: usize, tag: u8) -> bool`,
    `pub fn insert(&mut self, idx: usize, tag: u8) -> bool`,
    `pub const fn new() -> Self` (N power of two, debug_asserted)

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `FixedTopK8::push`/`pop` and `SwarTagFilter::contains`
  contain zero data-dependent forward branches (objdump on x86_64); the sort network
  is the existing `cex`-based one, byte-count of conditional moves verified.
- **Reference Invariant:** differential proptest against the bcinr references
  (`bitonic_pq.rs`, `swar_quotient.rs`): push/pop sequences match a BTreeSet-based
  oracle top-8 and membership matches a HashSet oracle.
- **Over-refusal neighbor (paired valid-neighbor test):** for the filter's
  `contains`, a full-word occupancy test (all 8 lanes occupied by distinct tags)
  must still return true for present tags and, mirror-side, an absent tag at a
  fully-occupied index must return false — over-refusal here is false negatives on
  valid members; the court runs both directions.
- **Benchmark Target:** top-k and membership benches in `purrdf_testkit::bench`
  (report-only), against a BinaryHeap/BTreeSet baseline.
- **Layer Boundary:** `check-layers.py` clean; ledger rows extended under the
  existing v26.10.26 branchless-migration block — sort16 job gains `FixedTopK8`,
  filter job gains `SwarTagFilter`; no new module, no duplicate network or match
  body (`check-shared-helpers.py` clean).

Status note: LANDED, UNVERIFIED (no-build machine) — `FixedTopK8` in
`crates/branchless/src/sort16.rs:303`, `SwarTagFilter<const N>` in
`crates/branchless/src/filter.rs:141`. **Recorded divergence:** `FixedTopK8::push`
(sort16.rs:324, `# DIVERGENCE from bcinr`) is mask-gated — a priority
`>= u32::MAX` is a no-op — whereas bcinr's unguarded push writes the sentinel as
a real priority and evicts the current max. The gate keeps the sentinel lane
reserved (bcinr `bitonic_pq.rs:42` convention); any consumer relying on
evict-on-sentinel must not port unchanged.

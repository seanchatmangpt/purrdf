# [OPT-55] Extend SWAR Home with Horizontal Reduce/Scan Kernels

## 1. TARGET CONTEXT
- **Crate & File:** `crates/branchless/src/swar.rs` (home; currently 19 pub kernels,
  ledger job `branchless-swar-word-kernels`, `helpers-ledger.toml:3869`) — new
  horizontal family appended, no new module
- **Subsystem:** Ingestion
- **Inspection Agent:** OPT-42 ticket-writer lane (bcinr handoff §3.5)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** bcinr's `reduce.rs` carries a family of horizontal
  SWAR reducers — `horizontal_or_u32:23`, `horizontal_and_u32:46`,
  `horizontal_xor_u32:69`, `horizontal_max_u8x8:159`, `horizontal_min_u8x8:186`,
  `swar_horizontal_sum:212`, `swar_horizontal_max_u8:240`, `swar_count_eq_u8:280`,
  plus slice-level `reduce_min_u32:315`, `reduce_max_u32:342`, `reduce_sum_u64:369`
  (`~/bcinr/crates/bcinr-logic/src/reduce.rs`, 742 lines). PurRDF's
  `purrdf_branchless::swar` has match/classify/parse kernels but no horizontal
  reduce family.
- **Hardware-Level Impact:** horizontal reductions written per-callsite as unrolled
  `v0 op v1 op ...` chains re-derive the pairwise tree each time; a fixed-bound
  family keeps the tree in one audited body with deterministic latency and no
  loop-carried branch.
- **Baseline Cost:** 0 measured — bench in section 4 is the measurement.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Append the horizontal family to the existing SWAR
  home: per-word reducers over 8x u8 lanes (sum, min, max, count-eq, or/and/xor
  broadcast) and slice-level reducers over `&[u32]`/`&[u64]` with a fixed unroll
  factor (4-way tree, masked tail). Slice-level kernels reuse the word kernels —
  no second body.
- **Hard Bounds Applied:** fixed 4-way unroll bound, masked tail (no per-element
  branch on the hot loop), zero-alloc, all lane compares via the existing
  `branchless-mask-minmax-select` select idiom.
- **Proposed Type Layout & Signatures:**
  - `pub const fn horizontal_sum_u8x8(w: u64) -> u32`
  - `pub const fn horizontal_min_u8x8(w: u64) -> u8` /
    `pub const fn horizontal_max_u8x8(w: u64) -> u8`
  - `pub const fn horizontal_count_eq_u8(w: u64, t: u8) -> u32`
  - `pub const fn horizontal_or_u64(w: u64) -> u64` (lane-or broadcast check)
  - `pub fn reduce_min_u32(s: &[u32]) -> u32`,
    `pub fn reduce_max_u32(s: &[u32]) -> u32`,
    `pub fn reduce_sum_u64(s: &[u32]) -> u64`

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** per-word kernels are `const fn`, branch-free (proptest
  vs the bcinr references in `reduce.rs` test modules); slice kernels have exactly
  one branch (the loop bound), no per-lane branches.
- **Allocation Invariant:** no allocation on any input length (alloc-probe run in
  the ticket's court).
- **Over-refusal neighbor:** the empty-slice case is the valid neighbor — the
  slice reducers must return a defined identity (`u32::MAX`/`u32::MIN`/0) rather
  than panicking on `&[]`; bcinr's `reduce_min_u32` documents the same identity
  contract (`reduce.rs:315`). A kernel that refuses an empty slice (panic/None
  where identity is defined) fails the court.
- **Benchmark Target:** extend the `purrdf_testkit::bench` primitives suite
  (OPT-20 lineage) with the horizontal family; report-only.
- **Layer Boundary:** `check-layers.py` clean; ledger job
  `branchless-swar-word-kernels` gains the new entry_points — home stays
  `purrdf_branchless::swar::find_byte_in_word`, no new job row, no duplicate body
  (`check-shared-helpers.py` clean).

Status note: LANDED, UNVERIFIED (no-build machine) — the horizontal family is in
`crates/branchless/src/swar.rs` (`horizontal_and_u32` at swar.rs:632 et al.).
**Recorded contract deviation:** `horizontal_and_u32(&[]) == 0` is the landed
behavior (pinned, swar.rs:1088), diverging from the mathematical identity of
AND-reduce over the empty set (all-ones). This matches the bcinr-contract choice
to return the zero identity rather than `u32::MAX`; consumers that need the
set-identity must guard the empty case themselves — the pin is the spec, not an
accident.

# [OPT-32] Guaranteed-cmov Sorting Networks; Wire the Dormant Quad-Lane Sort

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/primitives.rs:16-38`; `crates/datalog/src/seminaive/cycle.rs:280-321`; `crates/hnsw/src/select.rs:130,170`, `search.rs:496-574`; `crates/rdf-core/src/describe.rs:383-386`, `dataset_view.rs:1403,1424`; `crates/sparql-eval/src/property_fn_eval.rs:3433,7432`
- **Subsystem:** Relational Sieve / Retrieval
- **Inspection Agent:** Agent 12 (Branchless Comparator & Sorting Networks)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `sort_quad_lanes_branchless([u64;4])` exists (optimal 5-comparator Batcher odd-even network, primitives.rs:16-38) but **has no production call site** (only `primitives_hotpath_tests.rs:32`), and its `cswap!` macro uses a plain `if` + `let tmp` swap — LLVM usually but not always lowers it to cmov; the guaranteed form is `min`/`max` or `select_unpredictable`. Branchy hot small-N sorts: leapfrog alignment `let mut target = cursors.iter().filter_map(Cursor::current).max()?` + `if value > target { target = value; … }` (cycle.rs:288,296-300; N = Frame arity, default 4); HNSW `chosen.sort_unstable()` at cap ≤ 16 (`select.rs:130`; beam pre-check :67-71); heap admission branch chain `results.len() < ef || results.peek().is_some_and(|worst| score < *worst)` (`search.rs:565`); quad-key canonical order `ordered.sort_unstable_by_key(|quad| (quad.g, quad.s, quad.p, quad.o))` (describe.rs:383) — exactly the case the network was written for; 2-4 element sorts at property_fn_eval.rs:3433,7432.
- **Hardware-Level Impact:** generic `sort_unstable` call overhead + data-dependent branches on ≤16 elements; branch mispredictions in the leapfrog align loop and heap admission decide-push.
- **Baseline Cost:** generic sort on N=4-16 vs a fixed 5/19/63-comparator cmov network.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) fix `cswap!` to guaranteed-cmov (`(lo, hi) = (a.min(b), a.max(b))` or `core::intrinsics::select_unpredictable`), then add `sort8_lanes_branchless` (19 comparators) and `sort16_lanes_branchless` (63-comparator Batcher odd-even mergesort) beside the existing 4-lane in `primitives.rs`. (2) Wire `sort_quad_lanes_branchless` into describe.rs:383 (pad lanes to 4 with `u64::MAX`, run the network, strip sentinels) and the small dataset_view/describe sorts when input is known-small (1-8 terms). (3) Leapfrog: branchless max-of-4 via chained `TermId::max` (cmov on u64) or a 4-lane network on `(TermId, cursor_idx)` lanes; the all-of-N `Intersection::contains` chain (cycle.rs:310-321) becomes N predicated compares. (4) HNSW: 16-lane network on `(distance_bits, row)` packed into u64 keys (ordered-float bit trick: `!(bits as u64)` for positive, `bits | 0x8000…` for negative flips float order to integer order); keep `sort_unstable` only for the rarer >16 beam. (5) Heap admission (search.rs:565): replace decide-push-or-not with a predicated swap of `(worst, score)`; the early-exit `current > worst` is a cmov min. (6) ORDER BY comparator collapse: pre-compute a ranked u64 ordinal per row (class rank in high bits, in-class key ordinal in low bits) during the existing key-projection pass (modifier.rs:658-740), so the sort comparator collapses to one u64 compare and the branchy `ValueClass` tower (modifier.rs:1401-1600) runs only in projection; `width ≤ 16` keys compare via predicated min/max fold, not early-return loops (cross-ref OPT-22 for the saturating helpers).
- **Hard Bounds Applied:** networks fixed at N=4/8/16; >16 falls back to `sort_unstable`; f64→u64 key packing is total-order-correct.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub fn sort8_lanes_branchless(lanes: [u64; 8]) -> [u64; 8];   // 19 comparators, cmov
  pub fn sort16_lanes_branchless(lanes: [u64; 16]) -> [u64; 16]; // 63 comparators, cmov
  fn packed_distance_key(score: f64, row: u32) -> u64;           // order-preserving pack
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** networks emit zero conditional jumps on the compare path (`cmov` only — verify via asm inspection on the bench target).
- **Allocation Invariant:** 0 (register/stack lanes only).
- **Benchmark Target:** 16-lane network ≥ 3× vs `sort_unstable` at N=16; leapfrog alignment round ≤ 2 ns at arity 4; quad-key sort beats generic at N=4 (OPT-20 suite).
- **Layer Boundary:** kernels in `purrdf-core::primitives` / `purrdf-hnsw`; `check-layers.py` clean; differential test that network output equals `sort_unstable` on adversarial inputs (duplicates, sentinels).

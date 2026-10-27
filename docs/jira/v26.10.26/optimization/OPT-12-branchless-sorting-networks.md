# [OPT-12] Branchless Sorting: Guaranteed-cmov cswap, 8/16-Lane Networks, Ranked-u64 ORDER BY

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/primitives.rs:16-38`; `crates/datalog/src/seminaive/cycle.rs:280-321`; `crates/hnsw/src/select.rs:130,170`; `crates/hnsw/src/search.rs:496-574`; `crates/sparql-eval/src/modifier.rs:658-740,1963`; `crates/sparql-eval/src/cdt_agg.rs:479-495`; `crates/sparql-eval/src/property_fn_eval.rs:3433,7432`; `crates/rdf-core/src/describe.rs:383,386`; `crates/rdf-core/src/dataset_view.rs:1403,1424`
- **Subsystem:** Relational Sieve | Branchless Primitives
- **Inspection Agent:** Agent 12 (Branchless Comparator & Sorting Networks)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** The 4-lane kernel exists but is not actually branchless — `cswap!` (primitives.rs:19-28) is a plain `if` with `let tmp` swap; LLVM lowers it to cmov only opportunistically. `sort_quad_lanes_branchless` (primitives.rs:16-38, optimal 5-comparator Batcher odd-even) has **zero production callers** (tests only). Hot scalar sites:
  - Leapfrog alignment: `let mut target = self.cursors.iter().filter_map(Cursor::current).max()?;` (cycle.rs:288) plus branchy align loop `if value > target { target = value; aligned = false; }` (cycle.rs:296-300); `Intersection::contains` (cycle.rs:310-321) is an all-of-N branch chain.
  - HNSW: `chosen.sort_unstable();` (select.rs:130), `beam.sort_unstable();` (select.rs:170) on exactly ≤16 `Ranked {f64, usize}` items (cap = M/M0, default 16, params.rs).
  - Heap admission branch chain: `let admitted = results.len() < ef || results.peek().is_some_and(|worst| score < *worst);` (search.rs:565) plus pop/break chain (search.rs:554).
  - ORDER BY: permutation sort comparator runs the branchy `total_order` → `ValueClass` rank-then-compare tower per comparison (modifier.rs:1963; eval_order_by modifier.rs:658-740; aggregate ORDER BY cdt_agg.rs:479-495).
  - Quad-key canonical ordering: `ordered.sort_unstable_by_key(|quad| (quad.g, quad.s, quad.p, quad.o));` (describe.rs:383) — exactly the 4-lane case the network was written for; small object/reifier sets `objects.sort_unstable()` (dataset_view.rs:1403,1424), `ordered_reifiers.sort_unstable()` (describe.rs:386).
  - 2–4 element sorts: `merged.sort_by_key(|(call, _)| *call);` (property_fn_eval.rs:3433), `slots.sort_by_key(|(_, slot)| *slot);` (property_fn_eval.rs:7432).
  - `clamped_scaling_u64` (primitives.rs:41-48) uses `checked_mul` + match — branchy.
- **Hardware-Level Impact:** branch misprediction (~15–20 cycles) per data-dependent compare; generic `sort_unstable` call overhead on 2–16 element inputs where a fixed network is 5–63 unconditional cmov pairs; ORDER BY tower = multiple dependent branches per comparator invocation, O(n log n) times.
- **Baseline Cost:** generic introsort on 16 elements ≈ 60+ compares with branches; 16-lane Batcher = 63 branchless comparators, fully pipelined.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. **Fix the kernel first**: rewrite `cswap!` to the branchless house form **verbatim** — the XOR-mask compare-exchange already landed as `sort_small::cswap` (`crates/branchless/src/sort_small.rs:24-29`):
     ```rust
     let mask = (a[i] > a[j]) as u32;
     let diff = (a[i] ^ a[j]) & 0u32.wrapping_sub(mask);
     a[i] ^= diff;
     a[j] ^= diff;
     ```
     The `min`/`max`-pair form originally proposed below is **superseded** — the XOR-mask form is the guaranteed-cmov shape the adversarial scan pinned (`sort_small.rs:16-22` doc names it "the guaranteed-cmov form `purrdf-core::primitives::cswap!` should adopt (see OPT-12)"). `sort_small::cswap` is `pub(crate)`, so `crates/rdf-core/src/primitives.rs:19-27` adopts the form verbatim (single-home note: the one mutable home stays `sort_small`; if visibility is ever widened, primitives.rs re-exports instead of keeping a second body). Current offending macro body (primitives.rs:19-27): plain `if lanes[$a] > lanes[$b] { let tmp = ... }` — LLVM lowers to cmov only opportunistically. Add `sort8_lanes_branchless` (19 comparators) and `sort16_lanes_branchless` (63 comparators, Batcher odd-even mergesort) beside the existing 4-lane network in `primitives.rs` (or delegate to `purrdf::branchless::{sort_small, sort16}`).
  2. **Wire the 4-lane network**: describe.rs:383 — pack `(g, s, p, o)` into 4 `u64` lanes, pad to 4 quads with `u64::MAX` sentinels, run the network, strip sentinels; use for small object/reifier sets (dataset_view.rs:1403/1424) when len ≤ 4 (dispatch on len, fall back to `sort_unstable` above).
  3. **Leapfrog alignment** (cycle.rs:280-304): compute `target` via chained `TermId::max` (compiles to cmov); per alignment round, fixed 2/3/4-comparator network (arity is the `Frame<I, const N: usize = 4>` constant, join.rs:149); `Intersection::contains` (cycle.rs:310-321) becomes N predicated compares (accumulate a `keep` mask, no early return).
  4. **HNSW beam** (select.rs:130,170): 16-lane network over `(distance_bits, row)` packed into one u64 key — flip float order into integer order (`Ordered`-float encoding: `!(bits as u64)` for non-negative, `bits as u64 | 0x8000_0000_0000_0000` for negative); keep `sort_unstable` fallback for beams > cap.
  5. **Heap admission** (search.rs:496-574): replace the decide-push-or-not branch chain with a predicated swap of `(worst, score)` — branchless top-1-of-≤ef; the early-exit `current > worst` test stays (control flow, not per-element).
  6. **ORDER BY comparator collapse** (modifier.rs:658-740, 1963; cdt_agg.rs:479-495): during the existing key-projection pass, pre-compute a **ranked u64 ordinal per row** — `ValueClass` rank in high bits, in-class key ordinal in low bits (fixed-width keys, width ≤ 16 compared via predicated min/max fold, not early-return loop). The permutation sort comparator collapses to one u64 compare; the branchy `total_order` tower survives only inside the projection pass (once per row, not per comparison).
  7. **Micro-sorts** (property_fn_eval.rs:3433, 7432): 2-comparator / 3-comparator insertion networks, guaranteed cmov, allocation-free.
  8. **`clamped_scaling_u64`** → branchless saturating form: `(value as u128 * scale as u128).min(u64::MAX as u128) as u64` (or `saturating_mul` + floor/ceiling with `ceiling.max(floor)` to keep `clamp` panic-free); datalog already models the idiom (seminaive.rs:400,3574 `saturating_mul`/`.max()`).
  9. **Seek side**: reuse `search_eytzinger_u64` (`crates/rdf-core/src/eytzinger.rs:26-49`) for cursor/merge seeks — the branchless `(eytzinger[k] < target) as usize; k = 2*k + cmp` descent is the canonical predicated-index pattern.
- **Hard Bounds Applied:** networks apply only at fixed N ∈ {2,3,4,8,16}; inputs above the lane count fall back to `sort_unstable` (loud fallback, not silent degradation of the invariant); ORDER BY rank ordinals assume fixed key width ≤ 16 per row.
- **Proposed Type Layout & Signatures:**
  ```rust
  #[inline(always)] const fn cswap(lanes: &mut [u64; N], i: usize, j: usize) {
      // XOR-mask form, verbatim from purrdf::branchless::sort_small::cswap
      // (crates/branchless/src/sort_small.rs:24-29) — u64-widened:
      let mask = (lanes[i] > lanes[j]) as u64;
      let diff = (lanes[i] ^ lanes[j]) & 0u64.wrapping_sub(mask);
      lanes[i] ^= diff;
      lanes[j] ^= diff;
  }
  pub fn sort8_lanes_branchless(mut lanes: [u64; 8]) -> [u64; 8];    // 19 comparators
  pub fn sort16_lanes_branchless(mut lanes: [u64; 16]) -> [u64; 16]; // 63 comparators
  /// distance packed so integer order == float order
  fn pack_distance(score: f64, row: usize) -> u64;
  /// per-row sort ordinal: class rank << 56 | key ordinal (width <= 16)
  fn ranked_ordinal(keys: &[KeyValue], order_by: &[OrderClause]) -> u64;
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** network bodies emit zero conditional jumps (cmov/select only) — verify in release asm (`cargo asm` spot-check on `sort16_lanes_branchless`); `Intersection::contains` accumulation form has no early-return branch.
- **Allocation Invariant:** all networks operate on fixed arrays in registers/stack; 0 heap allocations; ORDER BY ordinals computed into the existing projection buffers.
- **Benchmark Target:** 16-lane network < `sort_unstable` by ≥2× at N=16 (`Throughput::Elements`); ORDER BY permutation sort ≥1.5× on 1k-row suites; add all to the `primitives` bench suite (OPT-20) — per repo law, no speedup asserted without a bench.
- **Layer Boundary:** kernels live only in `crates/rdf-core/src/primitives.rs` (+ eytzinger reuse); consumers in datalog/hnsw/sparql-eval reference, never re-implement; `check-layers.py` and `check-shared-helpers.py` clean.
- **Correctness differential:** each converted site keeps a differential test vs. the previous scalar sort (in-repo precedent: `merge_intersect_rows` oracle, `leapfrog_and_binary_joins_agree`, seminaive.rs:7616). Ordered-float packing must round-trip `PartialOrd` semantics for `f64` including ±0.0 and infinities (test).
- **Implemented in `purrdf::branchless::{sort_small, sort16}`** (crate `crates/branchless`, ported from
  BranchlessCInRust): XOR-mask compare-exchange house form, 8-lane bitonic + optimal n=5..8
  networks, and the true `[u32; 16]` Batcher network.

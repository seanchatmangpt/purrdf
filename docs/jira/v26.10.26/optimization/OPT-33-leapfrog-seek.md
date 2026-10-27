# [OPT-33] Branchless Leapfrog Seeks: Port the Datalog Driver Seam to SPARQL and Pack

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/ir/pack/triples.rs:750-782,884-1008,912-945,1536`; `crates/datalog/src/cursor.rs:458-489,498`; `crates/datalog/src/seminaive/cycle.rs:282-330,390`; `crates/sparql-eval/src/bgp.rs:588-912,639-651,707-737`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 14 (Bitset-Parallel Frontier & Leapfrog Joins); Agent 03 (seek variants)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** SPARQL BGP evaluation is pure index-nested-loop over six `u32`-ordinal permutation arrays — per stage, per input row: `let plan = plan.unwrap_or_else(|| ctx.dataset.probe_plan(...)); for quad in ctx.dataset.quads_for_pattern_with_plan(&plan, s, p, o, *gm) { … bind_row(row, cp, &quad, ctx) … }` (bgp.rs:707-737, plan reuse :639-651) — each pattern probed independently, no multiway intersection. A real leapfrog triejoin exists only in the datalog engine: `ValueCursor::seek` (cursor.rs:458-489, doc :498 "supports the `seek` a leapfrog intersection drives") driven by `cycle.rs:282-330` (seek-to-max + all-seek consensus + backtracking :390), but its `Range`-run seek is a branchy `binary_search`/`partition_point`. Pack layer: `local_lookup`/`binary_search_range` branch chains (triples.rs:750-782, called at 884-1008).
- **Hardware-Level Impact:** re-probing the permutation index once per row serializes dependent cache misses per join stage; branchy binary-search seeks mispredict on skew.
- **Baseline Cost:** per-row index probes O(rows × pattern) vs O(total seek distance) for leapfrog; branchy seeks vs cmov descents.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) make the datalog `Range`-run seek branchless via the OPT-03 `lower_bound_eytzinger` extension or a galloping/cmov lower bound (cursor.rs:460-473); wire `search_eytzinger_u64` where exact-match fits. (2) Port the leapfrog driver (cycle.rs:282-330 seek-to-max/consensus/backtrack pattern + `ValueCursor::seek` seam) to SPARQL BGPs as an alternative kernel for highly-connected patterns, where `QuadProbePlan` reuse already provides the loop-invariant plan a multiway intersection needs; guarded by the existing differential-test pattern — `merge_intersect_rows` (triples.rs:1536) is the kept-as-oracle precedent and `leapfrog_and_binary_joins_agree` (seminaive.rs:7616) the in-repo differential model. (3) Pack `local_lookup`/`binary_search_range` seek chains route through the same branchless lower bound (IntVector accessor form per OPT-03). Bitset encoding of FoQ posting lists is a separate decision (cross-ref OPT-13); this ticket is the seek/join algorithm layer only.
- **Hard Bounds Applied:** cursor count = join arity (≤ 4 in `Frame<I, const N: usize = 4>`); seek consensus bounded by run lengths; no new indexes required.
- **Proposed Type Layout & Signatures:**
  ```rust
  trait SeekCursor { fn seek(&mut self, target: u64) -> Option<u64>; }  // branchless impl for Range/Perm/Tail
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** seek descents are cmov (OPT-03 kernel); driver loop has no data-dependent branch per candidate beyond the consensus compare.
- **Allocation Invariant:** 0 allocations in the join loop (cursor state on stack).
- **Benchmark Target:** highly-connected BGP court (WatDiv/LUBM class) shows join-stage probe count ↓ vs index-nested-loop baseline; differential agreement test mandatory before promotion.
- **Layer Boundary:** `check-layers.py` clean; the SPARQL port reuses datalog driver design as reference implementation — no second triejoin body in `helpers-ledger.toml` terms.

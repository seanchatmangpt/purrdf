# [OPT-34] Wire `bitset_intersect_u64` Into the Pack FoQ Intersection

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/primitives.rs:38-41` (dead primitive); `crates/rdf-core/src/ir/pack/triples.rs:912-945,1536`; `crates/rdf-core/src/ir/pack/bits.rs:190,419,715`; `crates/rdf-core/src/ir/dataset.rs:694-800`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 14 (Bitset-Parallel Frontier)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `pub fn bitset_intersect_u64(chunk_a: u64, chunk_b: u64) -> u64 { chunk_a & chunk_b }` (primitives.rs:38-41) — **zero production callers**, referenced only by `primitives_hotpath_tests.rs:39-40`. The pack `(?,p,o)` intersection decodes the shorter FoQ delta list and test-filters each entry: `if o_count <= p_count { Box::new(foq_list(...).filter(move |&pos| part.sp.get(pos as usize) == lp)…) } else { … binary-search each So slice … }` (triples.rs:912-945); the retired merge-intersect survives only as the test oracle (`merge_intersect_rows`, triples.rs:1536). The comment records the measured cost story: "on the WatDiv and LUBM query sets one list was at least 32 times the other in 1,090 of 1,520 calls" — full-list decode is the thing to avoid. `RankSelect`/`BitVec` with raw `words: Vec<u64>` (bits.rs:190,419,715) already exist — the natural word-AND carrier — but no word-wise AND routine spans them.
- **Hardware-Level Impact:** per-entry delta-decode + `sp.get` test serializes on the dense-side list length; word-parallel AND processes 64 candidates/cycle.
- **Baseline Cost:** O(shorter-list decode + per-entry test) vs O(words/64) cycles.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) bitset-encode FoQ position lists (per predicate/object local id, or per hot (p,o) pair) as word arrays over the `Sp`-position/local-id domain, so `(?,p,o)` becomes `bitset_intersect_u64` over precomputed words instead of decode-plus-filter; this is an auxiliary eager side-index or format v2 (`TRIPLES_FORMAT_VERSION = 1`, triples.rs:142) — the measured 32× asymmetry is the evidence the decode is the cost, and the shorter-list probe must remain the fallback when a side is too sparse to bitset (sparsity threshold recorded as a constant). (2) Where conversion is already free: word-wise AND of two `RankSelect` word spans answers `(s,?,o)`-style set meets with zero re-encoding. (3) `QuadKey::chunk_matches` (dataset.rs:694-800): the 8-row `[u32;4]` key/mask kernel is already SIMD-shaped and branchless — optionally add dense-subject `u64` word lanes for reuse-heavy stages; `binding_pattern.rs:330` (`a.bound & b.bound`) is the semantic precedent. (4) Keep the differential oracle: any bitset rewrite is guarded by the kept `merge_intersect_rows` + current probe as dual oracles, per the house pattern.
- **Hard Bounds Applied:** bitset only when density exceeds threshold (sparse side stays probe); domain = local id space per partition; word count ≤ partition cardinality/64.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct PostingWords<'a> { words: &'a [u64], base: u32 }
  impl PostingWords<'_> { pub fn intersect(self, other: Self) -> impl Iterator<Item = u32>; } // word-AND + trailing_zeros
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** word-AND lane is 1 cycle/word; candidate iteration via `trailing_zeros` (branchless per bit).
- **Allocation Invariant:** intersect yields without materializing result vectors (lazy word walk).
- **Benchmark Target:** WatDiv/LUBM `(?,p,o)` class queries: intersection cost tracks the shorter *word count*, not the shorter list length; dual-oracle differential test passes.
- **Layer Boundary:** encoding stays inside `ir/pack` (ring-fenced); `check-layers.py` clean; on-disk format change (if taken) bumps `TRIPLES_FORMAT_VERSION` with golden-byte coverage per the frozen-corpus rules.

# [OPT-03] Eytzinger Lookup Wiring + lower_bound/Prefetch Extension

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/eytzinger.rs:11-51` (kernel home, zero production callers today); wiring sites: `crates/rdf-core/src/ir/pack/triples.rs:200-206,765-781,884-1008`; `crates/rdf-core/src/ir/dataset.rs:55,1107,1929,2019,2118-2131`; `crates/rdf-core/src/ir/triple.rs:413-418,127-132`; `crates/rdf-core/src/ir/statistics.rs:221-224,378-381,799,842`; `crates/datalog/src/store.rs:672-703`; `crates/rdf-core/src/evidence/dictionary.rs:116,125,153`
- **Subsystem:** Ingestion
- **Inspection Agent:** Agent 03 (Eytzinger Dictionary Resolution)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `s_set.binary_search(&s)`, `p_set.binary_search(&p)`, `o_set.binary_search(&o)` per triple in `encode_partition` (triples.rs:200-206).
  - Hand-rolled mid-point `binary_search_range(IntVectorRef, …)` over bit-packed `IntVector` dictionaries (triples.rs:765-781), called at `:884, :886, :892, :901, :940, :1004, :1008` — the hottest read path in pack storage (BGP/spo probe).
  - `self.locations.binary_search_by_key(&handle, |(stored,_)| *stored)` (dataset.rs:55, macro-expanded); `self.named_graphs.binary_search(&graph).is_ok()` (dataset.rs:1107); `reifiers.partition_point(|(r,_,_)| *r < reifier)` lo/hi pairs (dataset.rs:1929, 2019, 2118-2119, 2130-2131; triple.rs:413-418, 1860-1861).
  - `partition_point`/`binary_search_by_key` over relation/predicate statistics tables (statistics.rs:221-224, 378-381).
  - Datalog join probes: `subjects[lo..].partition_point(|&x| x <= s)` + `run.binary_search(&o)` + `perm.partition_point` (store.rs:672-703).
  - Sorted-Vec dictionary with `pairs.insert(position, …)` memmove (evidence/dictionary.rs:116, 125, 153).
- **Hardware-Level Impact:** every probe is log₂(n) *dependent* cache misses (each level's line address depends on the previous compare — no prefetchability); 3 serialized searches per encoded triple (F1); bit-unpack + miss per level on the packed-vector path (F2). Binary search is the canonical worst case for the memory wall.
- **Baseline Cost:** log₂(n) serialized misses per lookup (≈20 dependent accesses at n=10⁶); vs. Eytzinger's ~log₂(n) *independent* misses that hardware prefetch/ILP can overlap, with branchless `cmov` descent.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Reuse and extend `crates/rdf-core/src/eytzinger.rs` (do NOT duplicate the kernel — one home per job):
  1. **Wire existing kernel** at Tier-1 u64 exact-match sites: pack encode sets (triples.rs:200-206 — local id = layout position is acceptable since sets are build-local; verify `bits_for` width consistency or keep a permutation), named-graphs membership (dataset.rs:1107), datalog subject-run probes (store.rs:672-703), statistics composite keys packed to a single u64.
  2. **Extend `eytzinger.rs`** (the only sanctioned home) with:
     - `lower_bound_eytzinger(&[TermId], u64) -> usize` — branchless descent returning the *sorted index* of the first element ≥ target; serves all `partition_point` lo/hi range pairs (reifiers, triple.rs:127-132, statistics ranges) as `[lb(x), lb(x+1))`.
     - Generic-key `search_by` / `lower_bound_by` accessor form for non-`Vec<u64>` storage (packed `IntVector` accessors, permutation-indirected arena compares at evidence/ledger/terms.rs:252,350,388).
     - `#[inline] unsafe fn prefetch_read(addr: *const u64)` — `core::arch` cfg-gated (`x86_64::_mm_prefetch`, aarch64 `prfm`), no-op fallback. Workspace grep confirms zero prefetch today.
     - Document the 1-based layout-position return semantics of `search_eytzinger_u64` and add a `layout_to_sorted` permutation helper (X6).
  3. **Packed-vector path (F2):** `IntVector` is not addressable in place — build a side Eytzinger u64 array at partition open, or a 1-level sampled-pivot index (B-tree-style cache block); do NOT reuse exact-match directly.
  4. **Mutating dictionaries (X4):** evidence/dictionary.rs keeps its insert path; add a lazily-built Eytzinger read mirror (dirty-flag rebuild after batch inserts). Read-path-only via `OnceCell` mirror — every call site is `&self` with `Vec`-backed storage, so no public API changes (X5).
- **Hard Bounds Applied:** Eytzinger arrays built at seal/freeze only; no incremental/insert variant; mutable dictionaries get dirty-flag rebuild, never in-place layout mutation.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub fn lower_bound_eytzinger(eytzinger: &[TermId], target: u64) -> usize; // sorted index of first >= target
  pub fn search_by<K: ?Sized>(eytzinger: &[TermId], key: &K, f: impl Fn(&TermId, &K) -> Ordering) -> Option<usize>;
  #[inline] pub unsafe fn prefetch_read(addr: *const u64); // cfg-gated, no-op fallback
  pub fn layout_to_sorted(n: usize) -> Box<[u32]>; // permutation
  ```
- **Tier-2 follow-ons (adapted keys, lower priority):** embedding `binary_search_ids` over 32-byte digests (view.rs:402 → :4794) — first-8-bytes u64 prefilter Eytzinger + exact confirm, or generic 32B layout; text partition/term dictionaries (index.rs:1142,1421,1448,1461,1726,1734,2081) — u64 hash/prefix side table; OWL DL role tables + adjacency (roles.rs:333,567,1025,1073,1135-1136; graph.rs:812,1076,3014-3015; hyper.rs:1467,3468-3469 — `:733` adjacency membership should be a bitset, not a search).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** descent loops compile to `cmov` (`k = 2k + (a[k] < target)` predicated index); zero data-dependent branches in `search_eytzinger_u64` / `lower_bound_eytzinger`.
- **Allocation Invariant:** layouts built once at seal into `Box<[TermId]>`; probe paths allocate 0.
- **Benchmark Target:** new `crates/rdf-core/benches/` eytzinger suite (see OPT-20): search vs. `binary_search`/`partition_point` at 10³–10⁷ keys, hit/miss/skew patterns; target ≥2× on n ≥ 10⁵ (prefetch-overlapped), no regression at n < 64.
- **Layer Boundary:** kernel stays in `purrdf-core`; `check-layers.py` 270 edges remain clean. Differential test: eytzinger results equal `binary_search`/`partition_point` oracle over randomized corpora, including n=0, n=1, non-power-of-2, duplicate keys (extends `crates/rdf-core/tests/primitives_hotpath_tests.rs`).
- **Implemented in `purrdf::branchless::eytzinger`** (crate `crates/branchless`, ported from
  BranchlessCInRust-compatible shape): the single home for the layout builder, branchless search,
  `lower_bound_eytzinger`, `search_by`, and layout permutation.

> **Implementation note (v26.10.26):** `purrdf::branchless::eytzinger` landed the lower bound
> with a **subtree-size array** (`build_subtree_sizes(len, &mut [u32])`, length n+1; going
> right at node k contributes `S[2k]+1`) rather than a rank-of-last-left-turn array — the
> originally sketched rank form was falsified by the linear-oracle differential. Also landed:
> `build_layout_permutation` / inverse (index-0 sentinel), `search_by` accessor, and a safe
> no-op `prefetch_read` (advisory-only; audited-lift home documented — the `unsafe`
> `core::arch` form above was dropped to keep the crate `forbid(unsafe_code)`). rdf-core
> switchover/re-export remains open.

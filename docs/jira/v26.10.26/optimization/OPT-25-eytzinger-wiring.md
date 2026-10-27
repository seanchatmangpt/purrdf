# [OPT-25] Wire Eytzinger Search Into Dictionary Lookups; Add lower_bound + Prefetch

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/eytzinger.rs` (exists, zero production callers); top sites `crates/rdf-core/src/ir/pack/triples.rs:200-206,765-781,884-1008`; `crates/rdf-core/src/ir/dataset.rs:55,1107,1929,2019,2118-2131`; `crates/datalog/src/store.rs:672-703`
- **Subsystem:** Relational Sieve / Memory Arena (dictionary resolution)
- **Inspection Agent:** Agent 03 (Eytzinger Dictionary Resolution)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `s_set.binary_search(&s)`, `p_set.binary_search(&p)`, `o_set.binary_search(&o)` per triple in `encode_partition` (triples.rs:200-206); hand-rolled `binary_search_range` over a bit-packed `IntVector` (triples.rs:765-781), called at :884, :886, :892, :901, :940, :1004, :1008 on the BGP/spo probe path — the hottest read path in pack storage; `binary_search_by_key` in the `source_location_method!` macro (dataset.rs:55); `named_graphs.binary_search(&graph).is_ok()` (dataset.rs:1107); `reifiers.partition_point` lo/hi pairs (dataset.rs:1929, 2019, 2118-2119, 2130-2131; triple.rs:413-418); datalog join probes `subjects[lo..].partition_point` + `run.binary_search(&o)` + `perm.partition_point` (store.rs:672-703).
- **Hardware-Level Impact:** each probe is log₂(n) dependent cache lines, fully serialized; the IntVector variant adds a bit-unpack per level; no prefetching anywhere (workspace grep: zero hits).
- **Baseline Cost:** 3 × log₂(n_terms) miss-serialized cache lines per triple on the encode path; one serialized miss chain per BGP probe per run.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** extend `eytzinger.rs` (do not re-implement the core loop): (1) `lower_bound_eytzinger(&[TermId], u64) -> usize` (branchless descent, returns sorted-index of first ≥ target) — the dominant idiom is `partition_point` lo/hi pairs; (2) a generic-key `search_by` accessor form for `IntVector`/arena/string keys; (3) an `#[inline] unsafe fn prefetch_read(addr)` (`core::arch` x86_64 `_mm_prefetch`, cfg-gated no-op fallback) — first prefetch in the workspace; (4) a `layout_to_sorted` permutation helper and a doc fix stating `search_eytzinger_u64` returns the 1-based layout position, not the payload index. Conversion targets, in order: pack triples encode sets (build once per partition in Eytzinger layout; verify local-id numbering vs `bits_for(n_p-1)` widths or keep a permutation); the packed-IntVector probe path (sampled-pivot eytzinger index or side u64 array at partition open — never reuse exact-match directly); dataset locations (`+ permutation`), named-graphs membership (exact-fit, rebuild on seal), reifier ranges (two branchless descents: [lb(x), lb(x+1))); statistics composite key packed to u64; datalog subject runs at seal. Tier 2 (key adaptation): embedding 32-byte digests (first-8-bytes u64 prefilter + 32-byte confirm, `ir/embedding/view.rs:402,4794`), text partition/term dictionaries (side u64 hash/prefix table, `text/src/index.rs:1142,1421,1448,1461,1726,1734,2081`), OWL DL role tables and adjacency (`entail/src/owl_dl/roles.rs:333,567,733` — at :733 a plain bitset beats any search), `graph.rs:812,1076,3014-3015`, `hyper.rs:1467,3468-3469`, datalog prefix scans (`store/index.rs:170-173`), ledger terms indirection (`evidence/ledger/terms.rs:252,350,388`), `ir/triple.rs:127-132`, `shapes/src/class_membership.rs:79,105,362,581`, `sparql-eval/src/witness.rs:174,386,529,564`. Sorted-Vec-with-`insert` dictionaries (`evidence/dictionary.rs:116,125,153`) get read-path-only Eytzinger with a dirty-flag rebuild — never convert the insertion-position path. All sites are `&self` + `Vec`-backed: a lazily-built mirror (rebuilt on seal/mutation) slots in without API change. NOT a target: `ir/builder.rs:320` `Interner` (HashTable, already cache-decent); static small tables.
- **Hard Bounds Applied:** lookup ≤ log₂(n) level-descents with prefetch-distance-1; keys ≤ u64 (composite keys packed); mirrors rebuilt only at seal/mutation boundaries.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub fn lower_bound_eytzinger(eyt: &[TermId], target: u64) -> usize; // sorted-index of first >= target
  pub fn search_by<K>(&self, key: K, cmp: impl Fn(usize, &K) -> Ordering) -> Option<usize>;
  #[inline] pub unsafe fn prefetch_read(ptr: *const u8); // cfg(target_arch)-gated
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** descent loops compile to cmov (`k = 2k + (a[k] < target)` form preserved); no per-level branches.
- **Allocation Invariant:** mirrors are built once at seal (allocation at seal only); probe paths allocate 0.
- **Benchmark Target:** probe throughput ≥ binary_search at 10³–10⁷ sizes with hit/miss/skew patterns (OPT-20 bench suite); pack BGP probe path measurable via existing pack benches.
- **Layer Boundary:** all kernels stay in `purrdf-core` (`layers.toml` clean via `check-layers.py`); tests extended for n=0, n=1, non-power-of-2, duplicate keys (current `primitives_hotpath_tests.rs:12` covers happy path only).

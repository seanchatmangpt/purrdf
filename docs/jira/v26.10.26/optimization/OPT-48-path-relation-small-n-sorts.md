# [OPT-48] Path Relation Adjacency Small-N Sorts to Branchless Networks

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/path_relation.rs:816-818, 849,
  1026-1027, 1154-1158`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5, second half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** Property-path step construction sorts its
  dense-index adjacency with std sort:
  - `path_relation.rs:816` `nodes.sort_unstable();` (Vec<TermValue>,
    allocation at each step build)
  - `path_relation.rs:818` `statements.sort_unstable();`
  - `path_relation.rs:849`
    `list.sort_unstable_by_key(|edge| (edge.to, edge.statement));`
  - `path_relation.rs:1026` `list.sort_unstable();` — reverse-adjacency
    neighbour lists (`Vec<u32>`) in `reverse_adjacency()` (lazy, OnceLock)
  - `path_relation.rs:1154` `raw.sort_unstable();` — digest path-step list
- **Hardware-Level Impact:** std `sort_unstable` on `Vec<u32>` neighbour
  lists is a generic branchy quicksort/insertion hybrid: per-call indirect
  calls, unpredictable branches on short lists (HNSW-style adjacency
  fan-out is 2-16 entries per node), no ILP across comparators. The u32
  lists at :849/:1026 are pure `u32` sorts of known-small spans —
  comparator-network form executes depth 6 (sort8) fixed, branch-free,
  register-resident.
- **Baseline Cost:** UNVERIFIED — no bench exists for
  `reverse_adjacency()` build or per-step adjacency sort; OPT-20's
  primitives bench suite is the home for the baseline run.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** For the u32 neighbour lists (:849 edge keys
  can pack `(to, statement)` into a u64 key or sort by composite u32 key;
  :1026 pure `Vec<u32>`), cap-and-network: when `len <= 8` copy into
  `[u32; 8]` (sentinel `u32::MAX` fill), run
  `purrdf_branchless::sort_small::sort8`, copy back; `len <= 16` →
  `purrdf_branchless::sort16::batcher_sort16_u32_in_place`.
  The `TermValue` lists (:816/:818) keep std sort — `Ord` on `TermValue`
  is not a u32 network's job; only the u32 spans convert. :1154 stays
  std (arbitrary N, digest path, cold).
  - Status: kernels **Implemented in `purrdf_branchless::sort_small`
    (`sort2/4/5/6/7/8`, XOR-mask `cswap`) and `purrdf_branchless::sort16`
    (`batcher_sort16_u32_in_place`)** — landed; this ticket is the wiring.
    Wiring LANDED as bookkeeping in
    `crates/sparql-eval/src/path_relation.rs` — `sort_u32_list` (:1206) and
    `sort_edges_small_n` (:1234), call sites :849/:1026 now resolve, pinned by
    four oracle/permutation tests (:2444, :2473, :2492, :2526). Landed,
    UNVERIFIED (no-build machine — cargo not run on this lane).
- **Hard Bounds Applied:** fixed-bound network depth (6 comparators stages
  for 8, 63 comparators for 16); no allocation beyond the existing
  neighbour `Vec`s; sentinel fill bounded by `u32::MAX`.
- **Proposed Type Layout & Signatures:**
  ```rust
  fn sort_u32_list(list: &mut [u32]) {
      match list.len() {
          0..=8 => { /* copy to [u32; 8], sort8, copy back */ }
          9..=16 => { /* copy to [u32; 16], batcher_sort16_u32_in_place */ }
          _ => list.sort_unstable(),
        }
  }
  ```
- **Reuse-First:** extend the landed
  `branchless-compare-exchange` / sort-network ledger rows
  (helpers-ledger.toml:3932); no new sort body anywhere — one home per
  job, `purrdf_branchless::sort_small`/`sort16` are the only sanctioned
  sort-network homes.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** for every converted list, output equals the
  std-sort oracle on randomized + adversarial inputs (the sort16 oracle
  harness pattern, sort16.rs:130-171).
- **Allocation Invariant:** zero new allocations; the `[u32; 8]` /
  `[u32; 16]` staging arrays are stack-resident.
- **Benchmark Target:** extend the OPT-20 bench suite with a
  `reverse_adjacency` build bench and a per-step adjacency sort bench;
  report-only per AGENTS.md, never a gate.
- **Layer Boundary:** `sparql-eval` may depend on `branchless` only if the
  edge passes `python3 scripts/check-layers.py --home-for` / `make check`
  layers.toml clean.
- **Valid-neighbor (refusal mirror):** no new refusals added; the
  `_ => list.sort_unstable()` fallback keeps any N > 16 list correct —
  over-refusal risk is zero, but the N>16 neighbour test remains required.

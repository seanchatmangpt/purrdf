# [OPT-13] 8-Hop Traversal Ceiling + Visited64 Bitmask

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/path.rs:1756-1890` (ClosureFrame), `path.rs:1904-1919` (RangeFrame tail), `eval.rs:1643-1681` (reachable_from); `crates/entail/src/owl_dl/roles.rs:357-470` (close_edges), `owl_dl/saturate.rs:405-427` (role_closure); `crates/shapes/src/class_membership.rs:233-276` (reaches_by_subclass); `crates/n3/src/reasoner.rs:1312-1345,330-410`
- **Subsystem:** Graph Topology
- **Inspection Agent:** Agent 13 (8-Hop Graph Depth Limiter)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `while let Some(n) = self.frontier.pop() { ... self.result.insert(n); ... return Step::Call(FrameRequest::Reach(self.inner, n, self.forward)); }` (path.rs:1870) — the `p+`/`p*` transitive closure has **no hop counter anywhere**; termination rests solely on `visited: Visited<I>` (a hash table, path.rs:425, inserts at 1867/1878, membership at 1811/1828/1847). Cyclic graphs terminate but with unbounded hop count. `RangeFrame`'s `{n,}` tail (path.rs:1904-1919) is a full `*` closure — same unbounded-hop exposure (doc at 1913-1915 bounds only the composed *prefix*, not the tail). `reachable_from` (eval.rs:1650, 1661-1668) does a linear membership scan per popped node: `for previous in &visited { if self.growth.terms_equal(previous, node)? { ... } }` — O(n²) over the visited list. In entail, `close_edges` (roles.rs:396+) iterates to fixpoint bounded only by `poll()?`; `role_closure` uses `reached: BTreeSet<u32>` (saturate.rs:416). Shapes uses `visited_inline: SmallVec<[TermId; 32]>` spilling to `Option<FastSet<TermId>>` (class_membership.rs:243-268).
- **Hardware-Level Impact:** unbounded traversal depth on hostile data (linear-chain graphs of arbitrary length); O(n²) string-compare membership scans in CONSTRUCT reachability; hash/BTreeSet overhead per visited node where a single 64-bit word suffices.
- **Baseline Cost:** O(V+E) traversal with per-node hash insert; O(n²) for reachable_from; no worst-case hop bound.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Reuse the in-repo `PathLimits` construction (`crates/sparql-eval/src/path_relation.rs:492-565`: typed constructor rejecting `min_hops > max_hops`, `max_hops()` accessor, depth-bounded DFS via `let mut max_depth = limits.max_hops();` at 1354).
  1. **Hop ceiling:** `pub const HOP_CEILING: u8 = 8;` per crate, configurable via a `PathLimits`-style `max_hops: u8` field. Thread a `hops: u8` through `ClosureFrame`; before each `Step::Call(FrameRequest::Reach(...))` (path.rs:1875, 1886) test `hops == ceiling` and fail typed. Same ceiling for the `RangeFrame` `{n,}` tail.
  2. **Typed failures** mirroring existing precedents: `EvalError::HopCeilingExceeded { path: PathSummary, hops_taken: u8, limit: u8 }` raised via the existing `ctx.storage.fail(...)` channel (path.rs:1851) so it flows through the governed error path; `EntailError::HopCeiling { chain, hops, limit }` following `RoleHierarchyError` (`entail/src/lib.rs:351, 405-407`); `ReasonerError::HopCeiling` in n3 (consistent with the `error_seen: HashSet<ReasonerError>` dedup at reasoner.rs:83/149).
  3. **Visited64 bitmask:** `struct Visited64 { bits: u64, dense: LinearIndex }` — `insert(id) -> bool` assigns a dense 0..63 slot on first sight and sets the bit; on the 65th distinct node, degrade **once, typed** to the existing hash-set `Visited` (path.rs:425) / `FastSet` (shapes). Nodes are already interned ids; the local `LinearEnumerator` assigns dense indices on first visit.
  4. **Application sites:** (A1) `ClosureFrame` hop counter + Visited64; (A2) `RangeFrame` tail shares both; (A4) `reachable_from` — replace the linear scan with Visited64 over the (small) constructed-node population, keeping the `AdmittedVec` as the index→term table; (B1) `close_edges` per-start frontier bitmask (individuals are dense `usize` indices already — plain `u64` works directly); (B2) `role_closure`: `BTreeSet<u32>` → direct `u64` bitmask (role vocabularies far under 64 per closure); (E1) `reaches_by_subclass`: two-word `Visited64` (64 classes) replacing the 32-slot SmallVec+spill.
  5. **Where NOT to impose 8** (explicit exclusions): the `PathLimits` witness engine (already bounded and spec'd by tests, path_relation.rs:2707); the datalog chase round loop (budget-bounded by `StepMeter`/`stop`, chase.rs:870,996); the n3 iteration budget (reasoner.rs:1304-1305); datalog `shortest_path` (`paths.rs:18-46`) — diagnostics-only, cap at `Some(node_count)` returning `None`-typed exhaustion instead; `relax_strata` (`paths.rs:57-85`, `max_passes: Option<usize>` with typed `false` on breach) is the in-repo bounded-passes template. The bitmask does not apply to generic-K `shortest_path` without a dense remap.
- **Hard Bounds Applied:** depth ≤ 8 hops (default, configurable); visited set ≤ 64 nodes before typed degrade; frontier population bounded by traversal class.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub const HOP_CEILING: u8 = 8;
  pub struct Visited64 { bits: u64, dense: LinearIndex }   // insert -> bool (newly seen)
  // errors:
  EvalError::HopCeilingExceeded { path: PathSummary, hops_taken: u8, limit: u8 }
  EntailError::HopCeiling { chain: RoleChainRef, hops: u8, limit: u8 }
  ReasonerError::HopCeiling { hops_taken: u8, limit: u8 }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** membership in Visited64 is one shift+and+test; the 65th-node degrade fires at most once per traversal.
- **Allocation Invariant:** 0 heap allocations while the traversal holds ≤ 64 distinct nodes (bitmask + fixed-size dense index); degrade path allocates once, never per-node.
- **Benchmark Target:** `reachable_from` over N constructed nodes drops from O(n²) compares to O(n) bit ops; path closure over a 64-node cycle trips in bounded time regardless of cycle multiplicity.
- **Layer Boundary:** ceiling constant and error variants live in their owning crates (`sparql-eval`, `entail`, `n3`), no new cross-crate deps; `check-layers.py` clean. Over-refusal guard per CLAUDE.md: every new ceiling ships with paired tests — the bounded case (e.g. a 7-hop path, both-endpoints-bound `p+`) must still succeed; only the 9th hop refuses.
- **Implemented in `purrdf::branchless::bitset`** (crate `crates/branchless`, ported from
  BranchlessCInRust): the word-level set/clear/rank/select primitives are the Visited64
  groundwork (mask ops + dense-index select).
- **Amendment (scan handoff §3.4) — Visited64 ops spec'd as `branchless::bitset` calls.**
  Verified API (`crates/branchless/src/bitset.rs`):
  - `pub const fn is_subset_u64(x: u64, y: u64) -> u64` (bitset.rs:126) — the residual
    form `(x & !y) == 0` as 1/0; the typed "did the frontier stay inside the ceiling
    set" test.
  - `pub fn intersect_u64_slices(a: &mut [u64], b: &[u64])` (bitset.rs:225),
    `union_u64_slices` (bitset.rs:240), `difference_u64_slices` (bitset.rs:255) — the
    multi-word forms for the two-word `reaches_by_subclass` (E1) and per-start frontier
    bitmasks (B1). Note the exact names carry the `_slices` suffix — there is no
    single-word `intersect_u64`; a single-word intersection is `x & y` inline (use the
    slice fns only at 2+ words).
  - `any_bit_set_u64_slice(a: &[u64]) -> bool` (bitset.rs:270-271) — frontier-exhausted
    test.
  - `hamming_u64_slices(a, b) -> usize` (bitset.rs:207) / `jaccard_u64_slices(a, b) ->
    f32` (bitset.rs:187) — optional diff metrics for the typed-hop-exceeded diagnostics
    payload.
  Visited64's per-word membership (`bits >> slot & 1`) and dense-slot assignment stay
  local to `Visited64`; `bitset` owns word/slice algebra only. Layer edge
  `sparql-eval`/`entail`/`shapes` → `purrdf-branchless` is additive
  (`purrdf-branchless` deps only `purrdf-hash`, `layers.toml:386-391`); run
  `python3 scripts/check-layers.py` after the edges land.

# [OPT-15] Shard or Confine the Per-Row Hot Locks; Pad the Parallel Block Slots

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/eval.rs:183-241,462`; `crates/hnsw/src/lib.rs:441`, `search.rs:128`; `crates/validate/src/geo/session.rs:28,31-32`; `crates/sparql-eval/src/parallel.rs:1417,3514`; `crates/sql/src/compiler.rs:1933-1941`
- **Subsystem:** Memory Arena / Concurrency
- **Inspection Agent:** Agent 15 (Zero-IPC Multi-Agent Blackboard)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `TermLookup { shared: Mutex<Option<SharedTermLookup<I>>>, … }` + `SharedWorkspace<Mutex<TermLookupEntries<I>>>` (eval.rs:183-241) — the highest-frequency mutex in the engine: per term lookup, i.e. per-row-per-binding, contended by all workers forked via `fork_admitted`; HNSW `scratch: Mutex<Vec<Visited>>` (lib.rs:441) taken **per distance computation** during search (search.rs:128 documents the two-locks-per-distance problem); geo `prepared: Shared<Mutex<PreparedGeodesicCache>>` per geo call (session.rs:28); `BlockSlot<R,H> = Mutex<Option<…>>` one per block (parallel.rs:1417,3514) — already sharded but not cache-line padded; `AST_COMPILE_MEMO: OnceLock<Mutex<HashMap<[u8;32], Arc<CompiledQuery>>>>` per-query (compiler.rs:1933-1941).
- **Hardware-Level Impact:** mutex lock/unlock + potential futex on the per-row path; false sharing between adjacent `BlockSlot`s; single `Mutex<HashMap>` serializing BLAKE3-digest memo lookups under threaded admission.
- **Baseline Cost:** uncontended lock ≈ 20 ns; contended ≈ 100 ns+ per acquisition, multiplied per row.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** treat only the four sites that warrant it (the audit's bottom line):
  1. **F4 term-lookup memo (per-row)**: shard the map by hash bits into N cache-line-padded `Mutex` buckets (pad each bucket's lock + head), or per-worker thread-local maps drained/merged into the shared map at checkpoint boundaries with monotonic sequence stamps for deterministic merge order.
  2. **F8 HNSW scratch (per-distance)**: per-thread scratch — pass `&mut Vec<Visited>` down or a `thread_local!` pool; single-writer confinement is natural (one search = one thread).
  3. **F7 geo prepared cache**: shard into padded buckets or thread-confine per worker with merge-on-complete (only when geo: filters run hot).
  4. **F10 BlockSlot padding**: `#[repr(align(64))]` on `BlockSlot` to kill false sharing between adjacent blocks.
  - `AST_COMPILE_MEMO` is per-query: shard (16 shards keyed on digest[0..4]) or ArcSwap read-mostly map; will not show in row-throughput profiles — lower priority.
  - **Document confinement, don't rewrite** (explicit no-action list): write-once `OnceLock`/`LazyLock` statics (Tier 4 — the ideal zero-IPC construct); single-worker charge/evidence mutexes justified by design comments (`row_checkpoint.rs:174,177-178`, workspace accounting); already-sharded slot patterns (`plan/mod.rs:243` slot-per-entry free list — the in-repo model; pipeline_bundle digest memo reseat-on-accumulate; governor OnceLock trip path). No channels/crossbeam/pipe IPC exist in any engine hot path (test/tooling only) — nothing to replace with ring buffers there.
- **Hard Bounds Applied:** shard count power-of-2, index = top hash bits; merge order = monotonic sequence stamps (determinism preserved).
- **Proposed Type Layout & Signatures:**
  ```rust
  #[repr(C, align(64))] struct Shard { lock: Mutex<Bucket>, pad: [u8; 0] }   // one cache line
  struct ShardedLookup<const N: usize> { shards: [Shard; N] }                 // N = 16 default
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (concurrency ticket); shard index = shift+mask.
- **Allocation Invariant:** sharded buckets pre-allocated; merge-on-checkpoint allocates once per worker.
- **Benchmark Target:** parallel BGP/term-heavy eval scales with worker count (no single-mutex serialization wall in profiles); `BlockSlot` false-sharing eliminated (verify `align_of::<BlockSlot>() == 64`).
- **Layer Boundary:** determinism courts must stay green (merge order stamped); `check-layers.py` clean; documented-confinement sites get `# IPC-CLASS: CONFINED` style annotations mirroring the house SLEEP-CLASS pattern.

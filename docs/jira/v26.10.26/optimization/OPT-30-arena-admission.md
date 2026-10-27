# [OPT-30] Flat 32 MiB Arena as a New Admission Impl (No Parallel Allocator)

## 1. TARGET CONTEXT
- **Crate & File:** `crates/lex/src/allocation.rs:272-275,31-42,72-90,148-195,367-407,526-554`; `crates/lex/src/allocation/shared.rs:80-84,219-255,730-741`
- **Subsystem:** Memory Arena
- **Inspection Agent:** Agent 08 (Memory Arena Confinement)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** only the dataset term text is arena-confined (`Interner { arena: Vec<u8>, … }`, `ir/builder.rs:320-325`, strings stored once as `StrRange`). Every other hot-path working state is a forest of individually global-allocated buffers that are merely *accounted* through the `Admission`/`WorkspaceCapability` seam — `AdmittedVec` is a `Vec<T>` wrapper (`workspace.rs:696-700`), each vector its own heap block; `Solution = SmallVec<[Option<SolutionTerm>; 4]>` spills per row past 4 columns (`solution.rs:460`); scratch/computed terms are not arena-backed at all (cross-ref OPT-10).
- **Hardware-Level Impact:** N disjoint heap blocks per query (TLB churn, allocator lock, fragmentation) where one contiguous block suffices; accounted-but-unconfined growth defeats locality.
- **Baseline Cost:** one global allocation per Vec/String/Header instead of one bump per object.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** a new `Admission` impl, **not** a new allocator: `struct ArenaAdmission { block: NonNull<u8>, cap: u32 /* ≤ 32 MiB */, live: u32 }` implementing `purrdf_lex::allocation::Admission` (`resize(live) -> Result<(), StorageError>`, allocation.rs:272-275). `Memory<'_, ArenaAdmission>` then works unchanged for all admit/refund logic (reserve/push, allocation.rs:367+, 526-554). Exhaustion is the existing typed path: extend the closed `CapacitySpace` enum (allocation.rs:148-195, 23 variants) with one variant (e.g. `QueryScratch`) and return `CapacityError::new(space, attempted, 32 MiB)` — allocation-free refusal with exact u128 counts (allocation.rs:72-90); **zero fallback allocations**: refuse instead of ever calling the global allocator. Backing: one `Shared<AlignedBlock>` — the align(256) over-alignment precedent (`shared.rs:730-741`) extends trivially to any power-of-2 up to 32 MiB — so multiple AdmittedVecs retain the arena through the existing owner-retention protocol (`SharedOwned::retain_current`, shared.rs:406-421). `AdmittedVec::with_capacity` (workspace.rs:770-789) gains an arena-backed variant; the `allocation: Option<WorkspaceAllocation>` field points at the arena charge instead of a per-vector block. Reset protocol: wholesale `live = 0` per query (matches `reset_charged_growth`, scratch.rs:1043-1044 and `Memory::resume`, allocation.rs:395-400), with `CapacitySpace::check` before any mutation.
- **Hard Bounds Applied:** arena cap 32 MiB; bump-pointer only; no fallback path; typed exhaustion.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct ArenaAdmission { block: NonNull<u8>, cap: u32, live: u32 }
  impl Admission for ArenaAdmission { fn resize(&mut self, live_bytes: usize) -> Result<(), StorageError>; }
  // CapacitySpace gains: QueryScratch
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** bump allocation is pointer-add + capacity check; no allocator lock on the hot path.
- **Allocation Invariant:** arena-backed structures issue exactly one global allocation per query; 0 fallback allocations (alloc-probe-verified).
- **Benchmark Target:** allocation-count bench (OPT-20 suite) shows 1 block per query for arena-backed paths; `miri` clean via the existing lane pattern (zero-alloc streaming precedent).
- **Layer Boundary:** implementation lives in `purrdf-lex` allocation home; `check-layers.py` clean; `helpers-ledger.toml` records the arena as the one home for query-scratch confinement.

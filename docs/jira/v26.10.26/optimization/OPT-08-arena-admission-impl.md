# [OPT-08] Flat 32 MiB Arena via New Admission Impl + Arena-Backed ScratchInterner

## 1. TARGET CONTEXT
- **Crate & File:** `crates/lex/src/allocation.rs:272` (`Admission` seam); `crates/rdf-core/src/ir/builder.rs:320-325` (the one live arena); `crates/sparql-eval/src/scratch.rs:221-252` (primary escape site); `crates/sparql-eval/src/expr.rs:6955-7027`; `crates/sparql-eval/src/solution.rs:460`; `crates/sparql-eval/src/agg_fn.rs:1540`; `crates/lex/src/allocation/shared.rs:730-741`
- **Subsystem:** Memory Arena
- **Inspection Agent:** Agent 08 (Memory Arena Confinement)

## 2. MECHANICAL PATHOLOGY
- **Verdict:** only the RDF interner term text is arena-confined (`builder.rs:320-325`: flat `Vec<u8>` arena + `StrRange` + `HashTable<u32>`, growth admitted via `memory.reserve` with `CapacitySpace::LocalTermBytes` check). Every other hot-path working state is *accounted but disjoint* per-object heap — charged through `Admission`/`WorkspaceCapability`, never backed by one contiguous block. `AdmittedVec` is a `Vec<T>` wrapper (workspace.rs:696-700): each vector is its own heap block.
- **Escape sites:**
  - **C1 (largest):** `ScratchInterner` stores full `TermValue`s — `values: Vec<TermValue>`, `ghosts: Vec<Option<TermValue>>` (scratch.rs:221-252). `TermValue` variants hold `String` payloads (`ir/term.rs:409-439`): every minted term costs 1–3 global `String` allocations + a `HashTable` insert. Direct structural divergence from the `Interner` arena design.
  - **C2:** CONCAT builds the concatenated `String` plus 2 more `String`s (datatype, lang) per evaluated row (expr.rs:6955-7027) — 2–3 extra allocations per CONCAT row.
  - **C3:** `Solution = SmallVec<[Option<SolutionTerm>; 4]>` (solution.rs:460) — rows wider than 4 columns spill into N disjoint per-row blocks inside `Vec<RetainedRow>`.
  - **C4:** `Box::new(SumAccumulator { total: 0 })` / `Box<dyn AggregateAccumulator>` per aggregate group (agg_fn.rs:1540, returns at 1785, 2101).
  - **C5:** `format!`/`to_string()` bypass the codebase's own `Memory::format` (allocation.rs:882-894) and `format_exact` (workspace.rs:2440-2479) — e.g. `n.to_string()` typed-literal rendering per emitted value (agg_fn.rs:1545).
  - **C6:** scratch take/drain return fresh `Vec`s (`take_values_from -> Vec<(u64, TermValue)>`, scratch.rs:1231); parallel forking clones the whole scratch table (scratch.rs:268).
  - **C7:** every `Shared`/`SharedOwned`/`SharedText` mint is its own global-allocator control block (shared.rs:234, layout 200-202); `RowBag::Shared` is per-object, not slab-backed.
- **Hardware-Level Impact:** allocator lock contention and TLB/cache fragmentation across thousands of disjoint blocks per query; each `String` mint is a malloc + memcpy that the arena design (store-once, range-backed) provably avoids in the interner.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** a new `Admission` impl, **not** a new allocator:
  - `struct ArenaAdmission { block: NonNull<u8>, cap: u32 /* ≤ 32 MiB */, live: u32 }` implementing `purrdf_lex::allocation::Admission` (allocation.rs:272). `Memory<'_, ArenaAdmission>` then works unchanged for all admit/refund logic (allocation.rs:367+).
  - **Exhaustion:** extend the closed `CapacitySpace` enum (allocation.rs:148-195, 23 variants) with one variant (`QueryScratch`); `resize` returns `StorageError::Capacity(CapacityError::new(CapacitySpace::QueryScratch, attempted, 32 MiB))` — allocation-free refusal with exact u128 attempted/limit counts (allocation.rs:72-90). **No new error enum.**
  - **Backing:** one `Shared<AlignedBlock>` using the align(256) overalignment precedent (shared.rs:730-741, `#[repr(align(256))] struct Aligned`); multiple structures retain the arena through the existing `SharedOwned::retain_current` protocol (shared.rs:406-421).
  - **Zero fallback allocations:** bump-pointer inside the block; `resize` is the only growth path — refuse (`AdmissionFailed`/`Capacity`) instead of ever calling the global allocator. `AdmittedVec::with_capacity` (workspace.rs:770-789) gains an arena-backed variant; the `allocation: Option<WorkspaceAllocation>` field points at the arena charge instead of a per-vector block.
  - **Reset protocol:** wholesale `live = 0` per query, matching `reset_charged_growth` (scratch.rs:1043-1044) and `Memory::resume` (allocation.rs:395-400); `CapacitySpace::check` guards the 32 MiB cap before any mutation.
- **Placement order (by escape magnitude):**
  1. **C1:** ScratchInterner → Interner-style `arena: Vec<u8>` (arena-backed) + `StrRange` values + dedup-by-value before push, mirroring `builder.rs:320-325`.
  2. **C2:** intern CONCAT datatype/lang IRIs — closed 3-element set (`XSD_STRING` / `RDF_LANG_STRING` / `RDF_DIR_LANG_STRING`, expr.rs:6993-6997) resolved once per process.
  3. **C3:** wide rows stored as `(len: u16, cells_offset: u32)` into one flat cell array.
  4. **C4:** arena-emplaced accumulators with drop tracking (or inline small-box).
  5. **C5/C6:** route diagnostics/formatting through the existing `Memory::format` / `format_exact`; drain helpers take `&mut` destination instead of returning fresh `Vec`s.
- **Hard Bounds Applied:** arena cap 32 MiB (`PRAGMA max_memory`-equivalent ceiling), enforced in `resize` before allocation; failure is a typed, allocation-free refusal — fail closed.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation invariant ticket); bounds check is one predictable compare in `resize`.
- **Allocation Invariant:** a full evaluation pass mints **0** global-allocator calls on the accept path (verify with `purrdf-alloc-probe`); exhaustion is the only exit and is typed.
- **Benchmark Target:** per-row scratch/CONCAT allocation count → 0; query wall-clock improvement recorded in the OPT-20 bench suite (report-only, never a gate).
- **Layer Boundary:** `ArenaAdmission` lives in `purrdf-lex` (allocation home); sparql-eval/rdf-core consume it via the existing `Admission`/`Memory` seam — no parallel allocator, no new helper home (`check-shared-helpers.py` clean; `check-layers.py` 270 edges stay violation-free).

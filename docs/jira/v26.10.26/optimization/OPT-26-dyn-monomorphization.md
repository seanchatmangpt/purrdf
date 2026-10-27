# [OPT-26] Monomorphize Closed-Set dyn Trait Sites (Parser Memory, GuardEvaluator, MetricWorkObserver)

## 1. TARGET CONTEXT
- **Crate & File:** `crates/lex/src/allocation.rs:272,367` + ~60 instantiation sites; `crates/datalog/src/seminaive.rs:2804,4242` + guards plumbing; `crates/geo-kernel/src/context.rs:70,333,521`; `crates/sparql-eval/src/agg_fn.rs:254,339,378,523`
- **Subsystem:** Ingestion / Relational Sieve
- **Inspection Agent:** Agent 10 (Virtual Dispatch & Tagged Union Monomorphization)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `Memory<'_, dyn ParserAdmission + '_>` at ~28 sites in `sparql-algebra/src/parser.rs` (443, 465, 490, 546, 569, 630, 653, 746, 815, 3509…) and `parser/machine.rs:140,202`; `storage: &mut dyn Storage` at 8 sites in `cdt/src/term.rs:357-565` + 28 in `functions.rs` (alias `cdt/src/memory.rs:13`); `guards: &'a dyn GuardEvaluator<D, N>` threaded through every join/projection step of the datalog chase (`seminaive.rs:2804,4242`, `session.rs:153`, `schedule.rs:391,445,555`, `seminaive/guards.rs:41,90,207`, `ordered.rs:181`); `observer: &'a mut dyn MetricWorkObserver` fields (`context.rs:333,521`; 235 dyn sites total); `Box<dyn AggregateAccumulator>` with a `Box<dyn Any + Send>` double-dispatch inside `combine` (`agg_fn.rs:339,378`).
- **Hardware-Level Impact:** vtable hop per parsed token / per derived tuple / per geo work unit / per aggregate row; indirect-call latency + blocked inlining on the hottest loops in the workspace.
- **Baseline Cost:** 1-2 indirect calls + lost inlining per unit; GuardEvaluator dispatch is per-tuple in the chase (hottest loop).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** ranked value × feasibility.
  1. **F7 (zero-risk, rank 1):** `Memory<'a, S: Admission + ?Sized>` is already generic — instantiate concrete `type ResMemory<'a> = Memory<'a, Resident>` at the ~60 parser/CDT/xsd sites. Implementor set closed (`Resident` lex, `ChildStorage` cdt, `Resident` xsd, `RangeProducts`). No enum needed; plain generics suffice.
  2. **F1 (hottest loop):** `&dyn GuardEvaluator<D, N>` → `GuardMode` `#[repr(u8)]` enum + untagged payload union holding `NoGuards` (guard.rs:488, the only production impl) plus one reserved first-party slot; enum keeps ABI stable vs plain generics.
  3. **F6:** `MetricWorkObserver` → `enum ObserverKind { Unmetered, Continuation }` + untagged payload, or genericize `MetricWorkContinuation` on `O: MetricWorkObserver`; the blanket `impl for &mut T` (context.rs:265) must be modeled explicitly or dropped.
  4. **F3:** monomorphize first-party scalar aggregates into `BuiltinAgg` `#[repr(u8)]` enum; keep `Box<dyn>` only for public custom aggregates (cross-ref OPT-04).
  5. Then: `SnapshotStatistics` 3-variant enum (`statistics.rs:71`; blanket impl layered.rs:114 enumerable); `RuleEvidence` → concrete `Capture` (evidence.rs:31,320; single impl, N=1); `ByteDrain` 4-variant enum (`sink.rs:242,270,300,349,363`; `Tee` payload stays generic); `RdfCodec` 6-variant (documented cold — low priority).
- **Flagged OPEN (design decision, never closed enumeration):** `PropertyFunction` (~80 impls), `PfCursor` (~100 impls), `StopSignal` (cross-ref OPT-04), `PageProvider`/`SegmentedProvider`, `EvidenceSource`/`ServiceResolver`, `GraphResolver`, `StreamingSink`, `WorkspaceReservation` — all public extension surfaces; treatment is enum-fast-path-for-first-party + dyn-fallback.
- **Hard Bounds Applied:** enum discriminants only where the production implementor set is closed (N=1..6 verified per trait).
- **Proposed Type Layout & Signatures:**
  ```rust
  pub type ResMemory<'a> = purrdf_lex::allocation::Memory<'a, Resident>;
  #[repr(u8)] pub enum GuardMode { NoGuards /*, Reserved1 */ }
  #[repr(u8)] pub enum BuiltinAgg { Count, Sum, Min, Max, Sample, GroupConcat }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** chase guard evaluation is a discriminant switch (single predictable branch), no vtable load; parser `Memory` calls monomorphize (verify `cargo asm`/`objdump` shows direct calls).
- **Allocation Invariant:** no new allocations; existing `Box<dyn>` in open-API paths unchanged.
- **Benchmark Target:** parser hot path and datalog chase measurable via existing benches; expect indirect-call elimination on parse and per-tuple paths.
- **Layer Boundary:** public traits unchanged (open set preserved); `check-layers.py` clean; conformance courts stay green.

# [OPT-10] Monomorphize Closed-Set dyn Traits (Memory<Resident>, GuardEvaluator, MetricWorkObserver, Aggregates)

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-algebra/src/parser.rs:443,465,490,546,569,630,653,746,815,3509,3523,3573,3584,3646,3699,4929,4979,5000` + `parser/machine.rs:140,202`; `crates/cdt/src/term.rs:357-565`, `functions.rs` (28 sites), `memory.rs:13,119,130`; `crates/xsd/src/range/storage.rs:52`; `crates/entail/src/owl_dl/data.rs:124,142`, `bounds.rs:258,326,379,759`, `admission.rs`; `crates/datalog/src/guard.rs:354,488`, `seminaive.rs:2804,4242`, `seminaive/session.rs:153`, `schedule.rs:391,445,555`, `seminaive/guards.rs:41,90,207`, `seminaive/ordered.rs:181`; `crates/geo-kernel/src/context.rs:70,265,333,521,601`; `crates/sparql-eval/src/agg_fn.rs:254,339,378,523,1486`; `crates/rdf-core/src/statistics.rs:71`, `dataset_view.rs:1164,1910,2305`, `statistics/layered.rs:24,114`, `seminaive/evidence.rs:31,320`, `stratum.rs:71,85`, `sink.rs:242-403`, `ir/segmented/session.rs:1720`; `crates/rdf/src/native_codecs/codec.rs:40,140`
- **Subsystem:** Ingestion (parser admission) | Relational Sieve (datalog chase) | Memory Arena | Egress
- **Inspection Agent:** Agent 10 (Virtual Dispatch & Tagged Union Monomorphization); overlap with Agent 04 (OPT-04)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `Memory<'s, dyn ParserAdmission + 's>` at ~28 parser sites (parser.rs:443 etc.) and `pub(crate) type Memory<'a> = purrdf_lex::...Memory<'a, dyn Storage + 'a>` (cdt/memory.rs:13) — every parser instantiates the already-generic `Memory<'a, S: Admission + ?Sized>` (lex/allocation.rs:272,367) through a trait object.
  - `guards: &'a dyn GuardEvaluator<D, N>` (seminaive.rs:2804,4242; session.rs:153; schedule.rs:391,445,555; guards.rs:41,90,207; ordered.rs:181) — ~35 dyn sites; **only production impl is `NoGuards`** (guard.rs:488), everything else is test scaffolding. Closed set (N=1 in prod).
  - `observer: &'a mut dyn MetricWorkObserver` (geo-kernel/context.rs:333; `Option<&'a mut dyn>` at :521) — 235 dyn sites, the largest single trait count; production set ≈ closed (2 first-party impls: `MetricWorkContinuation` context.rs:358, `Unmetered` :601, plus blanket `impl<T: ?Sized> MetricWorkObserver for &mut T` :265).
  - `AggregateAccumulator` boxed everywhere: `fn combine(&mut self, other: Box<dyn AggregateAccumulator>)` (agg_fn.rs:339), `fn into_any(self: Box<Self>) -> Box<dyn std::any::Any + Send>` (:378 — a second dynamic-dispatch layer), `fn init(...) -> Box<dyn AggregateAccumulator>` (:523); 53 dyn sites in agg_fn.rs alone.
  - `Option<&mut dyn RuleEvidence<D, N>>` (stratum.rs:71,85) — single impl `Capture<S, D::Id, N>` (evidence.rs:320).
  - `drain: Option<&'a mut dyn ByteDrain>` (sink.rs:396; session.rs:1720) — closed first-party kinds: `WriterDrain` (:270), `Measure` (:300), `Digest` (:349), `Tee<A,B>` (:363), blanket `&mut T` (:251).
  - `&'static dyn RdfCodec` from `codec_for` (rdf/native_codecs/codec.rs:140) — 6 impls, self-documented COLD dispatch (doc comment).
  - `statistics() -> Option<&dyn SnapshotStatistics<I>>` (dataset_view.rs:1164,1910,2305; layered.rs:24) — closed: `LocalStatistics` (ir/statistics.rs:917), `SegmentedSession` (statistics.rs:304), `TripleRead` (triple/read.rs:101), blanket `impl<D: LayeredStatistics>` (layered.rs:114).
- **Hardware-Level Impact:** per-tuple/per-row vtable hop (2 cycles + probable L1I/L1D miss on far-side control word) in the hottest loops in the workspace: guards evaluate once per derived tuple in the datalog chase; accumulators per row per aggregate; parser admission per token; geo observer per metric step. ~1,445 non-test dyn sites workspace-wide (~630 excluding `dyn Error`/`FnMut` boilerplate).
- **Baseline Cost:** 1 indirect call + branch-target prediction per element; unmonomorphizable generic code paths (no inlining of `NoGuards`' no-op).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal (ranked by value × feasibility):**
  1. **F7 (highest value-per-effort, zero-risk):** `Memory<'a, S>` is already generic — instantiate concretely. Add `pub type ResMemory<'a> = Memory<'a, Resident>;` and sweep the ~60 parser/CDT/xsd/entail dyn sites to it. No enum needed; plain generics suffice since `ParserAdmission`/`Storage` impl sets is closed (`Resident`, `ChildStorage` cdt/memory.rs:130, xsd `Resident` range/storage.rs:52, `RangeProducts` entail/admission.rs). Removes vtable calls from the query-text and CDT parse hot paths.
  2. **F1 `GuardEvaluator`:** replace `&dyn GuardEvaluator<D,N>` with `#[repr(u8)] enum GuardMode { NoGuards, /* reserved first-party slot */ }` + untagged payload union. Guards evaluate once per derived tuple — hottest loop; enum keeps ABI stable vs. pure generics.
  3. **F6 `MetricWorkObserver`:** `#[repr(u8)] enum ObserverKind { Unmetered, Continuation }` + untagged payload; drop or model the blanket `&mut T` impl (context.rs:265) as a nested monomorphized case. Mechanical across 235 sites.
  4. **F3 `AggregateAccumulator`:** monomorphize first-party scalar aggregates (`COUNT/SUM/MIN/MAX/SAMPLE/GROUP_CONCAT`) into `#[repr(u8)] BuiltinAgg` enum with untagged payload; route only custom (public-API) aggregates through `Box<dyn>`. Also collapses the `into_any` downcast protocol for builtins.
  5. **F9 `SnapshotStatistics`:** 3-variant enum (Local/Segmented/TripleRead); blanket `LayeredStatistics` impl enumerable.
  6. **F10 `RuleEvidence`:** trivially closed (N=1) — replace `Option<&mut dyn RuleEvidence>` with `Option<&mut Capture<...>>` concrete type; no enum.
  7. **F11 `ByteDrain`:** `#[repr(u8)] enum DrainKind { Writer, Measure, Digest, Tee }` (Tee generic payload) or genericize Tee usage.
  8. **F16 `RdfCodec`:** closed 6-variant enum or return-by-value from `codec_for`; explicitly documented cold — low priority.
- **Hard Bounds Applied:** conversion only where the implementor set is closed/enumerable (verified above per trait).
- **Proposed Type Layout & Signatures:** all enum payloads `#[repr(C)]` untagged unions where payload sizes differ; `#[repr(u8)]` discriminants; see per-item proposals above.
- **Open-set flags (design decisions — enum-fast-path for first-party + dyn fallback, NEVER closed enumeration; public extension surfaces):** `PropertyFunction` (property_fn.rs:2300, ~80 impls), `PfCursor` (:609, ~100 impls), `StopSignal` (see OPT-04 atomic-bit design), `PageProvider`/`SegmentedProvider` (ir/paged/provider.rs:216; ir/segmented/mod.rs:276), `EvidenceSource`/`ServiceResolver` (evidence_exec.rs:114; remote.rs:747), `GraphResolver` (engine.rs:1422), `StreamingSink` (gts/reader.rs:364), `WorkspaceReservation` (dataset_view.rs:194), `NativeError` (hash/error.rs:50). Cold/no-action: `Box<dyn std::io::Write>` sites, `Arc<dyn Any + Send + Sync>` type erasure, `EvidenceLease` marker.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** hot paths (datalog chase guard eval, aggregate folds, parser admission, geo metric loop) emit zero vtable hops — no indirect calls on the per-tuple/per-row/per-token path.
- **Allocation Invariant:** no new heap allocations introduced; `ResMemory` instantiation removes fat-pointer construction.
- **Benchmark Target:** datalog chase and aggregate GROUP BY throughput improvement visible in the OPT-20 bench suite; parser admission cost reduction in lexer/parser benches.
- **Layer Boundary:** zero cyclic or layer violations via `check-layers.py`; public extension APIs (open-set traits listed above) unchanged in signature and dyn usability.

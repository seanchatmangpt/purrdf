# [OPT-23] Registry IRI→u32 Interning + Open-Set dyn Fast Paths (Design Decisions)

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/user_fn.rs:969-973,425-468`; `crates/sparql-eval/src/remote.rs:747`; `crates/sparql-eval/src/update.rs:382`; `crates/sparql-eval/src/property_fn.rs:609,2300`; `crates/shapes/src/sparql.rs:963-965,1324`; `crates/shapes/src/query_law.rs:135-136`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 04 (§2, §3) + Agent 10 (open-set flags)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `UserFunctionRegistry { fns, native, exprs: DetHashMap<String, …> }` keyed by IRI `String` holding `NativeFnBody = Arc<dyn Fn(&[&TermValue]) -> …>` etc. (user_fn.rs:969-973, 425-440); dispatch = hash-map String lookup + 2 vtable hops per function invocation, per row in FILTER/SELECT bodies (hottest hook in the SHACL validator path). `NativeFnContext.charge: &dyn Fn(NativeCharge) -> …` (user_fn.rs:468,547) despite `NativeCharge` being a 3-variant enum (:459).
- **Hardware-Level Impact:** String hash + compare per call, 2 fat-pointer loads + indirect calls per invocation; allocator traffic on the registry key path.
- **Baseline Cost:** ~2 vtable hops + String hash per function call per row.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) Intern function/relation IRIs to `u32` ids at registration; store bodies in a `Vec<BodySlot>` indexed by id — dispatch becomes array index + `match` on a `kind: u8` discriminant (enum-of-boxes, 1 branch instead of 2 hops). (2) Replace the `charge` closure with a direct `&GovernorState` method call (`Work/WorkspaceGrowth/Checkpoint` is a textbook discriminant case). (3) Built-in property functions: `enum BuiltinPropertyFn { Approved, SelfRelation, … }` with a `supports: u8` capability bitmask (`supports_candidate_scope` etc.); evaluator fast-paths built-ins via `match`, falls to `Arc<dyn PropertyFunction>` only for user registrations (public API preserved). (4) Open-set traits — **design decisions, not closed enumeration** (Agent 10 flags): `PropertyFunction`/`PfCursor` (~80/~100 impls, designed extension surface): enum fast path for first-party relations (`MemoryRelation`, `PathWitnessRelation`, `GeoRelation`, `HnswRelation`, `TextSearchRelation`), `dyn` fallback for external impls. Same treatment: `ServiceResolver`, `GraphResolver`, `UserFunctionAdmission`, `PageProvider`/`SegmentedProvider`, `EvidenceSource`, `StreamingSink`, `WorkspaceReservation`, `NativeError`.
- **Hard Bounds Applied:** builtin enum variants bounded by the in-tree producer set; `supports` bitmask u8.
- **Proposed Type Layout & Signatures:** `struct BodySlot { kind: u8, id: u32 }`; `#[repr(u8)] enum BuiltinPropertyFn { … }` + `supports: u8`.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** built-in dispatch is a `match` on a u8 discriminant; no `dyn` call on first-party paths.
- **Allocation Invariant:** 0 allocations per function call (registration-time interning only).
- **Benchmark Target:** property-function join and SHACL-AF function benches show ≥1 vtable hop removed per row.
- **Layer Boundary:** public extension APIs unchanged (external registries keep compiling); `check-layers.py` clean.

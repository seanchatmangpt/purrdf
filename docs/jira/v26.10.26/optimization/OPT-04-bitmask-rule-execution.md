# [OPT-04] Bitwise Stop-Signal, Boundness Mask, and BitmaskRule Short-Circuits

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/governor.rs:34,41,60`; `crates/sparql-eval/src/governor/mod.rs:128`; `crates/validate/src/governors.rs:148,187,217`; `crates/sparql-eval/src/property_fn_plan.rs:958,998,1055,1171,3121,3213,6151,6175`; `crates/entail/src/owl_dl/graph.rs:1578,1592,1651`
- **Subsystem:** Relational Sieve / Attestation (guard & hook execution)
- **Inspection Agent:** Agent 04 (Bitwise Rule & Hook Execution)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `pub trait StopSignal { fn stopped(&self) -> bool; }` (governor.rs:34; duplicate at governor/mod.rs:128) stored as `Option<Arc<dyn StopSignal>>` (governors.rs:148,187,217; ~20 entail sites; `seminaive.rs:773` polled per round at :817); `bound: &dyn Fn(&Variable) -> bool` threaded through planning (property_fn_plan.rs, 8 sites); `compat: &dyn Fn(usize, usize) -> bool` in the DL clique search (graph.rs:1578,1592,1651), called O(n²) per search.
- **Hardware-Level Impact:** 2 vtable hops (fat pointer → vtable → method) + probable cache miss on the far-side control word per poll/call; `compat` pays an indirect call O(n²) times.
- **Baseline Cost:** 2-3 cycles + miss per dispatch; O(n²) indirect calls per clique search.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) **StopSignal → atomic latch**: predicate set is exactly `{not-stopped, stopped-cancelled, stopped-deadline}` — replace with a host-published `AtomicU64` where bit 0 = stop-latched, bits 1-2 = cause (`Cancelled`/`Deadline`, existing enum governor.rs:60). Hot loop: `if stop_bits & 1 != 0` — 1 load + 1 ALU op, no vtable, no `Option` niche branch; keep the trait as a thin outer shim for API compat. (2) **Boundness bitmask**: precomputed `u64`/`SmallVec<[u64; 4]>` mask replacing all 8 `&dyn Fn(&Variable) -> bool` sites — `is_bound(v) = (mask[v >> 6] >> (v & 63)) & 1`. (3) **Clique adjacency bitmask**: `SmallVec<[u64; N]>` adjacency per item — `compat(i, j) = adj[i] & (1 << (j & 63)) != 0`; highest-value BitmaskRule in the entailment engine. (4) **SHACL kind-mask**: constraint dispatch is already enum (`Constraint` ~15 variants, shapes.rs:438; `RuleBody` rules.rs:94) — add a per-shape precomputed `u64` kind-mask; one `testnz` per value node skips whole component families (e.g. no `sh:datatype` work). (5) **Accumulator enum-of-boxes**: in-tree `SUM/COUNT/MIN/MAX/AVG/GROUP_CONCAT/SAMPLE` → `enum Accum` + `u16` capability bitmask (`combine`/`distinct`/`into_any`); user-registered aggregates keep the boxed fallback (public API preserved). (6) **N3 CPS → stack machine**: `next: &mut dyn FnMut(&mut Alpha, &mut Bindings) -> bool` continuations (n3/src/reasoner.rs:6838,6887,6905,6955) and `emit` (:3256,3281) → explicit `enum Frame { Term(..), Sequence(..), Graph(..) }` stack; removes one dyn call + heap frame per binding step. (7) Misc: blank-node `mint: &mut dyn FnMut() -> u64` → `&mut u64` counter (shapes/src/rules.rs:1421,1709); `Box<dyn Iterator<QuadIds>>` → `enum QuadSource` (shapes/src/data_view.rs:877,894); `NativeCharge` 3-variant enum (user_fn.rs:459) → `&GovernorState` method.
- **Not enumerable (keep dyn, de-dynamicize dispatch only):** `NativeFnBody`/`ExprFnBody` user closures (`sparql-eval/src/user_fn.rs:425,432,440,661-666`), `PropertyFunction`/`PfCursor` user producers (`property_fn.rs:2300,609`), `ServiceResolver`/`GraphResolver`/`UserFunctionAdmission` host hooks — for these the win is interned-IRI→`u32` `Vec` dispatch + enum-of-boxes, never bitmasks.
- **Hard Bounds Applied:** boundness mask ≤ 256 variables (4×u64); adjacency bitmask ≤ 64 neighbors per word; stop cause = 2 bits.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct StopBits(AtomicU64);              // bit0 latched, bits1-2 cause
  pub struct BoundMask(SmallVec<[u64; 4]>);    // is_bound(v) = bit test
  pub enum Accum { Sum(..), Count(..), .. , Custom(Box<dyn AggregateAccumulator>) }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `is_bound`/`compat` compile to shift+and+testnz (1-2 cycles); stop poll is load+bit-test.
- **Allocation Invariant:** masks are precomputed per plan/shape; poll path allocates 0.
- **Benchmark Target:** stop-poll ≤ 2 ns; clique search removes O(n²) indirect calls; datalog round-loop poll measurable via existing seminaive benches.
- **Layer Boundary:** `check-layers.py` clean; public extension traits unchanged (conformance courts stay green).

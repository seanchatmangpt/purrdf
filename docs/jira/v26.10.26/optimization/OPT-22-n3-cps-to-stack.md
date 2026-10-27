# [OPT-22] N3 Reasoner: CPS Continuations → Explicit Stack Machine; Binding-Cycle Guard De-Hashed

## 1. TARGET CONTEXT
- **Crate & File:** `crates/n3/src/reasoner.rs:6838-6955` (CPS unification continuations), `:3256,3281` (emit callback), `:330-410` (binding-cycle guard); `crates/n3/src/proof_check_n3.rs:382` (derivation-depth walk)
- **Subsystem:** Ingestion/Reasoning (backward-chaining N3 kernel)
- **Inspection Agent:** Agent 04 (Bitwise Rule & Hook Execution, item 6); Agent 13 (8-Hop Graph Depth Limiter, findings D1/D2)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `reasoner.rs:6838-6955` — continuation-passing unification: `next: &mut dyn FnMut(&mut Alpha, &mut Bindings) -> bool` threaded through every matching step (sites at `:6838,6887,6905,6955`).
  - `reasoner.rs:3256,3281` — `emit: &mut dyn FnMut(BackwardStep)` invoked per emitted backward step.
  - `reasoner.rs:330-410` — variable-resolution cycle guard: `seen: &mut HashSet<Name>` (String keys) with `if !seen.insert(name.clone()) { return term.clone(); }` (`:336`), depth unbounded.
  - `proof_check_n3.rs:382` — `while let Some((position, bindings, dependencies)) = pending.pop()` walks derivation depth with no ceiling enforced.
- **Hardware-Level Impact:** one vtable call + one heap frame allocation per binding step during unification (the innermost loop of backward chaining); HashSet cycle guard clones a `Name` (heap string) per visit and hashes on every probe; unbounded recursion depth on cyclic bindings relies on hash-detection alone with no typed ceiling.
- **Baseline Cost:** ~2–5 cycles indirect call + allocator round-trip per binding step; per-visit clone+hash+alloc in the cycle guard.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. CPS → stack machine: replace the `next` continuation chain with an explicit work stack of `enum Frame { Term(..), Sequence(..), Graph(..) }` — the unifier pops/pushes frames instead of calling through `&mut dyn FnMut`. Zero dynamic dispatch; frame transitions are a `match` on the discriminant.
  2. Emit path: replace `emit: &mut dyn FnMut(BackwardStep)` with a monomorphized generic (`impl FnMut`) at the two call sites, or an enum sink over the enumerable emit kinds — one predictable branch instead of a vtable hop per step.
  3. Cycle guard (Agent 13 D2): replace `HashSet<Name>` with a linear set `SmallVec<[(NameId, u8); 8]>` (name id + depth), sized exactly to the 8-hop ceiling — at most 8 entries before a typed failure; linear scan of ≤8 entries beats hashing and clones nothing. Fail as `ReasonerError::HopCeiling` (new variant, consistent with the existing `error_seen: HashSet<ReasonerError>` dedup at `:83/149`).
  4. Depth ceiling (Agent 13 D1): the forward-chaining fixpoint (`:1312-1345`, `'fixpoint: loop`) keeps its existing budgets (`max_closure_facts`, `max_total_bytes` at `:1304-1305`) — budgets stay. Proof derivation depth (`proof_check_n3.rs:382`) gains a `depth: u8` counter failing typed at `HOP_CEILING = 8` via the same `ReasonerError::HopCeiling`.
- **Hard Bounds Applied:** frame stack depth capped by the walk depth cap (8); cycle guard holds ≤ 8 entries inline, zero heap; derivation depth ≤ 8 before typed refusal.
- **Proposed Type Layout & Signatures:**
  ```rust
  #[repr(u8)]
  enum Frame { Term(..), Sequence(..), Graph(..) }        // explicit stack machine
  struct CycleGuard { seen: SmallVec<[(NameId, u8); 8]> } // linear set, inline
  pub const HOP_CEILING: u8 = 8;
  ReasonerError::HopCeiling { hops_taken: u8, limit: u8 } // typed, dedup-compatible
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** 0 vtable calls in the unification hot loop — dispatch is enum-`match` only (emit path: monomorphized generic, one call site specialization each).
- **Allocation Invariant:** no heap frame per binding step (frames live on the explicit stack); cycle guard allocates nothing for chains ≤ 8; guard entries are `(NameId, u8)` pairs, no `Name` clones.
- **Benchmark Target:** backward-chaining unification steps show no allocator traffic (verify via `purrdf-alloc-probe` on a cyclic-graph corpus); depth-8 cyclic binding terminates with typed `HopCeiling`, not stack exhaustion.
- **Layer Boundary:** changes confined to `purrdf-n3`; zero violations via `check-layers.py`.

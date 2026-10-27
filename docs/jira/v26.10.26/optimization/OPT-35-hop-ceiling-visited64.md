# [OPT-35] 8-Hop Ceiling and Visited64 Bitmask for Multi-Hop Traversals

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/path.rs:1756-1890,1904-1919,3260-3287,425-439`; template `crates/sparql-eval/src/path_relation.rs:492-565`; `crates/sparql-eval/src/eval.rs:1643-1681`; `crates/entail/src/owl_dl/roles.rs:357-470`, `saturate.rs:405-427`; `crates/n3/src/reasoner.rs:1312-1345,330-410`; `crates/shapes/src/class_membership.rs:233-276`
- **Subsystem:** Relational Sieve (graph topology)
- **Inspection Agent:** Agent 13 (8-Hop Graph Depth Limiter)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `ClosureFrame` (`p+`/`p*` transitive closure) has a `Visited<I>` hash-table guard (`PathMap<I, ()>`, path.rs:425, inserted at 1867/1878, checked at 1811/1828/1847) but **no hop counter anywhere** — termination rests solely on visited-set O(V+E); cyclic graphs terminate with unbounded hop count. The `p{n,}` open tail (path.rs:1904-1919) is a full `*` closure — same exposure. `reachable_from` does an O(n²) linear scan for membership: `for previous in &visited { if self.growth.terms_equal(previous, node)? { … } }` (eval.rs:1650,1661-1668). N3 variable-resolution cycle guard uses a clone-heavy `HashSet<Name>` (reasoner.rs:336,333). shapes `reaches_by_subclass` uses `SmallVec<[TermId; 32]>` spilling to `Option<FastSet>` (class_membership.rs:243-268).
- **Hardware-Level Impact:** unbounded traversal depth on hostile cyclic data; hash-table insert/lookup per node where a bitmask bit suffices under 64 nodes; O(n²) term-equality scans.
- **Baseline Cost:** hash insert + compare per node; linear scan per pop.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) thread `hops: u8` through `ClosureFrame`; before each `Step::Call(FrameRequest::Reach(…))` (path.rs:1875,1886) test `hops == HOP_CEILING` (8) and fail typed via the governed error channel (`ctx.storage.fail(...)`, path.rs:1851). The in-repo template already exists: `PathLimits { max_hops, … }` with constructor validation and a hard-failure test (path_relation.rs:492-565, 530-532, test :2707) — reuse its construction with `const DEFAULT_HOP_CEILING: u8 = 8`. (2) `Visited64 { bits: u64, dense: LinearIndex }`: `insert(id) -> bool` assigns a dense 0..63 slot on first sight and sets the bit; on the 65th distinct node, degrade **once** to the existing hash-set `Visited`/`FastSet` — a typed upgrade, not an error. Applies at: `ClosureFrame` (A1), `p{n,}` tail (A2), `reachable_from` (A4 — strictly better even below 64; keep `AdmittedVec` as the index→term table), role-hierarchy per-start frontiers (roles.rs:396+, individuals are dense `usize` — plain `u64` works), `role_closure` BTreeSet → direct `u64` (saturate.rs:416, dense u32 ids), `reaches_by_subclass` two-word variant (class_membership.rs:246). (3) Typed failures per crate following existing precedents (`RoleHierarchyError` lib.rs:351,405-407; `relax_strata`'s bounded-`false` paths.rs:57-85): `EvalError::HopCeilingExceeded { hops_taken, limit }`, `EntailError::HopCeiling { … }`, `ReasonerError::HopCeiling { … }`; n3 derivation depth carried per `DerivedFact` accumulation, failing at 8. N3 cycle guard → `SmallVec<[(NameId, u8); 8]>` linear set (fits the ceiling exactly, removes all hashing).
- **Where NOT to impose 8** (explicit exclusions): the `PathLimits` witness engine (already bounded + spec'd by tests), the chase round loop (budget-bounded), n3 iteration budget (budget-bounded), datalog `shortest_path` diagnostics (cap at node_count, not 8).
- **Hard Bounds Applied:** 8 hops per traversal (configurable via `max_hops: u8` copied from `PathLimits`); bitmask population ≤ 64 with typed fallback.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub const HOP_CEILING: u8 = 8;
  struct Visited64 { bits: u64, dense: LinearIndex }   // insert(id) -> bool; degrade-on-65th
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** visited test/insert is shift+bit-or when population ≤ 64; hop check is one compare-branch per frontier expansion.
- **Allocation Invariant:** `Visited64` + frontier live on the stack/arena; hash-set only on the one-time 65th-node degradation.
- **Benchmark Target:** visited membership ≤ 2 ns; hostile-cycle court (cycle of length > 8) trips typed within bounded expansions; paired valid case (`p+` over an 8-deep chain) still succeeds — over-refusal rule.
- **Layer Boundary:** `check-layers.py` clean; `p*/p+` with both endpoints bound remains legal and cheap (boundness gate per OPT-19, not the hop ceiling).

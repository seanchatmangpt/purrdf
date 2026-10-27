# [OPT-50] MutableDataset Graph Declaration and Blank-Identity Scans to Indices

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/ir/mutable.rs:605-611`,
  `:983-985`, `:1245-1251`
- **Subsystem:** Memory Arena
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5, second half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `mutable.rs:605`
    `for index in 0..self.base.term_count()` with
    `if let TermRef::Blank {..} = self.base.resolve(id)`
    — `visit_blank_identities` walks the ENTIRE base term space to find
    blanks, O(total terms) per call regardless of blank count.
  - `mutable.rs:983`
    `self.declared_graphs.iter().position(|g| *g == value)` — linear
    scan + full `TermValue` equality per declared graph on every emptied
    declaration withdraw.
  - `mutable.rs:1245`
    `self.declared_graphs.iter().position(|g| g == graph)` — same linear
    `TermValue`-equality scan in `withdraw_graph_declaration`; :1251 does
    it again via `retain`.
- **Hardware-Level Impact:** :605 is a full-arena walk touching every
  term's arena bytes and resolve dispatch (tag check per term) — cache
  lines fetched for the ~100% non-blank majority; a blank-index side
  table turns it into O(#blanks) dense jumps. :983/:1245/:1251 compare
  full `TermValue` structs (heap string compare inside) per declaration
  per withdrawal, O(#graphs) with string-compare constants; a
  `TermId`-keyed map turns each into one hash probe.
- **Baseline Cost:** UNVERIFIED — no bench; measurable via the existing
  `mutable.rs:3911` full-scan-oracle property test rig extended with a
  term-count counter.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  - Blank side-index: a dense `Vec<TermId>` of blank-term ids maintained
    at mint/freeze time (the delta already tracks per-term lifetime at
    :244; the base store already has segmented storage to hang a
    blank-class index on). `visit_blank_identities` iterates the index
    instead of the whole term space; the delta/graph walks (:612-617)
    are unchanged.
  - `declared_graphs` keyed by id: keep `Vec` for order (freeze scan
    order is a pinned invariant, see the regression pin at :3520-3589)
    but add a `FastMap<TermValue-or-TermId, usize>` position index
    maintained on insert/remove so `position` becomes `get` + `swap_remove`
    is NOT allowed (order-pinned) — use `get` for the position and
    `Vec::remove` as today; the win is replacing per-entry
    `TermValue` equality with a hashed lookup.
  - Status: `purrdf_core::FastMap` is the sanctioned map home
    (CLAUDE.md; std `RandomState` clippy-banned) — Implemented in
    `purrdf_core::FastMap`, landed; this is wiring.
- **Hard Bounds Applied:** order of `declared_graphs` stays
  insertion-order (regression pin :3520 must keep passing unmodified);
  blank index bounded by blank count, rebuilt lazily at freeze.
- **Proposed Type Layout & Signatures:**
  ```rust
  blank_index: Vec<TermId>,              // dense, id-ascending
  declared_pos: FastMap<TermValue, usize>, // mirror of declared_graphs
  ```
- **Reuse-First:** extend the existing delta value-hash index
  (mutable.rs:781-784 documents the store-once hash-index pattern this
  ticket mirrors); no second interner, no second map type.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `visit_blank_identities` visits exactly the
  set and order the full-scan oracle visits (existing property test
  mutable.rs:3911 rig extended); withdrawal tests must pass unchanged.
- **Allocation Invariant:** the index itself may allocate once at
  freeze/declare, but the hot `withdraw_*` path performs zero
  allocations and zero `TermValue` deep-equality calls (assert via
  alloc-probe counters, `purrdf-alloc-probe` is the sanctioned home).
- **Benchmark Target:** freeze + declare/withdraw cycle bench in
  `purrdf_testkit::bench`, report-only.
- **Layer Boundary:** rdf-core-internal change; `make check` green; no
  new crate edges (check-layers.py unchanged).

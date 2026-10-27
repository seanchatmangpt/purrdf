# [OPT-24] Cold-Path & Explicit No-Action Register (Audit Ceiling)

## 1. TARGET CONTEXT
- **Crate & File:** workspace-wide; see per-item references
- **Subsystem:** All
- **Inspection Agent:** Agents 01, 06, 09, 10, 13, 15, 16, 17, 20 (consolidated no-action lists)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** N/A — this ticket is the loud register of sites inspected and deliberately *not* scheduled, so no lane re-discovers them (per ERRC asymmetry: a registered no-action is a permanent entropy sink).
- **Hardware-Level Impact:** none (cold paths).
- **Baseline Cost:** none material.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** none — these stay as-is. Register:
  - **Cold codegen/projection paths** (Agent 09): all `crates/rdf` projections (`obo_graphs/mapping.rs` 190 alloc sites, csvw, jsonld context compiler, viz), shapes linkml/json_schema/graphql/typescript/pydantic, retrieval admission/compile/plan, error-path `format!` (protocol.rs, update.rs, union.rs error arms). Once-per-document/query.
  - **Checked-`Result` contract sites** (Agent 16) — saturating conversion FORBIDDEN, would mask real bound bugs: modifier.rs:678 `checked_mul`, bgp.rs:1316 `checked_add(1)`, bgp.rs:2000 `checked_shl`, all `WorkspaceBoundOverflow` sites; governor early-exit control flow (~15 sites in modifier.rs/path.rs/binop.rs — converting changes refusal semantics).
  - **Byte-exact scoring** (Agent 16): `crates/text/src/ranking.rs` BM25F — `purrdf-bm25f-fixed-v2` profile, never convert.
  - **Load-bearing branchy min/max** (Agent 16): bgp.rs:1458-1460 axis_domains floor-clamp is already minimal (bare `f64::clamp` would panic on `rows < 1.0`); hnsw relation.rs:878-882 min-chains are the clamp-never-refuse mechanism (docs/design/purrdf-embedding-knn.md:246).
  - **Where NOT to impose the 8-hop ceiling** (Agent 13): `PathLimits` witness engine (path_relation.rs:492, already bounded + spec'd by tests), datalog chase round loop (budget-bounded via StepMeter/stop), n3 iteration budget, datalog `shortest_path` (diagnostics — node-count cap with typed `None`, not 8).
  - **Synchronization constructs to document, not rewrite** (Agent 15): all Tier-3/Tier-4 sites — `OnceLock`/`LazyLock` statics (write-once, ideal zero-IPC), row_checkpoint.rs:174 single-evaluator mutex-for-Sync, workspace/evidence mutexes, segmented session `Mutex<File>` (OS-required), bench tally, plan/mod.rs:243 slot free-list (in-repo model), parallel.rs block slots (correct, padding only — see OPT-15).
  - **Dispatch documented as cold** (Agent 10): `RdfCodec` media-type dispatch (codec.rs:140, once per parse, doc says cold), `Box<dyn io::Write>` egress (I/O-bound), `dyn Error` boilerplate (~1,000 `fn source()` impls).
  - **Already-fast, verified no-action** (Agents 01/11): json string body scanning (read.rs:645-668 chunked kernel), CSV StopSet, markdown LINE_FEED, unicode prefix_len (u64 SWAR); `TermId` (NonZeroU32 niche), `QuadRow` (16 B pinned — never align to 64), solution SmallVec rows (API-intrinsic), parallel chunking (no false sharing), `view_accounting.rs` atomics.
  - **In-place PIVOT/ASOF constraint note** (Agent 07): PIVOT/ASOF are SQL-text-delegated to frozen-duckdb today (compiler.rs:2105-2250); in-Rust in-place execution must be built fresh against `columnar` primitives and gated to the ≤64 envelope (companion to OPT-05/OPT-21).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A.
- **Allocation Invariant:** N/A.
- **Benchmark Target:** N/A.
- **Layer Boundary:** none of the registered sites may be silently converted by a future lane without citing this register and revisiting its reason.

# v26.10.26 Optimization Wave — Ticket Index

20-lane adversarial inspection of the purrdf workspace (2026-10-26) reduced to OPT tickets.
Every ticket follows the 4-section schema: Target Context / Mechanical Pathology /
Proposed Speed-Limit Replacement / Verification Invariant & Acceptance Criteria.
No source code was modified by the inspection; tickets are engineering specs only.

## Ticket index

| ID | File | Lane | Primary crates |
|---|---|---|---|
| OPT-01 | `OPT-01-wire-simd-delimiter-scans.md` | Agent 01 (SWAR scans) | lex, rdf-core |
| OPT-02 | `OPT-02-token64-borrowed-token-path.md` | Agent 02 (tokens) | lex, rdf-core, rdf, xsd |
| OPT-03 | `OPT-03-eytzinger-wiring-lower-bound.md` | Agent 03 (Eytzinger) | rdf-core, datalog |
| OPT-04 | `OPT-04-bitmask-rule-execution.md` | Agent 04 (bitmask hooks) | sparql-eval, entail, validate |
| OPT-05 | `OPT-05-sql-rdf-dag-interning.md` | Agent 05 (SQL-in-RDF) | sql, duckdb |
| OPT-06 | `OPT-06-sparql-row-envelope-64.md` | Agent 06 (row envelope) | sparql-eval |
| OPT-07 | `OPT-07-duckdb-eval-per-cell-allocs.md` | Agent 07 (PIVOT/ASOF, F2/D3) | duckdb |
| OPT-08 | `OPT-08-arena-admission-impl.md` | Agent 08 (arena) | lex, sparql-eval |
| OPT-09 | `OPT-09-hot-path-heap-elimination.md` | Agent 09 (heap sweep) | rdf-core, sparql-eval, retrieval |
| OPT-10 | `OPT-10-dyn-monomorphization-closed-sets.md` | Agent 10 (dyn Traits) | sparql-algebra, datalog, geo-kernel |
| OPT-11 | `OPT-11-cache-line-geometry-soa.md` | Agent 11 (layout) | rdf-events, rdf-core, hnsw |
| OPT-12 | `OPT-12-branchless-sorting-networks.md` | Agent 12 (sorting) | rdf-core, datalog, hnsw, sparql-eval |
| OPT-13 | `OPT-13-hop-ceiling-visited64.md` | Agent 13 (hop ceiling) | sparql-eval, entail, n3, shapes |
| OPT-14 | `OPT-14-bitset-intersection-leapfrog.md` | Agent 14 (bitset joins) | rdf-core, datalog, sparql-eval |
| OPT-15 | `OPT-15-hot-lock-sharding.md` | Agent 15 (zero-IPC) | sparql-eval, hnsw, validate |
| OPT-16 | `OPT-16-saturating-arithmetic.md` | Agent 16 (saturation) | sparql-eval, retrieval, gts |
| OPT-17 | `OPT-17-arrow-egress-wiring.md` | Agent 17 (Arrow framing) | arrow, sparql-results, n3, gts |
| OPT-18 | `OPT-18-streaming-blake3-receipts.md` | Agent 18 (BLAKE3 receipts) | rdf-core, rdf, gts |
| OPT-19 | `OPT-19-static-ast-admission-gate.md` | Agent 19 (AST gate) | sparql-eval |
| OPT-20 | `OPT-20-primitives-bench-suite.md` | Agent 20 (bench/layers) | rdf-core, testkit |
| OPT-21 | `OPT-21-columnar-materialization.md` | Agent 07 (columnar F4-F6) | columnar |
| OPT-22 | `OPT-22-n3-cps-to-stack.md` | Agent 04/13 (N3 CPS) | n3 |
| OPT-23 | `OPT-23-registry-interning-open-dyn-flags.md` | Agent 04/10 (registries) | sparql-eval, shapes |
| OPT-24 | `OPT-24-cold-path-and-noaction-register.md` | consolidated | workspace-wide |
| OPT-25 | `OPT-25-eytzinger-wiring.md` | Agent 03 | rdf-core, datalog |
| OPT-26 | `OPT-26-dyn-monomorphization.md` | Agent 10 | sparql-algebra, datalog, geo-kernel |
| OPT-27 | `OPT-27-sql-termid-dag.md` | Agent 05 | sql, duckdb |
| OPT-28 | `OPT-28-row-envelope-sparql-eval.md` | Agent 06 | sparql-eval |
| OPT-29 | `OPT-29-inplace-pivot-asof-egress.md` | Agent 07 | duckdb, columnar |
| OPT-30 | `OPT-30-arena-admission.md` | Agent 08 | lex, sparql-eval |
| OPT-31 | `OPT-31-scratch-interner-arena.md` | Agent 08 | rdf-core, sparql-eval |
| OPT-32 | `OPT-32-sorting-networks-cmov.md` | Agent 12 | rdf-core, datalog, hnsw |
| OPT-33 | `OPT-33-leapfrog-seek.md` | Agent 14 | rdf-core, datalog, sparql-eval |
| OPT-34 | `OPT-34-bitset-foq-intersection.md` | Agent 14 | rdf-core |
| OPT-35 | `OPT-35-hop-ceiling-visited64.md` | Agent 13 | sparql-eval, entail, n3, shapes |
| OPT-36 | `OPT-36-canon-scratch-buffers.md` | Agent 09 | rdf-core, sparql-eval, retrieval |
| OPT-37 | `OPT-37-blake3-streaming-receipts.md` | Agent 18 | rdf-core, rdf, gts |
| OPT-38 | `OPT-38-ast-admission-gate.md` | Agent 19 | sparql-eval |
| OPT-39 | `OPT-39-bench-and-helpers-receipt.md` | Agent 20 | rdf-core, testkit |
| OPT-40 | `OPT-40-layout-geometry.md` | Agent 11 | rdf-events, rdf-core, hnsw |
| OPT-41 | `OPT-41-branchless-reuse-dedupe.md` | reuse-audit lanes | branchless, rdf-core |
| OPT-43 | `OPT-43-csv-portable-swar-kernel.md` | ticket-writer §3.5 | rdf-core — LANDED (both halves; wave-5 audit) |
| OPT-44 | `OPT-44-hnsw-finite-lane-mask.md` | ticket-writer §3.5 | hnsw |
| OPT-45 | `OPT-45-sparql-small-n-position-scans.md` | ticket-writer §3.5 | sparql-eval — landed, UNVERIFIED |
| OPT-46 | `OPT-46-n3-lexer-digit-run-swar.md` | ticket-writer §3.5 | n3 |
| OPT-47 | `OPT-47-order-by-key-reprojection.md` | ticket-writer §3.5 | sparql-eval — EXECUTED |
| OPT-48 | `OPT-48-path-relation-small-n-sorts.md` | ticket-writer §3.5 | sparql-eval — landed, UNVERIFIED |
| OPT-49 | `OPT-49-datalog-sources-small-n-sorts.md` | ticket-writer §3.5 | datalog — landed, UNVERIFIED |
| OPT-50 | `OPT-50-mutable-graph-scan-indices.md` | ticket-writer §3.5 | rdf-core — EXECUTED |
| OPT-51 | `OPT-51-n3-lexer-digit-run-swar.md` | ticket-writer §3.5 | n3 — superseded → OPT-46 |
| OPT-52 | `OPT-52-embedding-padding-scan-swar.md` | ticket-writer §3.5 | rdf-core — EXECUTED |
| OPT-53 | `OPT-53-space-filling-keys.md` | ticket-writer (bcinr) | branchless, geo-kernel — landed (cites verified, wave-5) |
| OPT-54 | `OPT-54-bmi-bitop-family.md` | ticket-writer (bcinr) | branchless — landed, UNVERIFIED |
| OPT-55 | `OPT-55-swar-horizontal-reduce.md` | ticket-writer (bcinr) | branchless — landed, UNVERIFIED |
| OPT-56 | `OPT-56-branchless-containers.md` | ticket-writer (bcinr) | branchless — landed, UNVERIFIED |
| OPT-57 | `OPT-57-leb128-base64-homes.md` | ticket-writer (bcinr) | rdf-core, branchless |
| OPT-63 | `OPT-63-opt41-followups.md` | ticket-writer (verify) | branchless — T-C landed, UNVERIFIED |
| OPT-66 | `OPT-66-arrow-batch-count-validation.md` | ticket-writer | arrow — DONE (130fb2f0), residual future |
| OPT-67 | `OPT-67-arrow-branchless-kernels.md` | ticket-writer (2026-10-27) | branchless, arrow |
| OPT-68 | `OPT-68-gts-canonarrow-cross-check.md` | ticket-writer (2026-10-27) | gts, arrow |
| OPT-69 | Python binding drops the provenance exit gate | — OPEN (optimization/OPT-69-python-provenance-exit-gate.md) |
| OPT-70 | CI-required follow-ups: lock missing 3 members, bounded-gates ladder single-source, generated/ coverage, guard vs documented commands | — OPEN (optimization/OPT-70-ci-required-followups.md) |
| OPT-71 | Error-surface deep items + wasm class parity gate | — OPEN (optimization/OPT-71-error-surface-and-wasm-class-gate.md) |
| OPT-5S | `../OPT-5s-bounded-ceiling.md` | Agent C14 | bounded wrapper + hook + corpus, ENFORCED |

Note (2026-10-27): `OPT-64 and OPT-65-frozen-duckdb-endgame.md` exists on disk but has no
index row above — pre-existing drift, recorded here rather than silently
repaired (row owner unknown). OPT-66/67/68 absorb the wave's OPT-18/19/20
request; those IDs were taken by the Agent 18/19/20 wave tickets, so the next
free IDs were used (renumbering notes in each file header).

**Renumbering note:** the 16 parallel-writer duplicate-ID files were renumbered to
OPT-25…OPT-40 (filenames and internal `[OPT-NN]` headings updated); the OPT-01…OPT-24
table above is authoritative for those IDs. Every OPT-01…OPT-40 ID now maps to exactly
one file.

## `purrdf::branchless` mapping

The shared branchless home `crates/branchless` (exposed as `purrdf::branchless`) is ported
from BranchlessCInRust (`~/bcinr` @ commit `1e92b213`; no_std, forbid(unsafe_code),
zero-alloc) and referenced by the tickets it implements kernels for. All 18 modules
landed: `swar`, `char_class`, `structural`, `eytzinger`, `search`, `hash`, `sort_small`,
`sort16`, `select`, `bitset`, `bitmap`, `filter`, `saturation`, `scaling`, `cost_math`,
`packing`, `circuit_breaker`, `simd_fallback`.

- OPT-01 → `branchless::swar`, `branchless::structural` (scan surface; `simd_fallback`
  is the ct-op home, not a scan home)
- OPT-02 → `branchless::swar`, `branchless::char_class` (token-validation kernels:
  `classify_bytes`, `byte_lane_in_range`, `parse_4_hex_digits`, `parse_hex_u32`,
  `parse_decimal_u64`, `is_all_ascii`)
- OPT-03 → `branchless::eytzinger` (single home incl. `lower_bound_eytzinger`)
- OPT-05 → `branchless::hash` (`wyhash_64` / `wymix` DAG node hash-cons)
- OPT-12 → `branchless::sort_small`, `branchless::sort16` (XOR-mask cmov compare-exchange)
- OPT-13 → `branchless::bitset` (Visited64 groundwork: `is_subset_u64`,
  `intersect/union/difference_u64_slices`, `any_bit_set_u64_slice`)
- OPT-14 → `branchless::bitset`, `branchless::bitmap` (word set-logic incl. ANDNOT,
  linear-counting)
- OPT-16 → `branchless::saturation`, `branchless::scaling` (`mul_sat_u64` wide-mul
  mask-select, `clamp_i64`, Q16 `fp_mul/fp_div`, `muldiv`)
- OPT-43 → `branchless::swar` (`find_byte_mask_word`, swar.rs:212 — wired in
  `crates/rdf-core/src/csv/scan.rs:22,121`)
- OPT-45 → `branchless::search::first_equal_lane_u64x8`
  (`crates/branchless/src/search.rs:133`; re-exported/wired in
  `crates/sparql-eval/src/solution.rs:187`) — landed, UNVERIFIED (no-build
  machine)
- OPT-53 → `branchless::packing` (Morton spread/compact 2D, packing.rs:319,337;
  geo-kernel Hilbert composition per ticket) — landed, UNVERIFIED
- OPT-54 → `branchless::bitset` (BMI trio + funnel shifts, bitset.rs:380,398) —
  landed, UNVERIFIED
- OPT-55 → `branchless::swar` horizontal family (swar.rs:632 `horizontal_and_u32`
  et al.) — landed, UNVERIFIED
- OPT-56 → `branchless::sort16::FixedTopK8` + `branchless::filter::SwarTagFilter`
  (sort16.rs:303, filter.rs:141) — landed, UNVERIFIED
- OPT-63 T-C → `branchless::packing` straddling `field_extract2`/`field_deposit2`
  u128-window form (packing.rs:253-289) — landed, UNVERIFIED
- OPT-47 → sparql-eval-local (precomputed `SortKey::rank` envelope,
  `crates/sparql-eval/src/modifier.rs`; deep `total_order` only on rank ties)
- OPT-50 → `purrdf_core::FastMap` dense indices (`declared_pos`,
  `crates/rdf-core/src/ir/mutable.rs`)
 - OPT-52 → kernel exists (swar.rs:80) but view.rs:4782-4793 uses a LOCAL SWAR expression — borrow-inexact kernel, 2x divergence documented in-ticket; relocation is a coordinator call
  `crates/rdf-core/src/ir/embedding/view.rs:4786`)

Unreferenced-by-ticket modules (`circuit_breaker`, `cost_math`) are the remaining
bcinr migration surface.

## Wave execution notes (2026-10-26)

- **Note B (OPT-48, resolved):** the missing `fn` definitions landed as bookkeeping
  in `crates/sparql-eval/src/path_relation.rs` — `sort_u32_list` (:1206) and
  `sort_edges_small_n` (:1234), with the call sites (:849, :1026) now resolving and
  four oracle/permutation tests pinning the kernels (:2444, :2473, :2492, :2526).
  Bookkeeping only; landed, UNVERIFIED (no-build machine — no cargo on this lane).
- **OPT-49:** landed, UNVERIFIED (no-build machine). Divergence note: the network
  landed as `sort_sources_by_body_index` / `sort_small_by_key` in
  `crates/datalog/src/seminaive/join.rs:495-545` (same graph as
  `purrdf_branchless::sort_small::sort8`, depth 6) rather than a direct
  `purrdf_branchless::sort_small` call; both call sites swapped —
  `crates/datalog/src/seminaive/factors.rs:364,850` (was scalar `sort_by_key`).
- **OPT-46/OPT-51 dedupe:** both tickets targeted the same `read_number` digit
  loops. OPT-46 is canonical (line refs verified against the final lexer);
  OPT-51 is merged-superseded with a pointer stub.
- Executed tickets are marked in the index rows above with on-disk citations in the
  `purrdf::branchless` mapping section.

## Explicit no-action register

OPT-24 is the audit ceiling: cold codegen/projection paths, checked-`Result` contract sites,
BM25F byte-exact scoring, load-bearing branchy clamps, where-NOT-to-impose the 8-hop ceiling,
and synchronization constructs to document rather than rewrite. No future lane may convert a
registered site without citing that ticket and revisiting its reason.

## Pre-existing live failure (not introduced by this wave)

`python3 scripts/check-shared-helpers.py` currently exits 1 — 323 stale capacity-proof FAIL
lines, all one class, across `crates/rdf-core/src/ir/*` (builder, composite, dataset,
frozen_terms, mutable, event_sink, global, ingest, paged/*, segmented/build, validate) and
`crates/rdf-core/src/turtle_render.rs`. This means `make helpers-hygiene` fails today,
before any optimization work. Recorded as existing state; remediation needs its own ticket
and is not performed by this documentation effort.

See Also: `docs/jira/v26.10.26/optimization/OPT-24-cold-path-and-noaction-register.md`

Index addendum (2026-10-27): OPT-42 (top level,
SUPERSEDED header on file), OPT-64 and OPT-65 (prose-mentioned only)
are also part of this wave's ticket set; OPT-58..62 are unused ids.

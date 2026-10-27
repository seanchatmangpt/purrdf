# [OPT-40] Cache-Line Geometry: EventQuad 32 B, Per-Axis SoA Quad Tables, FastQuad Lane, HNSW CSR

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-events/src/lib.rs:212,357-370`; `crates/rdf-core/src/ir/ingest.rs:125`; `crates/rdf-core/src/ir/dataset.rs:143-154,536-551,600-608,694-698`; `crates/rdf-core/src/ir/triple.rs:190-195,285-299`; `crates/hnsw/src/graph.rs:557-562,88-113`; precedent `crates/arrow/src/header.rs:12`
- **Subsystem:** Memory Arena / Storage
- **Inspection Agent:** Agent 11 (Cache-Line Geometry & SoA Layouts)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `pub struct EventTermId(pub u64)` — no `NonZero`, so `Option<EventTermId>` is 16 B and `EventQuad { s, p, o, g: Option<EventTermId> }` = 40 B/align 8 (rdf-events/lib.rs:212,357-370), staged in `raw_quads: Vec<EventQuad>` (ingest.rs:125). Quad permutations: five `OnceLock<Box<[u32]>>` ordinal arrays over the `Box<[QuadRow]>` table (dataset.rs:536-551, `QuadRow` 16 B size-pinned :143-154) — every probe iteration does `rows[order[i]]`: a 4 B sequential read plus a 16 B gather across ~4 cache lines per 4 hits, and permutation *builds* touch `{s,p,o,g}` scattered. `TripleDataset` holds 10 lazily-materialized permutations over 12 B rows (triple.rs:190-195,285-299). HNSW `layers: Vec<Vec<Vec<Ranked>>>` — one adjacency probe = pointer-chase + 16 B elements; layer-0 traversal is the hottest loop in the index (graph.rs:557-562).
- **Hardware-Level Impact:** gather-heavy probes (4 lines per 4 hits), 20% oversized ingest staging rows, pointer-chasing adjacency walk; false sharing ruled out (parallel scan chunking and accounting atomics audited clean).
- **Baseline Cost:** 40 B vs 32 B per staged quad; gather vs sequential SoA reads on every permutation build/probe.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) **P-3 (trivial first)**: `EventTermId(pub NonZeroU64)` mirroring `TermId(NonZeroU32)` (ir/term.rs:62-63) → `Option` niche-packs to 8 B → `EventQuad` = 32 B exactly (2/line), with a `size_of::<EventQuad>() == 32` pin beside the existing `QuadRow == 16` pin. (2) **P-2**: keep the `OnceLock<Box<[u32]>>` ordinals but build permutations per-axis SoA (sort key arrays, no 16 B row gathers) and let probes touch only bound axes' columns; the 0 = default-graph sentinel already exists (`axis_key`, dataset.rs:600-608). (3) **P-1 FastQuad lane** (only if SIMD residual filtering justifies it): `#[repr(C, align(64))]` group-of-four-quad lane — `rows: [[TermId; 3]; 4]` (48 B) + `graphs: [u32; 4]` (16 B) = 64 B exactly, zero padding, table-aligned so every unrolled/SIMD loop starts on a line boundary; `GraphMatch::Any/Default` filtering reads one contiguous 16 B word per lane (one `movdqu`), matching the `QuadKey` mask design (dataset.rs:694-698). Tail `Box<[QuadRow]>` for `len % 4`. Do NOT align individual 16 B quads to 64 B (4× inflation). (4) **HNSW CSR**: flat `levels`/offsets + one `Box<[u32]>` neighbor-id arena per layer replacing the triple-nested `Vec<Vec<Vec<Ranked>>>` (biggest traversal win; a `Graph` rewrite — separate phase). (5) Cosmetic: `#[repr(C)]` on `QuadKey` to pin the two `[u32;4]` arrays contiguous for future `std::simd`. Existing good shapes (do not touch): `TermId` 4 B niche-packed, `QuadKey` mask kernel, columnar crate (SoA exemplar), `VectorMatrix` flat data, SPARQL solution rows (AoS intrinsic to the row API).
- **Hard Bounds Applied:** FastQuad lane = exactly 64 B, 4 quads; EventQuad = exactly 32 B; compile-time size pins assert both.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct EventTermId(pub NonZeroU64);
  #[repr(C, align(64))] pub struct FastQuad { rows: [[TermId; 3]; 4], graphs: [u32; 4] }
  const _: () = assert!(size_of::<FastQuad>() == 64 && size_of::<EventQuad>() == 32);
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `GraphMatch` lane filtering is mask-AND over one aligned word; no per-quad branches added.
- **Allocation Invariant:** layouts are allocation-neutral (pinned sizes); CSR adjacency is one arena per layer.
- **Benchmark Target:** ingest staging 20% smaller (EventQuad); permutation build/probe throughput vs gather baseline on the pack/dataset benches; HNSW layer-0 traversal measurable in existing hnsw benches.
- **Layer Boundary:** `check-layers.py` clean; layout changes must not alter any serialized byte format (frozen vectors untouched — pack/dataset formats are separate from staging structs).

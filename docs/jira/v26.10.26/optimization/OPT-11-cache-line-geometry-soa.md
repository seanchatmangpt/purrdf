# [OPT-11] Cache-Line Geometry: EventQuad 40→32 B, Per-Axis SoA Quad Tables, FastQuad 4-Lane

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-events/src/lib.rs:212,357-370`; `crates/rdf-core/src/ir/ingest.rs:125`; `crates/rdf-core/src/ir/dataset.rs:143-150,542-551,600-608,694-698`; `crates/rdf-core/src/ir/triple.rs:190-195,285-299`; `crates/hnsw/src/graph.rs:546-562`
- **Subsystem:** Ingestion | Memory Arena
- **Inspection Agent:** Agent 11 (Cache-Line Geometry & SoA Layouts)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `pub struct EventTermId(pub u64)` — no `NonZero`, so `Option<EventTermId>` is 16 B (tag word) and `EventQuad { s,p,o: EventTermId, g: Option<EventTermId> }` = 8+8+8+16 = **40 B, align 8** (rdf-events/src/lib.rs:212,357-370), staged in `raw_quads: Vec<EventQuad>` (ingest.rs:125). Frozen-store probes iterate `rows[order[i]]` over five `OnceLock<Box<[u32]>>` ordinal permutations (`pos, osp, gspo, gpos, gosp`, dataset.rs:542-551) against the 16 B `Box<[QuadRow]>` table — a 4 B sequential read followed by a 16 B **gather** across ~4 cache lines per 4 hits, and permutation builds scatter-touch `{s,p,o,g}` of every row. `TripleDataset` repeats the geometry with 10 lazy permutations over 12 B `TripleIds` rows (triple.rs:190-195,285-299). HNSW adjacency is triple-nested `layers: Vec<Vec<Vec<Ranked>>>` (graph.rs:557-562): one probe = pointer-chase + 16 B `Ranked` (`f64`+`usize`) elements — the hottest loop in the index.
- **Hardware-Level Impact:** 20% wasted staging bytes per ingest quad; 4-line gather per permutation probe (L1/L2 miss per hit vs. 1 line for SoA column reads); pointer-chase + unpredictable branch per HNSW layer-0 neighbor step.
- **Baseline Cost:** ~20 B/quad warm index overhead over the row table (dataset.rs doc); one dependent cache miss per permutation level; one dependent load per HNSW adjacency vector.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal** (Agent 11 action ranking):
  1. **F-07/P-3 — `EventTermId(NonZeroU64)`**: mirror the `TermId` niche precedent (`crates/rdf-core/src/ir/term.rs:55-63`): `pub struct EventTermId(pub NonZeroU64)`. `Option<EventTermId>` niche-packs to 8 B → `EventQuad` = **32 B**, exactly 2/cache line — 20% off the ingest staging buffer, same win P3a already claimed for `QuadRow` (`dataset.rs:152-154`). Add compile-time pin `size_of::<EventQuad>() == 32`.
  2. **F-03/F-06/P-2 — per-axis SoA quad/triple tables**: keep the `OnceLock<Box<[u32]>>` ordinal arrays but iterate **per-axis SoA over rows** for permutation *builds* (sort 5 key arrays instead of gathering 16 B rows); for *probes*, keep the u32 gather but only touch bound axes' columns.
  3. **P-1 — `FastQuad` 4-lane batch unit** (cache-line-correct unit is 4 quads, NOT one 16 B row aligned to 64):
     ```rust
     #[repr(C, align(64))]
     #[derive(Clone, Copy)]
     pub struct FastQuad {
         rows: [[TermId; 3]; 4],   // bytes 0..48 : s,p,o of quads 0..3
         graphs: [u32; 4],         // bytes 48..64 : g sentinel 0 = default graph
     }                             // 64 B exactly, zero padding
     const _: () = assert!(size_of::<FastQuad>() == 64);
     ```
     `graphs` grouping exploits the `axis_key` `None → 0` sentinel already defined at dataset.rs:600-608, so `GraphMatch::Any`/`Default` filtering reads one contiguous 16 B word per lane (one `movdqu`), matching the `QuadKey` mask design (dataset.rs:694). Table type: `Box<[FastQuad]>` + tail `Box<[QuadRow]>` for `len % 4`.
  4. **F-13 — HNSW CSR adjacency**: replace `Vec<Vec<Vec<Ranked>>>` with flat `levels`/offsets + one `Box<[u32]>` neighbor-id arena per layer (CSR-style). Biggest traversal win, but a rewrite of `Graph`. `VectorMatrix` (graph.rs:88-113) is already flat/SoA — no action. Build-time-only `Edge` (32 B, graph.rs:546-554) not worth repacking.
  5. **F-05 — `#[repr(C)]` on `QuadKey { key: [u32;4], mask: [u32;4] }`** (dataset.rs:694-698) to pin array contiguity for future explicit `std::simd`; cosmetic today.
- **Hard Bounds Applied:** `size_of::<EventQuad>() == 32`; `size_of::<FastQuad>() == 64`, `align_of == 64`; `QuadRow` keeps its existing 16 B `size_of` pin (dataset.rs:154).
- **Proposed Type Layout & Signatures:** `FastQuad` above; `EventTermId(NonZeroU64)`; per-axis tables `s: Box<[u32]>, p: Box<[u32]>, o: Box<[u32]>, g: Box<[u32]>` (0 = default-graph sentinel, reusing dataset.rs:600-608); HNSW CSR `levels: Box<[u32]>, offsets: Box<[u32]>, neighbors: Box<[u32]>` per layer.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `GraphMatch` filtering over `FastQuad::graphs` compiles to one vector load + mask compare per 4-lane batch (no per-row graph `Option` branch).
- **Allocation Invariant:** layouts only — zero runtime allocation change except the 20% staging reduction from P-3; compile-time size pins enforce geometry.
- **Benchmark Target:** permutation build (freeze) ≥2× throughput (sequential per-axis sorts vs. scattered row gathers); HNSW layer-0 search improvement on the existing `hnsw` bench targets.
- **Layer Boundary:** all changes stay within `rdf-events`, `rdf-core` ir, and `hnsw` per `layers.toml`; zero violations via `check-layers.py`.

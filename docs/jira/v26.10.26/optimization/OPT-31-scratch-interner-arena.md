# [OPT-31] ScratchInterner: Ranges Into an Arena, Not Owned TermValue Strings

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sparql-eval/src/scratch.rs:221-252,268,1231,1043-1044`; `crates/sparql-eval/src/expr.rs:6955-7027,6993-6997`; `crates/sparql-eval/src/solution.rs:460,869-873,862-865`; reference design `crates/rdf-core/src/ir/builder.rs:320-325,447-502,545`
- **Subsystem:** Memory Arena
- **Inspection Agent:** Agent 08 (escape sites C1-C6); Agent 02 (XSD scalars feed)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `ScratchInterner { values: Vec<TermValue>, ghosts: Vec<Option<TermValue>>, index: HashTable<(ScratchId, u64)>, charged_bytes: AtomicU64 }` (scratch.rs:221-252) — but `TermValue` is `Iri(String)`, `Literal { lexical_form: String, datatype: String, … }` (ir/term.rs:409-439): every minted scratch term costs 1-3 separate global `String` allocations + a HashTable insert, charged but not confined. CONCAT builds the concatenated buffer plus two more Strings per mint (`workspace::string(datatype, …)` / `workspace::string(lang, …)`, expr.rs:7004-7017) — 2-3 extra heap allocations per evaluated row, then hands to `intern_workspace_term` into escape site C1. Per-row `SmallVec` spill past 4 columns (solution.rs:460) is N disjoint blocks. `format!`/`to_string()` bypass the codebase's own `Memory::format`/`format_exact` discipline (agg_fn.rs:1545, modifier.rs:5154-5158, convert.rs:479 vs allocation.rs:882-894, workspace.rs:2440-2479).
- **Hardware-Level Impact:** the single largest structural divergence from the `Interner` arena design (builder.rs:320-325); allocator lock + fragmentation per minted term; dedup-by-value impossible across Strings without re-hashing payloads.
- **Baseline Cost:** 1-3 global allocations + 1 hash insert per scratch term mint; 2-3 extra per CONCAT row.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** mirror `Interner` (builder.rs:320-325) inside `ScratchInterner`: one `arena: Vec<u8>` byte region + `StrRange`-backed entries replacing `TermValue` payloads, dedup-by-value before push (`push_str_with_memory`, builder.rs:545); growth admitted via the OPT-09 `ArenaAdmission` (or the existing `Memory` seam) — ranges replace `String`s. CONCAT: intern datatype/language IRIs — they come from a tiny closed set (`XSD_STRING`/`RDF_LANG_STRING`/`RDF_DIR_LANG_STRING`, expr.rs:6993-6997) — to constants, eliminating 2 of 3 allocations. Wide-row spill: store rows as `(len: u16, cells_offset: u32)` into a flat cell array (kills per-row SmallVec spill blocks). Diagnostics route through the existing `Memory::format`/`format_exact`. Take/drain helpers (`take_values_from`, scratch.rs:1231) return ranges/handles, not fresh `Vec`s. Placement order by escape magnitude: C1 scratch arena → C2 CONCAT datatype set → C3 row spill → C4 accumulator boxes (arena-emplaced with drop tracking, or cross-ref OPT-04 enum) → C5/C6 format discipline.
- **Hard Bounds Applied:** scratch text confined to the 32 MiB arena (OPT-09 cap); datatype set closed at 3 constants.
- **Proposed Type Layout & Signatures:**
  ```rust
  struct ScratchEntry { range: StrRange, kind: TermKind, datatype: DatatypeId /* 3-variant enum */ }
  struct ScratchInterner { arena: Vec<u8>, entries: Vec<ScratchEntry>, index: HashTable<(ScratchId, u64)> }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation ticket); mint path is hash→dedup→range-append.
- **Allocation Invariant:** scratch term mints = 0 global allocations once the arena is warm; CONCAT = 1 append (no datatype/lang Strings); alloc-probe receipt required.
- **Benchmark Target:** eval with heavy minting/CONCAT shows ≥10× allocation-count reduction (OPT-20 suite); existing solution/scratch tests byte-identical.
- **Layer Boundary:** change confined to `sparql-eval` (seam imported from `purrdf-lex`); `check-layers.py` clean; `helpers-ledger.toml` — interner pattern reuses the rdf-core home's design, recorded as a second sanctioned consumer, not a new body.

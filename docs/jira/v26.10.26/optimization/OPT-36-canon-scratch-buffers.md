# [OPT-36] Canon and Ingest Allocation Elimination (RDFC-1.0 Scratch, Per-Edge Term Clones)

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/ir/canon.rs:2462,2484,2506,2680-2698`; `crates/rdf-core/src/ir/skolem.rs:650`; `crates/rdf-core/src/ir/ingest.rs:82-95`; `crates/sparql-eval/src/path_relation.rs:812-814,1100-1102,1548-1554`; `crates/retrieval/src/fuse.rs:392`, `execute.rs:1725`; `crates/lex/src/xml/reader.rs:748,1246`
- **Subsystem:** Ingestion / Attestation
- **Inspection Agent:** Agent 09 (Heap Allocation Eliminator)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `hash_first_degree` allocates `let mut line = String::new()` + `let mut lines = Vec::new()` **per call**, called O(degree) times per node (canon.rs:2462); `hash_n_degree` deep-clones the issuer (BTreeMap id→label) per candidate permutation step (`candidate.issuer.clone()`, canon.rs:2484/2506); `format!("{position}.o")`-family per nested component (:2680-2698); `let iri = format!("{}{}", self.prefix, encode_blank(label, scope));` per blank node (skolem.rs:650); `RawTerm::from_event` `to_owned()` per term component (ingest.rs:82-95); per-matched-edge `nodes.push(from.clone())` + per-statement `TermBox::new(subject.clone())` ×3 and 6-way term clones (path_relation.rs:812-814,1100-1102,1548-1554); per-fused-row `retained.push(ScopedFusedRow::new(row.clone(), evidence.clone())?)` (fuse.rs:392); `attested.clone()` per materialized row (execute.rs:1725); per-char `Cow::Owned(c.to_string())` on character-reference expansion (xml/reader.rs:748).
- **Hardware-Level Impact:** the canon cluster is the hottest allocation site in the workspace (O(nodes × degree) allocations + O(permutations) BTreeMap deep clones); per-row/per-edge clones multiply through join and retrieval paths.
- **Baseline Cost:** allocation per incident quad / per permutation / per edge / per row.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) canon: reusable scratch `String`/`Vec` in `&mut self` (`.clear()` per line) or arena `Memory` (cross-ref OPT-09); replace the per-permutation issuer clone with an undo-log (journal) issuer with rollback or a persistent map; component suffix `format!("{position}.o")` → reuse a scratch position buffer pushing only the differing last segment. (2) skolem: `self.scratch.clear(); push_str(prefix); …; intern_iri(&self.scratch)` — one alloc per blank node removed. (3) ingest: lifetime-parameterized `RawTerm<'a>` borrowing `EventTerm<'a>`, deleting the intermediate owned copy (Eliminate). (4) path_relation: collect `TermId`s (Copy) instead of `TermValue` clones, map to `TermValue` once post-dedup; restructure `TermBox::new` to move. (5) retrieval: fuse.rs — only one container needs to own the row (build `ScopedFusedRow` first, clone into `rows` only when `retained` is `None`); evidence becomes a borrowed handle; execute.rs:1725 → `Rc<Row>`/arena share. (6) XML charref: `Cow::Borrowed` of a small-string/scratch instead of `c.to_string()`. (7) shapes dispatch keyed by interned `TermId` instead of cloned `TermValue` (`engine.rs:784-795,1216,1238`); violation records hold `ShapeId`/ids instead of owned IRIs (constraints.rs:3502-3504,3678-3680).
- **Explicit no-action (cold, audited):** all of `crates/rdf` projections (obo/csvw/jsonld/viz — once-per-document transforms), shapes LinkML/JSON-Schema/GraphQL/Pydantic codegen, retrieval admission/compile/plan `format!` reason strings, error paths in union.rs/protocol.rs, XML DTD/attlist loops.
- **Hard Bounds Applied:** scratch buffers reset per call-site unit (clear, not realloc); undo-log depth = candidate stack depth (bounded by the canon recursion cap).
- **Proposed Type Layout & Signatures:**
  ```rust
  struct CanonScratch { line: String, lines: Vec<[u8; 32]>, issuer_journal: Vec<(u32, u64)> } // undo-log entries
  enum RawTerm<'a> { Iri(&'a str), Blank(&'a str), Literal { lexical: &'a str, … } }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation ticket); canon hashing path becomes clear-refill loops.
- **Allocation Invariant:** canon hash_first_degree/hash_n_degree allocate 0 steady-state (scratch + journal); byte-identity of RDFC-1.0 output mandatory (frozen conformance vectors — no canonicalization output change).
- **Benchmark Target:** canon bench (existing) allocation count ≥ 10× ↓ on degree-heavy datasets; alloc-probe receipt on the retrieval/path paths.
- **Layer Boundary:** `check-layers.py` clean; canonicalization byte-exactness is the hard gate (GTS/frozen vectors untouched).

# [OPT-09] Hot-Path Heap Elimination: Canon Scratch, Path-Relation TermIds, Retrieval Row Clones

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/ir/canon.rs:2462-2698`; `crates/sparql-eval/src/path_relation.rs:812-814,1100-1102,1548-1554`; `crates/retrieval/src/fuse.rs:392`, `execute.rs:1725`; `crates/lex/src/xml/reader.rs:748,674,1246`; `crates/rdf-core/src/ir/ingest.rs:82-95`; `crates/rdf-core/src/ir/skolem.rs:650`; `crates/sparql-eval/src/expr.rs:1859-2042,3227,3721-3765,4099-4143,5376-5384`; `crates/shapes/src/engine.rs:784-795,1216,1238,3332`; `crates/shapes/src/constraints.rs:3502-3504,3678-3680,3909-3946`; `crates/shapes/src/components.rs:759-795`
- **Subsystem:** Ingestion | Relational Sieve | Egress (cross-cutting hot-path allocation sweep)
- **Inspection Agent:** Agent 09 (Heap Allocation Eliminator)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `hash_first_degree` (canon.rs:2462): `let mut line = String::new()` and `let mut lines = Vec::new()` **inside** the per-incident-quad loop; called O(degree) times per node during RDFC-1.0 canonicalization — the hottest allocation cluster in the workspace.
  - `hash_n_degree` (canon.rs:2484, 2506): `candidate.issuer.clone()` / `frame.issuer` — a full `BTreeMap` deep-clone per permutation step of the gossip search.
  - `path_relation.rs:812-814`: `nodes.push(from.clone()); nodes.push(term.clone())` per matched path edge; `:1100-1102`: `TermBox::new(subject.clone())` ×3 per recorded statement; `:1548-1554`: 6-way term clone per path row.
  - `fuse.rs:392`: `retained.push(ScopedFusedRow::new(row.clone(), evidence.clone())?)` per undecided fused row; `execute.rs:1725`: `attested.clone()` per materialized row pull.
  - `xml/reader.rs:748`: `text: Cow::Owned(c.to_string())` — a full heap `String` per expanded character reference.
  - `ingest.rs:82-95`: `RawTerm::from_event` `to_owned()` per term component — the main parse→store copy chain.
  - `skolem.rs:650`: `let iri = format!("{}{}", self.prefix, encode_blank(label, scope));` per blank node.
  - `expr.rs:1859-2042`: `out.push(v.clone())` per `Variable` in the plan walk (10 sites).
  - `engine.rs:784-795`: `entry(term.clone()).or_default().push(position)` — map keyed by cloned `TermValue` per foreign term; `:1216` `vec![term.clone()]` per target.
  - `constraints.rs:3502-3504,3678-3680`: `source_shape.clone()`, `severity.clone()` per violation record.
- **Hardware-Level Impact:** allocator lock + malloc/free traffic per element, cache pollution from scattered heap blocks, GC-of-freed-pages pressure in canon's O(nodes × degree) inner loop; every clone is a hidden global-allocator call on the hot path.
- **Baseline Cost:** ~4,800 raw alloc-in-loop sites surveyed, clustered to ~230 hot sites in rdf-core, ~410 in sparql-eval (warm subset), ~200 in retrieval, 33 in lex; canon.rs alone dominates by frequency (per-quad, per-permutation).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal** (in priority order):
  1. **Canon scratch buffers**: hoist `line`/`lines` to reusable `&mut self` scratch (`self.scratch.clear()` per line; reusable `Vec` cleared per call). Issuer deep-clone → **undo-log (journal) issuer**: record `(key, prior_label)` mutations and roll back on backtrack, or a copy-on-write persistent map. Both keep the canonical output byte-identical.
  2. **Path relations go id-first**: collect `TermId` (`Copy`, 4 B) instead of `TermValue` clones during edge/statement walks; `sort_unstable + dedup` on ids; materialize `TermValue` once post-dedup — O(edges) clones → O(unique nodes). Restructure `TermBox::new(subject.clone())` sites to move the original and clone only where the row genuinely retains ownership.
  3. **Retrieval row plumbing**: `fuse.rs:392` — build `ScopedFusedRow` first and clone into `rows` only when `retained` is `None` (undecided rows currently double-store); take `ScopedFusedRow::new(row, &evidence)` borrowed evidence handle. `execute.rs:1725` — share via `Rc<Row>` (or arena handle) between attested and streamed use instead of per-row clone.
  4. **Ingest/xml**: `RawTerm<'a>` borrowing `EventTerm<'a>` (or intern directly from `&str` event terms, deleting the owned intermediate); xml charref expansion → small-string/scratch `String` reused across events, emit `Cow::Owned` only when > inline; skolem IRI → `self.scratch.clear(); push_str(prefix); push_str(encoded); intern_iri(&self.scratch)` — one alloc fewer per blank node.
  5. **Plan walks**: `SchemaBuilder::push(&Variable)` borrowed variant (builder only needs hash/eq); `Rc<VarSchema>` sharing across union arms (`positive.rs:478` `ChoiceStep::Visit` currently clones `input` per arm).
  6. **Shapes id-keying**: dispatch maps keyed by interned `TermId` (Copy) instead of cloned `TermValue`; violation records hold `ShapeId`/`Severity` ids, not owned IRI/enum clones; id-first fast-path dedup in list-equality (`ids equal → equal`, clone only on tie-break, constraints.rs:3909-3946).
  7. **Nested-triple suffix sites** (canon.rs:2680-2698): `format!("{position}.o")` per component → shared buffer, push only the differing suffix segment.
- **Hard Bounds Applied:** zero per-element heap allocations on all listed hot loops; scratch buffers bounded by max line/batch size; id-based collections bounded by unique-node count.
- **Proposed Type Layout & Signatures:**
  ```rust
  struct CanonHasher { scratch_line: String, scratch_lines: Vec<[u8; 32]>, /* ... */ }
  struct JournalIssuer { base: BTreeMap<u64 /*node*/, u32 /*label*/>, journal: Vec<(u64, Option<u32>)> }
  impl JournalIssuer { fn set(&mut self, k, v); fn rollback(&mut self, mark: usize); fn mark(&self) -> usize; }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation invariant ticket); scratch-buffer reuse must not add per-element branches.
- **Allocation Invariant:** alloc-probe (dev-dependency lane) shows 0 allocations per `hash_first_degree` call after warmup; 0 per-edge allocations in path walks; ≤1 clone per undecided fused row; 0 per-char allocations in XML text events.
- **Benchmark Target:** RDFC-1.0 canonicalization bench improves measurably (extend existing canon bench; ratio vs. `--save-baseline`); no regression in W3C canon conformance corpus (exact digests unchanged).
- **Layer Boundary:** no new cross-crate dependencies; `RawTerm<'a>` change stays inside rdf-core's ingest seam; `check-layers.py` 270-edge set remains green; `check-shared-helpers.py` must not gain new duplicate-helper failures (reuse `TermLookup`, existing scratch idioms).
- **No-action list (cold, skip):** all `crates/rdf` projections (`obo_graphs/mapping.rs` ~190 hits, csvw, jsonld context compiler, viz), shapes linkml/json_schema/graphql/pydantic/typescript codegen, retrieval admission/compile/plan `format!` paths, error-path `format!`/`to_string` in `union.rs`/`protocol.rs`/`update.rs`, DTD-path clones (cold XML), `ir/paged` error formats.

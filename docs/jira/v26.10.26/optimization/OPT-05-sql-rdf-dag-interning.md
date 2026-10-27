# [OPT-05] SQL-in-RDF: Intern TermIds at Ingress, Compile as Flat DAG, Lower to Vector Kernels

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sql/src/compiler.rs:2398-2413,2149-2252,3279-3535,3564-3737,1989-1994,2014-2021`; `crates/sql/src/serializer.rs:126-131,310-314`; `crates/sql/src/ast.rs` (boxed-recursive AST, `ast.rs:630-634` `CompiledQuery`); `crates/duckdb/src/eval.rs:210,241-351,146-165`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 05 (SQL-in-RDF Categorical Compilation)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `struct DatasetReader { outgoing: HashMap<String, HashMap<String, Vec<RdfTerm>>> ... }` with `RdfTerm::Iri(String)` / `RdfTerm::Literal { lexical: String, datatype: Option<String> }` (compiler.rs:2398-2413) — **zero TermId interning exists in the SQL path**; every graph access does `term.clone()` of heap strings (compiler.rs:3774-3802).
  - Boxed-recursive AST: `SqlExpr::Binary { left: Box<SqlExpr>, ... }`, `ColumnRef { column: String, ... }` — tree, shared nothing, no hash-consing.
  - Compilation target is SQL *text*: `compile()` builds `String::with_capacity(256)` into `CompiledQuery { sql: String, params: Vec<SqlLiteral>, ... }` (serializer.rs:126-131); DuckDB re-parses the text (`self.conn.prepare(&compiled.sql)`, eval.rs:210). Full round trip: RDF text → String graph → String AST → String SQL → DuckDB parser → plan.
  - Parameter binding encoded **as text**: `params.push(lit.clone()); sql.push('$'); sql.push_str(&params.len().to_string());` (serializer.rs:310-314).
  - Duplicated inline serializer (`inline_select` / `write_inline_expr`, compiler.rs:3279-3535) with literals rendered in-place — a second SQL emitter that diverges from the parameterized one (composites refuse inline but render parameterized: `AstError::UnsupportedTerm` at compiler.rs:3502-3521 vs serializer.rs:419-461).
  - PIVOT/UNPIVOT as `format!` templates: `format!("PIVOT {table} ON {on_list} IN ({in_list}) USING {agg}({agg_col})")` (compiler.rs:2209), `format!("UNPIVOT ...")` (compiler.rs:2248-2249).
  - Scalar-template ladder in `compile_command` (compiler.rs:3564-3737): ~35 `format!` templates interpolating literals directly, `CompiledQuery::raw_compiled(sql, Vec::new())` — **zero parameters** (compiler.rs:3737).
  - Per-cell 4-5 allocations at egress: row loop `while let Some(row) = rows.next()?` (eval.rs:241) with `format!("{}/row_{}", ...)` + `format!("{}/{}", prefix, col)` and fresh datatype `String`s per cell (eval.rs:252-341); `Vec<Box<dyn ToSql>>` per execution (eval.rs:211-226).
  - Per-compile SHACL re-parse: `let shapes = format!("{}\n{}", include_str!(...), include_str!(...)); validate_ast_shacl(turtle, &shapes)?` before the memo is consulted (compiler.rs:1989-1994).
  - O(document) receipt payload rides in every `Arc<CompiledQuery>` clone: `cq.ast_graph_turtle = Some(turtle.to_string())` + canon bytes (compiler.rs:2005-2021).
  - View registration interpolates paths into DDL: `format!("CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_parquet(['{}'])", ...)` (eval.rs:146-165).
- **Hardware-Level Impact:** allocator lock + cache pollution per cell at egress; FFI boundary crossed per-value (row cursor, chunk API never touched, eval.rs:241); two divergent string emitters double validation work (`validate_identifier` re-runs per node per compile, serializer.rs:300-308).
- **Baseline Cost:** ~64 rows × N cols × 4-5 allocations per eval; O(document) bytes cloned per compile; per-node re-validation per compile.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal (6 steps, sequenced):**
  1. **Intern at ingress.** Replace `DatasetReader`'s `HashMap<String, …>` with `HashMap<TermId, …>` over the existing `purrdf_core` dictionary (`TermValue` → `TermId` at parse time, compiler.rs:2416-2456). The ~120 predicate IRI string literals become `const PRED_*: TermId` resolved once per process (`OnceLock` intern table). Removes per-access string clones (compiler.rs:3774-3802) and `format!("{NS}{prop}")` construction (compiler.rs:834,1455,1464,1658-1666).
  2. **Flat DAG IR instead of boxed `SqlExpr`.** `Vec<PlanNode>` arena with `PlanNode { op: Op, children: [NodeId; N], column: ColumnId, param: ParamId }`; hash-cons each node on insertion so identical subtrees share a `NodeId` — structural equality becomes `usize` compare; `expr_references_target` (compiler.rs:1809) becomes set membership. Acyclicity by construction (nodes reference lower indices only); existing depth caps (compiler.rs:287, serializer.rs:71,291) become arena-position checks, free.
  3. **Lower, don't serialize.** `lower(dag) -> DuckDbPlan` alongside the string path: filters → expression bindings, projections → `ColumnId`s, params → positional indices carried as `ParamId` on the Literal node (replacing the `$n` text protocol). PIVOT/UNPIVOT/SetOp become plan operators (`Pivot { table, on: [ColumnId], in: [TermId], agg: AggKind }`). The serializer remains only as a debug/explain renderer derived from the DAG — one emitter, killing the B5 fork.
  4. **Vector-kernel egress.** Replace the row loop (eval.rs:241-351) with chunk-level extraction: dictionary-encoded string vectors map directly onto the interned `TermId` space (dictionary index → intern index); datatype IRIs resolved from `OnceLock` constants instead of per-cell `.to_string()`. `RdfTriple` becomes `{ subject: TermId, predicate: TermId, object: TermId }`; strings materialize only at the final serialization boundary (the `purrdf_columnar` 5-table layout already models this). `crates/arrow/src/header.rs` egress needs no change.
  5. **Cache plans, not text.** Keep the BLAKE3-of-canon memo key (compiler.rs:1933) but store `Arc<PlanDag>`; receipt bytes (turtle clone + canon) move to a side table keyed by `h_ast`. Hoist the SHACL shapes parse into a `OnceLock`.
  6. **Kill the scalar-template arm.** Each `compile_command` template becomes a plan constructor (`Insert { table: TableId, cols: [ColumnId], values: [ParamId] }`, etc.). DDL/ATTACH-with-path (eval.rs:146) can never be parameterized in DuckDB — keep text there but gate behind a `PathBuf`-typed argument (never `&str`), making escape structurally impossible.
- **Hard Bounds Applied:** zero dynamic string building in the compile path; datatype IRI set closed (OnceLock table); acyclic DAG by construction; depth caps enforced as arena positions.
- **Proposed Type Layout & Signatures:**
  ```rust
  struct PlanNode { op: Op, children: [NodeId; N], column: ColumnId, param: ParamId }
  type NodeId = u32; type ParamId = u32; type ColumnId = u32;
  fn lower(dag: &PlanDag) -> Result<DuckDbPlan, AstError>;
  type RdfTriple = { subject: TermId, predicate: TermId, object: TermId }
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (allocation/dispatch-focused); DAG node dispatch is a `match` on a closed `Op` enum, no vtables.
- **Allocation Invariant:** per-cell egress allocations drop from ~4-5 to 0 for dictionary/numeric columns (batch conversion into shared buffers); compile path performs no dynamic string building except the DDL/ATTACH `PathBuf`-gated cases.
- **Benchmark Target:** existing `duckdb` benches (e.g. `sieve_bench.rs`) as baseline; the pivot→ASOF→QUALIFY pattern must show the Rust-side allocation count collapse; keep `crates/duckdb/tests/chicago_sql_operator_conformance_court.rs` green as the reference-output court.
- **Layer Boundary:** `PlanDag` lives in `crates/sql`; `crates/duckdb` consumes plans only; zero violations via `check-layers.py`. Sequencing: steps 1+6 mechanical/independent; steps 2+3 behind the existing serializer (kept green as reference); step 4 last, gated on a frozen-duckdb chunk-API capability probe (row cursor is the only path exercised today).
- **Amendment (scan handoff §3.4):** step 2's DAG node hash-consing uses the landed
  `branchless::hash::wyhash_64` as the node hash — no second hasher in `crates/sql`.
  Verified export: `pub fn wyhash_64(data: &[u8], seed: u64) -> u64`
  (`crates/branchless/src/hash.rs:56`, with `pub fn wymix(a: u64, b: u64) -> u64` at
  `hash.rs:21` for the fixed-field node mixing). For arena `PlanNode`s, hash the packed
  node fields: `wyhash_64(&node_packed_bytes, 0)` or, for pure-`u32`-field nodes,
  seed-fold `wymix(field, seed)` per field — either way one home
  (`purrdf-branchless::hash`), recorded in `helpers-ledger.toml`; duplicate hash bodies in
  `crates/sql` are forbidden by `check-shared-helpers.py`. Layer edge
  `purrdf-sql` → `purrdf-branchless` is additive (branchless deps only `purrdf-hash`,
  `layers.toml:386-391`); run `python3 scripts/check-layers.py` after the edge lands.

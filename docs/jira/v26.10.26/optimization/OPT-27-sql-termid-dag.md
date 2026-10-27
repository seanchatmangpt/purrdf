# [OPT-27] SQL-in-RDF: Intern Terms at Ingress, Compile a Flat Hash-Consed DAG, Lower to Plan

## 1. TARGET CONTEXT
- **Crate & File:** `crates/sql/src/compiler.rs:2398-2413,1933-2034,3279-3535,3537-3738`; `crates/sql/src/serializer.rs:126-314,590-595`; `crates/sql/src/ast.rs:594-634`; `crates/duckdb/src/eval.rs:146-165,210-351`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 05 (SQL-in-RDF Categorical Compilation)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `DatasetReader { outgoing: HashMap<String, HashMap<String, Vec<RdfTerm>>> }` with `RdfTerm::Iri(String)` — zero TermId interning anywhere in the SQL path (compiler.rs:2398-2413); boxed-recursive `SqlExpr` AST with owned Strings (`ast.rs`); `CompiledQuery { sql: String, params: Vec<SqlLiteral> }` — compilation target is SQL *text*, which DuckDB re-parses (`eval.rs:210` `self.conn.prepare(&compiled.sql)`); memo caches the string, not the plan (`static AST_COMPILE_MEMO: OnceLock<Mutex<HashMap<[u8;32], Arc<CompiledQuery>>>>`, compiler.rs:1933-1942) plus `turtle.to_string()` cloned per compile (:2014,2020); PIVOT rendered as `format!("PIVOT {table} ON {on_list} IN ({in_list}) USING {agg}({agg_col})")` (:2209); positional `$n` binding encoded as text (`sql.push('$'); sql.push_str(&params.len().to_string())`, serializer.rs:310-314); ~35 `format!` scalar templates with ZERO parameters (compiler.rs:3537-3738); a second, divergent inline serializer (compiler.rs:3279-3535) that refuses `StructLiteral` where the parameterized path renders it; egress row loop with `Vec<Box<dyn ToSql>>` per execution and per-cell `format!`/datatype-`String` allocation (eval.rs:211-226, 252-341).
- **Hardware-Level Impact:** full round trip RDF text → parse → String graph → String AST → String SQL → DuckDB parser; per-cell 4-5 allocations at egress; O(document) receipt payload (`turtle.to_string()`) cloned with every `Arc<CompiledQuery>`.
- **Baseline Cost:** thousands of String allocations per query compile + eval; two divergent SQL emitters kept in lockstep by hand.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal (6 steps, sequenced):**
  1. **Intern at ingress**: `HashMap<TermId, …>` over the existing `purrdf_core` dictionary; the ~120 predicate-IRI string literals become `const PRED_*: TermId` resolved once (`OnceLock`); kills per-access `term.clone()` and all `format!("{NS}{prop}")` construction (compiler.rs:834, 1455, 1464, 1658-1666).
  2. **Flat DAG IR**: `Vec<PlanNode>` arena with `PlanNode { op: Op, children: [NodeId; N], column: ColumnId, param: ParamId }`; hash-cons each node on insertion (BLAKE3 or FxHash over `(op, child ids, payload ids)`) so identical subtrees share a `NodeId` and structural equality is `usize` compare; acyclic by construction (nodes reference lower indices); existing depth caps (compiler.rs:287, serializer.rs:71, 291) become arena-position checks — free.
  3. **Lower, don't serialize**: `lower(dag) -> DuckDbPlan` — filters → `duckdb::Expression` bindings, params → positional `Vec<Value>` indices carried as `ParamId` (replacing the `$n` text protocol); PIVOT/UNPIVOT/SetOp become plan operators, not `format!` templates; the serializer survives only as a debug/explain renderer derived *from* the DAG — one emitter, eliminating the inline/parameterized fork.
  4. **Vector-kernel egress**: chunk-level extraction per column instead of the row loop; dictionary-encoded string vectors map directly onto the interned TermId space; `RdfTriple` becomes `{ subject: TermId, predicate: TermId, object: TermId }`; strings materialize only at the final serialization boundary (matches the `purrdf_columnar` 5-table layout). Needs a frozen-duckdb chunk-API capability probe first.
  5. **Cache plans, not text**: memo stores `Arc<PlanDag>` (+ prepared-statement handle); receipt bytes move to a side table keyed by `h_ast`; hoist the SHACL shapes parse (compiler.rs:1989-1994) into a `OnceLock`; shard or ArcSwap the memo lock (cross-ref OPT-15).
  6. **Kill the scalar-template arm**: each of the ~35 `format!` templates becomes a plan constructor (`Insert { table: TableId, cols: [ColumnId], values: [ParamId] }`); DDL/ATTACH-with-path (eval.rs:146-165, `pub register_parquet_view`) can never be parameterized in DuckDB — keep text there but gate the argument as `PathBuf`-typed, never `&str`, making escape structurally impossible.
  - Bugs to fix en route: `render_scalar_literal` digit heuristic and `ends_with("integer")` suffix acceptance (compiler.rs:2946-2973); PREPARE/EXECUTE double-render (compiler.rs:3694-3699); empty-projection law fork (serializer.rs:171-173 vs compiler.rs:1331-1336); `serialize_frame` ignoring its depth parameter (serializer.rs:590-595).
- **Hard Bounds Applied:** depth caps as arena-position checks; memo capacity 256 (existing); acyclic by construction.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct PlanNode { op: Op, children: [NodeId; N], column: ColumnId, param: ParamId }
  pub type PlanDag = Vec<PlanNode>;   // NodeId = usize; nodes reference lower indices only
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** DAG lowering is an acyclic index walk — no string building on the plan path.
- **Allocation Invariant:** compile allocates the arena once; egress per-cell String allocation → 0 (TermId triples); `turtle.to_string()` removed from the hot path.
- **Benchmark Target:** existing serializer stays green as the reference output (conformance courts, e.g. `duckdb/tests/chicago_sql_operator_conformance_court.rs`); compile+eval allocation count per query is the regression metric (OPT-20 suite).
- **Layer Boundary:** new IR lives in `crates/sql` (layer position per `layers.toml`); `check-layers.py` clean; frozen-duckdb chunk probe recorded before step 4 lands.

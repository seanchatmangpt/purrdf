# [OPT-14] Wire `bitset_intersect_u64` Into the Pack FoQ Path + Branchless Leapfrog Seeks

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/primitives.rs:38-41` (dead kernel), `crates/rdf-core/src/ir/pack/triples.rs:912-945` (FoQ probe), `crates/rdf-core/src/ir/dataset.rs:760` (`QuadKey::chunk_matches`), `crates/sparql-eval/src/bgp.rs:588-912` (index-nested-loop BGP), `crates/datalog/src/seminaive/cycle.rs:282-330` + `crates/datalog/src/cursor.rs:458-489` (in-repo leapfrog reference)
- **Subsystem:** Relational Sieve
- **Inspection Agent:** Agent 14 (Bitset-Parallel Frontier & Leapfrog Joins)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - Kernel dead in production: `pub fn bitset_intersect_u64(chunk_a: u64, chunk_b: u64) -> u64 { chunk_a & chunk_b }` (`primitives.rs:38-41`) — referenced only by `tests/primitives_hotpath_tests.rs:39-40`. `search_eytzinger_u64` likewise has zero production callers (see OPT-03).
  - Pack `(?,p,o)` intersection is probe-then-test, not word-parallel (`triples.rs:912-945`): decode the **shorter** FoQ delta list and test each entry — `if o_count <= p_count { Box::new(foq_list(...).filter(move |&pos| part.sp.get(pos as usize) == lp) ...) }`. Measured cost note in-tree: on WatDiv/LUBM one list was ≥32× the other in 1,090 of 1,520 calls; a merge decoded every entry of the long list. The retired merge-intersect survives only as the correctness oracle `merge_intersect_rows` (`triples.rs:1536`).
  - SPARQL BGP is pure index-nested-loop over six `u32`-ordinal permutations (`bgp.rs:588-912`): per stage, per input row, `ctx.dataset.quads_for_pattern_with_plan(&plan, s, p, o, *gm)` (`bgp.rs:707-737`) re-probes the permutation index; no hash/merge/leapfrog join, no bitset intersection anywhere.
  - A full leapfrog triejoin exists **only** in the datalog engine: seek-driven multiway intersection (`cycle.rs:282-330`: `let value = cursor.seek(run, target, ...)` then `.all(|cursor| cursor.seek(...) == Some(wanted))` + backtracking), over branchy binary-search seeks (`cursor.rs:460-473`). Differential guard already exists: `leapfrog_and_binary_joins_agree` (`seminaive.rs:7616`).
- **Hardware-Level Impact:** per-entry delta-list decode (`sp.get`) = serialized dependent loads with bit-unpack per candidate; per-row permutation probe = log₂(n) dependent cache misses × rows; zero word-parallelism despite the single-cycle AND primitive sitting unused.
- **Baseline Cost:** O(shorter-list) decodes + O(list) per-entry filter for `(?,p,o)`; O(rows × log n) for BGP stages.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. **Bitset-encode FoQ position lists** per hot `(p,o)` pair (or per predicate/object local id) as `u64` word arrays; `(?,p,o)` flips from "decode shorter list + per-entry `sp.get`" to `bitset_intersect_u64` over precomputed words. Encoding choice: on-disk format v2 **or** an eager auxiliary side-index (format version byte `TRIPLES_FORMAT_VERSION = 1` at `triples.rs:142`).
  2. **`QuadKey::chunk_matches` bitset lanes** (`dataset.rs:760`): carry dense-stage candidate sets as `u64` words so the 8-row `u8`-mask residual filter reduces to word ANDs for dense-subject stages.
  3. **Branchless seek**: replace `binary_search_range`/`local_lookup` (`triples.rs:750-782`) and datalog `Range`-run seek (`cursor.rs:460-473`) with the eytzinger/galloping cmov lower bound from OPT-03 (`lower_bound_eytzinger`, generic-key accessor).
  4. **Port the datalog leapfrog driver to SPARQL BGPs** as an alternative kernel for highly-connected patterns: `cycle.rs:282-330` + `ValueCursor::seek` semantics over the `QuadProbePlan` the loop already reuses (`bgp.rs:639-651`), replacing per-row permutation re-probes.
  5. **Guard**: keep `merge_intersect_rows` (`triples.rs:1536`) as the differential oracle — the same pattern that guards the shorter-list probe guards the bitset rewrite.
- **Conversion cost story (load-bearing):** on-demand bitset materialization pays only when **both sides are dense and reused across rows of the same join stage** (exactly the BGP inner pattern; nothing currently caches per-stage bitsets). Zero-re-encode win: `RankSelect`/`Bp`/`Bo` already store raw `u64` words (`bits.rs:190,419,715`) — a word-wise AND of two `RankSelect` word spans answers `(s,?,o)`-style meets with no re-encoding. Semantic precedent in-tree: `binding_pattern.rs:330` `meet` is literally `a.bound & b.bound`.
- **Hard Bounds Applied:** intersection width = 64 candidates per word (`bitset_intersect_u64`); leapfrog cursor arity ≤ `Frame` const N (4); delta-list decode bounded to the shorter side until side-index lands.
- **Proposed Type Layout & Signatures:** reuse `primitives::bitset_intersect_u64` verbatim (no duplicate body, per helpers-ledger); side-index shape: per `(p,o)` slot `{ words: Box<[u64]>, base: u32 }` built at partition seal; SPARQL leapfrog kernel: `fn leapfrog_stage(plan: &QuadProbePlan, cursors: &mut [QuadCursor<'_>], ...) -> Result<(), EvalError>` behind the existing plan-selection seam.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** hot intersect loop is word-AND + `trailing_zeros`; seeks compile to cmov descent (OPT-03 kernel); no per-entry branch chains in the `(?,p,o)` path.
- **Allocation Invariant:** 0 heap allocations per intersected word / per seek; side-index built once at seal (or format v2), never per query.
- **Benchmark Target:** `(?,p,o)` partition_rows ≥4× on the ≥32×-asymmetry class; BGP connected-pattern stages show multiway-intersection vs index-nested-loop parity-or-better; land in the OPT-20 bench suite with `--save-baseline`.
- **Layer Boundary:** kernels stay in `purrdf-core` / `purrdf-datalog` / `sparql-eval` per `layers.toml`; `check-layers.py` clean; differential oracle test green (bitset path == merge oracle == shorter-list probe on frozen corpora).
- **Implemented in `purrdf::branchless::{bitset, bitmap}`** (crate `crates/branchless`, ported from
  BranchlessCInRust): word-level set-logic (incl. ANDNOT/difference) and linear-counting
  bitmap primitives backing the word-parallel intersection.

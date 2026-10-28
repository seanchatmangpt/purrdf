# [OPT-42] Branchless Migration Handoff — Session Restart Continuation

> STATUS (2026-10-27): SUPERSEDED as the authoritative remaining-work
> list. Most of section 3 has landed via the OPT-43..68 ticket series
> (`docs/jira/v26.10.26/optimization/`); the five specialist agent files
> exist in `.claude/agents/`; region_is_zero landed (OPT-67,
> swar.rs:331). Read the optimization/ tickets for live scope. This file
> is a session-restart record, not a live queue.

## 1. TARGET CONTEXT
- **Crate & File:** `crates/branchless/` (18 modules), `docs/jira/v26.10.26/optimization/` (41 tickets + README)
- **Subsystem:** All
- **Inspection Agent:** Coordinator receipt, session 2026-10-26 (pre-restart)

## 2. STATE AT RESTART (verified facts)

Landed: `crates/branchless` (`purrdf-branchless`, no_std, forbid(unsafe), zero deps)
registered in workspace + facade (`purrdf::branchless`) + rdf-core (eytzinger
re-export switchover complete; rdf-core/src/eytzinger.rs is now a `pub use`).
layers.toml rows added (272 edges, check-layers.py exit 0). wasm32: crate +
purdhf-core --lib exit 0 (facade blocked upstream by PRE-EXISTING purrdf-n3
wasm break, unrelated). Lib tests ~142/143 passing at last per-module receipts;
integration tests 13/13. bcinr provenance pinned `1e92b213`, tree clean, 3/3
provenance spot-checks exact. OPT-01..41 written; 16 early duplicate-ID files
renumbered OPT-25..40; README index complete. Reuse/dedupe executed: char_class
re-exports swar kernels, bitmap→bitset adapters, packing→pdep delegation,
search→swar delegation, ct_min/max→select delegation, sort cswap consolidated
(sort_small::cswap single home; sort16 bitonic const-table canonical),
circuit_breaker::mask_of, helpers-ledger [[job]] rows for branchless families
(ledger lane completion NOT confirmed at restart — verify).
`~/bcinr` NEVER modified (read-only directive, verified clean).

## 3. REMAINING WORK (ordered)

1. **Final gate:** `cargo test -p purrdf-branchless --locked` (all targets),
   `cargo clippy -p purrdf-branchless --all-targets` — census showed ~341
   warnings (≈205 machine-fixable via `cargo clippy --fix`); gate is
   `-D warnings`, must reach zero.
2. **Helpers census:** `python3 scripts/check-shared-helpers.py` — branchless
   added ~14 FAILs (filter/hash re-derived splitmix/fnv/golden constants must
   route through purrdf_hash; swar.rs from_le_bytes sub-slices; isomorphic-body
   groups). **RESOLVED IN-TREE (2026-10-26):** the branchless FAIL classes are
   remediated in the source — `filter.rs`/`hash.rs` now route through
   `purrdf_hash::mix::GOLDEN_GAMMA` and `purrdf_hash::fnv::{BASIS, PRIME}`
   (filter.rs:17, hash.rs:240,252), `swar.rs` uses the `as_chunks::<8>` form
   (swar.rs:252,281,306,339,355), and the isomorphic-body groups carry their
   `[[distinct]]` rows in `helpers-ledger.toml`. Census BINARY RE-RUN still
   pending — compiles helper-census, UNVERIFIED on this (no-build) machine.
   Pre-existing 323 rdf-core FAILs are out of scope.
3. **Latent-risk HIGH fixes from the adversarial scan** (2 of 3 DONE inline:
   bitmap wasm32 `usize` hash truncation fixed; muldiv quotient-overflow
   contract documented). REMAINING: bloom `add` oracle is near-tautological
   (filter.rs:263 mirrors impl post-fix) — needs a third independent
   formulation oracle; `q16_log2_corrected` endpoint sweep vs f64 oracle
   (cost_math.rs:297-315, +16 bias line unverified); CMS hash-construction
   oracle from an equivalent-but-different formulation.
4. **Ticket amendments (scan-delivered, not yet applied):** OPT-02 (token
   validation via char_class/swar parse kernels), OPT-05 (wyhash_64 as DAG
   node hash-cons), OPT-09→OPT-01 (canon byte scans as OPT-01 consumers),
   OPT-13 (Visited64 ops spec'd as branchless::bitset calls), OPT-19
   (SmallVec var-set intersection → bitset::intersect_u64/is_subset_u64),
   OPT-21 (field_extract2/field_deposit2 straddling kernels as prerequisite),
   OPT-12 (primitives.rs cswap! adopts branchless cswap form verbatim),
   OPT-16 addendum (scaling.rs Q16 toolkit).
5. **New opportunity tickets OPT-43+ (findings in this session's scan lanes):
   purrdf top-10** (substitute.rs/solution.rs per-row position scans → lane-pack+tz;
   CSV portable-fallback SWAR; HNSW finite-check NaN lane mask; modifier.rs match
   towers; path_relation/datalog small-N sorts → sort8/sort16; mutable.rs graph
   scans; N3 lexer digit-run SWAR; embedding padding scan).
   **bcinr top-10 unported** (Morton/Hilbert 2D/3D; BitonicPriorityQueue8;
   dfa_advance; reduce.rs SWAR horizontal family; BMI blsi/blsmsk/blsr family;
   funnel shifts — the field_extract2 straddle prerequisite; SwarQuotientFilter
   container; LEB128; base64 chunk kernels — verify no mime home first).
6. **Follow-ups from OPT-41 (T-A..T-E):** case-fold policy bench call,
   const-generic width macro for select/clamp, straddling field kernels,
   bits.rs:530 altitude doc, bitset/bitmap select unification check.
7. **Eytzinger convention cross-check — RESOLVED (2026-10-26):** the
   layout-length convention (`len = n + 1`, i.e. usable slots + 1) is correct;
   all callers already pass n+1 — this item was stale (a misread of the padded
   `sizes` buffer as a slot-count argument). The real defect was the `len == 0`
   underflow in `build_subtree_sizes` (`crates/branchless/src/eytzinger.rs:98`,
   fixed with an `assert!(len >= 1)` boundary guard) plus the perfect-tree
   masking predicate (`n == 2^d - 1` masks the wrong-table symptom on some
   shapes), pinned by the `subtree_sizes_perfect_tree_masking` test
   (crates/branchless/src/eytzinger.rs:379). Convention documented in the
   module header (eytzinger.rs:74-76).
8. **CLAUDE.md agent-usage additions** (proposed in-session, awaiting approval:
   20-lane fan-out pattern + dispatch rules) and the 5 specialist agents in
   `.claude/agents/` (branchless-porter, repair-lane, reuse-auditor,
   verification-gate, ticket-writer).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA

- `make check` green for the new crate's slice (clippy -D warnings zero).
- Full branchless suite green (lib + integration), wasm32 --lib exit 0.
- check-layers.py exit 0; helpers census shows NO branchless-added FAILs.
- `~/bcinr` still `git status --porcelain` empty; provenance pin unchanged.

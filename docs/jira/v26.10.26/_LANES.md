# v26.10.26 branchless-completion wave — lane leases (2026-10-26)

One canonical checkout; lanes own ONLY the files listed. Coordinator owns git,
`helpers-ledger.toml`, `layers.toml`, all `Cargo.toml`/`Cargo.lock`, `.claude/`,
`CLAUDE.md`, `Makefile`, `.cargo/`. NO cargo/rustc anywhere; nothing >5s wall clock.

| Lane | Agent | Owned files |
|---|---|---|
| L1 | branchless-porter | crates/branchless/src/packing.rs |
| L2 | branchless-porter | crates/branchless/src/bitset.rs |
| L3 | branchless-porter | crates/branchless/src/swar.rs |
| L4 | branchless-porter | crates/branchless/src/sort16.rs |
| L5 | branchless-porter | crates/branchless/src/filter.rs |
| L6 | branchless-porter | crates/branchless/src/search.rs |
| L7 | branchless-porter | crates/branchless/src/eytzinger.rs |
| L8 | branchless-porter | crates/branchless/src/sort_small.rs |
| L9 | repair-lane | crates/sparql-eval/src/path_relation.rs |
| L10 | repair-lane | crates/sparql-eval/src/solution.rs |
| L11 | repair-lane | crates/datalog/src/seminaive/join.rs |
| L12 | repair-lane | crates/datalog/src/seminaive/factors.rs |
| L13 | repair-lane | crates/geo-kernel/src/cells.rs (+ cells/tests) |
| L14 | ticket-writer | docs/jira/v26.10.26/** status flips (not _LANES.md) |
| L15 | branchless-porter | crates/branchless/oracles/ref/{streams,bitset,swar}.py |
| L16 | branchless-porter | crates/branchless/oracles/ref/{eytzinger,sort_small,packing}.py |
| L17 | branchless-porter | crates/branchless/oracles/{conftest.py,test_kernels.py,pyproject.toml,gen/,vectors/} |
| L18 | branchless-porter | scripts/bounded, scripts/test-bounded-guard.sh |
| L19 | branchless-porter | scripts/target-hygiene.sh, scripts/lane-lease.sh |
| L20 | branchless-porter | .cargo/config.toml, Makefile (b/bt/lane/target-prune/ice recipes only) |

Shared seams: kernel signatures are pinned in each dispatch and in
`docs/jira/v26.10.26/optimization/OPT-4x*.md` — code against the pinned signature,
never against another lane's in-flight file. Ledger/layers/Cargo edits land via the
coordinator using the row texts in the dispatch prompts.

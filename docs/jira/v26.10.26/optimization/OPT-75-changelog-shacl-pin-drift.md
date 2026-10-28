# OPT-75: CHANGELOG SHACL allocation pins drifted from the enforceable test

**Status:** OPEN (filed 2026-10-27, wave-7 L16)

CHANGELOG.md Unreleased (~466-470) claims pins "40 / 78 / 48 / 101"
ungoverned and "58 / 98 / 66 / 126" governed. The enforceable contract
(`crates/shapes/tests/sparql_path_alloc.rs` CASES + ASK_FALLBACK
const) pins 40 / 77 / 48 / 79 ungoverned, 56 / 97 / 64 / 103 governed,
198 fallback. Four governed figures and sh:expression are stale.
The same stale figures sit in
`docs/design/purrdf-change-path-allocations.md:549-550` (its own :63
table shows the correct 40/77/48/79 - internally inconsistent).

Fix rides the next CHANGELOG regeneration: a commit message stating
the corrected pins (CHANGELOG is git-cliff-generated, never
hand-edited). The citation should also move from the report-only
`prebind_seed_fast_path` bench to the enforcing
`sparql_path_alloc.rs` test. Fix
purrdf-change-path-allocations.md:549-550 directly.

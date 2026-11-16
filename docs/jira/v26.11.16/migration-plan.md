# purrdf migration plan v26.11.16

Standing: UNVERIFIED

## Identity
- Path: /Users/sac/purrdf
- Remote: https://github.com/seanchatmangpt/purrdf (fork of Blackcat-Informatics/purrdf)
- Branch: main@0da05783 (tracks origin/main)
- Last commit: 2026-10-29 | commits: 759 | size: 474 MB
- Languages/build: Rust workspace (Cargo.toml, crates/ 247 MB, bindings/, clippy.toml,
  cliff.toml), Makefile, .githooks; vectors/ 29 MB, corpora/; CLAUDE.md present
- Nested repos: none

## Observed state
- Dirty (8): Cargo.lock, Cargo.toml, crates/process/Cargo.toml, crates/process/src/error.rs,
  crates/process/src/log/columnar.rs, crates/process/tests/process_courts.rs,
  dependency-ledger.toml, docs/RELEASE.md
- Untracked 0
- AHEAD 144 of origin/main / behind 0; stash 0; 3 unpushed local branches
  (graphlaw-carried, merge-upstream-main, merge-upstream-main-v2 — no upstream)
- Build dirs: target/ absent; crates/ 247 MB likely includes vendored/generated content
- Submodules: none

## What it is
PurRDF (Blackcat Informatics RDF library) carried as a never-publish fork with a
purrdf-process crate adding POWL 2.0, OCEL 2.0, HTN/FOND and conformance; consumes avec via
git rev.

## Classification
ACTIVE — census value stands; last commit 2.5 weeks ago.

## Migration risk
HIGH — 144 local commits on main are not on origin, plus 8 dirty process-crate files and 3
local-only branches. This is the largest unpushed commit backlog in the lane.

## Recommended action
COMMIT-PUSH
1. `cd /Users/sac/purrdf && git add -A && git commit -m "wip: process crate columnar OCEL"`
2. `git push origin main` — pushes the 144-commit backlog. DECISION: the last commit says
   "fork posture: never publish" — confirm pushing to the private seanchatmangpt fork is
   acceptable (it is not crates.io publishing).
3. `git push origin graphlaw-carried merge-upstream-main merge-upstream-main-v2`
4. `rm -rf target` if present before any copy (not needed for clone).
5. On new machine: `git clone https://github.com/seanchatmangpt/purrdf`.

## First commands on the new machine
- `cargo check` then `make check` (stated, not run)

## See Also
- ../v26.10.26/
- /Users/sac/docs/jira/v26.11.16/README.md

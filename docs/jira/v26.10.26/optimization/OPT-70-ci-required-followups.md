# OPT-70: CI-required follow-ups from the wave-2/3 audit (no-build machine)

**Status:** OPEN (filed 2026-10-27, from 20-lane audit waves)
**Why one ticket:** four mechanical items that all require CI or a build box;
none is safe to land by hand on this machine.

## 1. Cargo.lock is missing three workspace members

Members `purrdf` (crates/purrdf), `helper-census` (crates/helper-census),
`wasm-link` (crates/wasm-link) have no `[[package]]` entry in Cargo.lock.
All 49 locked first-party packages are 3.0.1; layering edges clean. Fix:
`cargo update --workspace --offline` on a build box (or let the next CI
`--locked` job fail and re-lock there). Until then `--locked` builds that
touch these members will refuse.

## 2. bounded-gates.yml re-implements the ladder inline

`.github/workflows/bounded-gates.yml` fb-ladder job duplicates
`scripts/fb-ladder.sh` with divergent semantics (W3 audit receipt):
- tier-1 gate list differs (CI adds check-serializer-rewinds.py; ladder
  omits it at fb-ladder.sh:73)
- fail-fast vs aggregate failure; per-gate vs aggregate receipts
- tier-3 probe identity differs (check-leaf-compile.sh vs leased
  `cargo check -p purrdf-branchless`)
- CI `RECEIPT tier=...` stdout lines violate the fb-receipt.sh grammar and
  append no JSONL; no ratchet/court; no SKIP fail-closed semantics
- `scripts/check-gate-parity.py` does not cover this pair.
Fix: single-source — workflow invokes `bash scripts/fb-ladder.sh` per tier
(needs a CI lane flag + a receipts-on-CI decision), reconcile the tier-1
list and tier-3 probe, extend check-gate-parity.py scope.

## 3. generated/ coverage gap

`make metadata`/check-generated.sh byte-verify only the two loss matrices +
entailment-rules doc. `generated/shapes-loss-ledger.json`,
`generated/queries/*.rq` (no in-repo generator at all), and the SSSOM
mapping are gated only by CI-only cargo tests (or nothing). Either fold
them into check-generated.sh or document them as CI-gated in CLAUDE.md's
generated-artifacts table.

## 4. bounded_build_guard blocks documented commands

`.claude/hooks/bounded_build_guard.py` refuses, per the CLAUDE.md-documented
surface: `bash scripts/lane-lease.sh ...` / `bash scripts/bounded-fmt.sh` /
`bash scripts/test-bounded-guard.sh` (bare script-file bash = uninspectable)
and `make b-<crate>` (not in MAKE_FAST_TARGETS, fail-closed). Either
allowlist those exact documented forms or update CLAUDE.md's command list.
Also: verification-gate/repair-lane agent bodies instruct cargo ladder steps
that the guard always refuses locally — reword to scripts/bounded forms or
mark CI-only.

## Verification invariant

Each item closes with a CI receipt (green `--locked` build incl. the three
members; check-gate-parity.py covering bounded-gates.yml; check-generated.sh
covering or documenting the three artifacts; hook self-test accepting the
documented command forms).

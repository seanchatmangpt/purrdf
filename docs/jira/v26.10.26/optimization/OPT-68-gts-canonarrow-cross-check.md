# [OPT-68] vectors/ Cross-Check Harness: CanonArrow Egress vs the GTS Goldens

> Renumbering note: requested as OPT-20; that ID is taken by
> `OPT-20-primitives-bench-suite.md` (Agent 20). Next free ID assigned per the
> README renumbering convention.

## 1. TARGET CONTEXT
- **Crate & File:** corpus `vectors/*.gts` (39 golden files, verified on disk
  2026-10-27 — 01-minimal … 33-multi-dict); consumer seam
  `crates/gts/src/mmr.rs:590-640` (`Proof::to_json`, an OPT-17 gap site);
  canonical engine seam `crates/rdf-core/src/ir/canon.rs`; existing
  frozen-corpus test pattern `crates/gts/tests/frozen_canonical_bytes.rs`;
  manifest `vectors/manifest.core.json`
- **Subsystem:** Attestation
- **Inspection Agent:** ticket-writer lane (2026-10-27), from the review lane
  that named the unused cross-check surface ("33 GTS nquads goldens" — see
  count note, §2)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** the GTS vectors are consumed today only through
  their frame-decode/canonicalize path (`frozen_canonical_bytes.rs`,
  `frozen_identities.rs`); **no test or harness anywhere feeds a golden's
  canonical N-Quads output through `ArrowEgressHeader` framing and compares
  the resulting envelope bytes against anything** — the canon-arrow egress
  surface has zero corpus coverage. The corpus is frozen
  (repo `CLAUDE.md` generated-artifacts table: "never regenerate or fix" —
  the wire format is governed upstream), so the harness is strictly
  read-only over `vectors/`.
- **Hardware-Level Impact:** none to fix — this is a coverage ticket; the cost
  is the *absent* invariant: nothing fails when canon-arrow egress drifts from
  byte-determinism on real canonical documents.
- **Baseline Cost:** 0 harnesses, 0 vector cells covered on this surface.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** one harness, one home — a new
  `crates/gts/tests/canon_arrow_cross_check.rs`, Chicago-style (real
  collaborators, assert final bytes, no mocks):
  1. Enumerate the corpus mechanically from `vectors/manifest.core.json` +
     the top-level `*.gts` files — **no hard-coded golden count**. The
     review lane's "33 nquads goldens" figure is recorded as the review's
     number, not an on-disk constant: the verified on-disk count of
     top-level `.gts` files is 39 (some are refusal/empty shapes with no
     nquads payload — 28-empty-file, 28b-non-header-item, etc.). The harness
     asserts its enumeration against the manifest, never against a literal
     33 or 39.
  2. For each golden: decode frames → canonical N-Quads (the existing
     frozen path) → produce the CanonArrow envelope: `header.to_bytes()` at
     offset 0, payload slabs straight from the canonical form, per the
     OPT-17 §3 writer spec — the harness is the *spec by example* for the
     OPT-17 writer, not a production egress route.
  3. Assert: magic/schema/flags/batch_count consistency rules (OPT-66's
     two compares), `fits_in` on the full envelope, reserved region zero
     (SWAR form per OPT-67), `total_byte_len == payload + 128 + 64`, and
     `triples_digest`/`columnar_digest` recomputed via streaming BLAKE3
     (the `sink::Blake3` drain per OPT-18-streaming-blake3-receipts — the
     existing OPT-18, not this ticket's old ID).
  4. Determinism law: run twice, bytes equal (no timestamps/env in the
     envelope — AGENTS.md:84, :344); golden-byte pattern per
     `crates/columnar/tests/golden_bytes.rs`.
  5. Over-refusal neighbor pairing: for every refusal asserted against a
     hostile header, the neighboring valid header shape still decodes `Ok`
     (canon empty `batch_count=1, payload=0` at minimum).
- **Hard Bounds Applied:** corpus frozen and read-only; the harness never
  writes under `vectors/`; envelope bounds per the fixed header law
  (128-byte header, 64-byte trailing padding).
- **Proposed Type Layout & Signatures:** none added to production code —
  the harness consumes `purrdf_arrow::ArrowEgressHeader` verbatim
  (`to_bytes`/`from_bytes`/`fits_in`, header.rs:71-189); if the OPT-17
  writer does not yet exist, the harness builds the envelope with
  `ArrowEgressHeader { .. }` struct literal + `to_bytes` over the canonical
  bytes as payload. This is the pre-wiring court for OPT-17, not a second
  writer home — the production writer remains single-homed in OPT-17's spec.
  Dependency direction: `crates/gts` may add a dev-dependency on
  `purrdf-arrow` (test-only dev-dep, layers.toml test edges stay clean —
  verify with `check-layers.py --fast` before landing).
  Wait: gts→arrow layer direction — verify against `layers.toml` at
  implementation time; if the edge is refused, the harness home moves to a
  `crates/arrow/tests/` target consuming the gts golden *bytes* as fixtures
  (byte fixtures, not a crate edge). The fallback decision is made at
  implementation time by running `check-layers.py` and recorded in the
  ticket's execution receipt — never assumed.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Allocation Invariant:** harness-only; no production alloc change.
- **Verification Invariant:** every enumerated golden produces an envelope
  that passes `from_bytes` + `fits_in` + digest re-check; a drift in the
  canon-arrow surface fails the court with the golden's name in the message.
- **Over-refusal neighbor:** hostile-header refusal corpus (bad magic,
  reserved-not-zero, batch_count-inconsistent per OPT-66) each paired with
  its valid neighbor still passing.
- **Frozen-corpus law constraint:** `vectors/` is never regenerated, never
  "fixed", never written by the harness; the GTS wire format is governed
  upstream. Any vector that cannot be framed (e.g. refusal shapes with no
  tabular payload) is a **loud typed skip** naming the file and reason —
  never a silent skip, never a faked pass.
- **Benchmark Target:** none (coverage ticket, no perf claim).
- **Layer Boundary:** `check-layers.py --fast` clean with the dev-dep edge
  resolved per §3; no ledger mutation (no new helper job).

# [OPT-66] ArrowEgressHeader batch_count Consistency Validation — Executed This Wave

> Renumbering note: requested as OPT-18; that ID is taken by
> `OPT-18-streaming-blake3-receipts.md` (Agent 18). Next free ID assigned per
> the README renumbering convention.

## 1. TARGET CONTEXT
- **Crate & File:** `crates/arrow/src/header.rs:104-111` (validation),
  `:98` (read), `:167-189` (error enum + `BadBatchCount`), `:234-255` (Default),
  tests `crates/arrow/tests/arrow_header_test.rs:196-262` (A-2 refusal + neighbor court)
- **Subsystem:** Egress
- **Inspection Agent:** ticket-writer lane (2026-10-27), absorbing the wave's
  batch-count review findings; format per OPT-17

## 2. MECHANICAL PATHOLOGY (pre-fix state, recorded for the receipt)
- **Current Code Construct:** before this wave, `from_bytes` read `batch_count`
  (`header.rs:98`) and never cross-checked it against `payload_byte_len` — the
  hostile-wire shapes `(batch_count = 0, payload > 0)` (a nonempty payload that
  belongs to no batch) and `(batch_count > 1, payload = 0)` (multi-batch framing
  over nothing) both decoded `Ok`. The commit that closes this ticket is
  `130fb2f0` "fix(arrow): refuse overflowing length headers; add fits_in guard +
  refusal matrix", which landed both the batch-count rules and the `fits_in`
  slice gate.
- **Hardware-Level Impact:** an internally inconsistent header survives decode
  and hands the egress layer a batch slab count that cannot partition the
  payload — downstream slicing either reads cross-batch garbage or panics on a
  range the header promised but the wire lacks.
- **Baseline Cost:** O(1) decode; the fix adds two integer compares, no loop, no
  allocation — cost is unchanged in the cache/mispredict sense.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT — STATUS: DONE (landed, ALIVE-by-test)
- **Architectural Proposal:** exactly the two consistency compares now present
  at `header.rs:106-111`:
  - `(batch_count == 0) && (payload_byte_len > 0)` → refuse `BadBatchCount(0)`
  - `(batch_count > 1) && (payload_byte_len == 0)` → refuse `BadBatchCount(n)`
  Refusal pairs with the valid-neighbor court required by the over-refusal law:
  `(batch_count = 1, payload = 0)` — the canon empty path (`Default`,
  header.rs:234-255, `batch_count: 1`, `payload_byte_len: 0`) — and
  `(batch_count = 4, large payload)` must both decode `Ok`. Both neighbors are
  pinned in `arrow_header_test.rs:234-262`
  (`header_accepts_consistent_batch_count_neighbours`), refusals pinned at
  `:201-232` (`header_refuses_inconsistent_batch_count`).
- **Hard Bounds Applied:** validation is two compares inside the existing
  fixed-offset decode, branch-predictable, zero-alloc; no new fields, no wire
  change (`SCHEMA_VERSION` stays `0x0002`).
- **Proposed Type Layout & Signatures:** unchanged —
  `from_bytes(&[u8; 128]) -> Result<Self, ArrowEgressHeaderError>`; new variant
  `BadBatchCount(u32)` at header.rs:178-179 with Display and
  `StructuredError` visits (`"canonarrow-bad-batch-count"`, header.rs:200-202,
  :225).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** refusal decision is pure integer compare; the A-2
  court (`arrow_header_test.rs:198-232`) asserts both refusal shapes
  byte-exactly via `from_bytes`.
- **Allocation Invariant:** decode remains allocation-free; the court also pins
  `to_bytes`/`from_bytes` round-trip identity.
- **Over-refusal neighbor:** executed and pinned
  (`arrow_header_test.rs:234-262`) — canon empty (1 batch, 0 payload) and
  4-batch large-payload headers still accepted.
- **Residual (future, not this wave):** a *per-batch minimum* rule
  (e.g. each declared batch's slab occupying ≥ some minimal byte footprint, or
  batch boundary offsets inside the payload region) is NOT implemented — the
  header carries no per-batch offset table, so any per-batch minimum is
  unenforceable at header-decode time and belongs to the OPT-67 slab-writer
  spec. Recorded as future work, not silently dropped.
- **Status line:** DONE — implemented in `crates/arrow/src/header.rs:104-111`,
  committed `130fb2f0`; test receipts on disk
  (`arrow_header_test.rs:196-262`); UNVERIFIED as to `cargo test` execution
  (no-build machine — CI gate owns the run).
- **Layer Boundary:** `check-layers.py` clean (no new edges; purrdf-arrow row
  unchanged).

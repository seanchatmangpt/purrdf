# [OPT-52] Purremb Zero-Padding Validation Scan to SWAR Word Scan

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/ir/embedding/view.rs:4772-4784`
  (`validate_zero_padding_usize`); ~13 call sites (:2483, :2549, :2702,
  :2722, :2736, :2831, :2867, :2915, :2958, :3033)
- **Subsystem:** Attestation
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5, second half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  ```rust
  // view.rs:4778
  if let Some(relative) = padding.iter().position(|byte| *byte != 0) {
      return Err(EmbeddingError::InvalidPadding {
          offset: u64::try_from(start + relative).unwrap_or(u64::MAX),
      });
  }
  ```
- **Hardware-Level Impact:** byte-at-a-time `position` over the padding
  span: one load + one compare + one data-dependent branch per byte,
  loop-carried dependence serializes; alignment padding spans (8/16/32
  bytes typical, per metadata.rs:2422 alignment math) pay this per byte
  at every section boundary of every purremb open. The canonical form is
  word-at-a-time: Mycroft zero-byte test per u64 (one XOR + one
  multiply/subtract fold), ~1 branch per 8 bytes, fully pipelined.
- **Baseline Cost:** UNVERIFIED — no purremb open-path bench isolating
  validation; add to the OPT-20 bench suite before any speedup claim.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** In `validate_zero_padding_usize`, scan the
  span in `u64` chunks (unaligned-safe via `as_chunks::<8>` after
  front-aligning the head, or byte head/tail with word body):
  1. For each whole word, `word != 0` fast path; if zero, continue.
  2. If nonzero, `purrdf_branchless::swar::find_byte_in_word(word, 0)`
     inverted — the zero-detect: the Mycroft construction
     `(word.wrapping_sub(0x0101..).!.. & 0x8080..)` is already the
     sanctioned form; recover the byte offset with trailing-zeros over
     the mask and report the exact `InvalidPadding { offset }` as today.
  3. Byte loop for the <8-byte head/tail only.
  Error type, offset arithmetic (`start + relative`), and refusal
  semantics byte-identical.
  - Status: kernels **Implemented in `purrdf_branchless::swar`
    (`find_byte_in_word(word, 0)` / `has_byte_in_word(word, 0)` are the
    zero-byte forms; ledger row `branchless-swar-word-kernels`,
    helpers-ledger.toml:3869)** — landed; this ticket is the wiring.
    If a dedicated `first_zero_byte_word` kernel is wanted for
    readability, it is ADDED to swar.rs as the one home — never a
    local copy in rdf-core.
- **Hard Bounds Applied:** fixed 8-byte word bound; head/tail byte loops
  bounded by 7+7 bytes; zero allocation (slice scan only); exact
  first-offender offset preserved.
- **Proposed Type Layout & Signatures:**
  ```rust
  fn validate_zero_padding_usize(
      bytes: &[u8], start: usize, end: usize,
  ) -> Result<(), EmbeddingError> {
      // identical signature; word body + byte head/tail internally
  }
  ```
  Single home: the one function every call site already uses — no new
  call-site changes at all.
- **Reuse-First:** extend `purrdf_branchless::swar`
  (helpers-ledger.toml:3869); `check_shared_helpers.py` /
  helper-census must show no second zero-scan body appearing in
  rdf-core.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** differential court vs the current byte loop
  over randomized spans: first-nonzero offset identical for all of
  {all-zero, nonzero at 0, at k<8, at 8, at last byte, multiple
  nonzeros} — the offset in `InvalidPadding` must match byte-for-byte.
- **Valid-neighbor (refusal mirror):** the refusal case (nonzero
  padding byte refused, profile.rs:592
  `non_zero_padding_is_refused`) stays green AND the paired valid
  neighbor (profile.rs:602 `zeroed_padding_passes_the_block_check`,
  same block with padding zeroed) must keep succeeding — both run in
  the same court; tightening nothing here, but the SWAR rewrite must
  not silently widen acceptance (e.g. reading past `end`).
- **Allocation Invariant:** zero allocation (slice scan; no Vec
  introduced).
- **Benchmark Target:** purremb open/validation bench in
  `purrdf_testkit::bench`, report-only, before/after.
- **Layer Boundary:** rdf-core already depends on branchless
  (eytzinger re-export precedent); `scripts/check-layers.py` +
  `make check` green.

> STATUS CORRECTION (2026-10-27, W2 audit): the wiring LANDED —
> validate_zero_padding_usize at
> crates/rdf-core/src/ir/embedding/view.rs:4772 (SWAR body ~:4790-4800) is
> converted, but as a LOCAL SWAR expression, diverging from the one-home
> rule the ticket itself declares (self-documented 2x divergence awaiting
> coordinator). Ticket's "wiring is the open work" status is stale.

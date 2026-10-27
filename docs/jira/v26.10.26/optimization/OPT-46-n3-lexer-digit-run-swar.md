# [OPT-46] N3 Lexer Digit Runs to SWAR Run Scan

> Merged ticket: folds in OPT-51 (duplicate inspection of the same
> `read_number` digit loops, v26.10.26 wave). OPT-51 is
> merged-superseded by this file; see
> `OPT-51-n3-lexer-digit-run-swar.md` for the pointer stub.

## 1. TARGET CONTEXT
- **Crate & File:** `crates/n3/src/lexer.rs:423-474` (`read_number`;
  scalar digit loops at :429-431 (integer), :441-443 (fraction),
  :452-455 (exponent, with `exponent_digits` counter));
  sibling scalar digit takes in `crates/n3/src/reasoner.rs:7751`
  (regex `\d` take_while) and :7856-7872 (format-spec width/precision
  loops) — same pathology class, cited for the follow-up home, in
  scope only if the census routes them here.
- **Subsystem:** Ingestion
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5;
  merged from both halves of the §3.5 split)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  ```rust
  while self.peek().is_some_and(|c| c.is_ascii_digit()) {
      value.push(self.bump().unwrap());
  }
  ```
  (lexer.rs:429-431; structurally identical loops at :441-443 and
  :452-455, the exponent loop additionally incrementing
  `exponent_digits`).
- **Hardware-Level Impact:** per digit: two Option-wrapped cursor
  calls (`peek` + `bump`), an `is_ascii_digit` range test, and a
  `String::push` with a per-byte capacity check. A k-digit literal
  costs ~5k dependent branches; the loop-carried dependence
  serializes and the predictor sees a data-dependent exit exactly
  once per number. No ILP across characters, no ILP across words.
- **Baseline Cost:** UNVERIFIED — `crates/n3/benches/reasoner.rs`
  has a `parse` bench (flat document of simple triples), but no
  numeric-literal-heavy lexer bench exists; add one to the
  `purrdf_testkit::bench` suites before landing.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** when the cursor's remaining input is
  contiguous in memory (the lexer's `input` byte slice), consume the
  digit run in 8-byte words. Per word:
  1. `purrdf_branchless::swar::byte_lane_in_range(word, b'0', b'9')`
     gives the digit mask in one fold (swar.rs:28 — landed).
  2. Zero-fill the out-of-bounds lanes of the tail word (cascade-safe
     per the Mycroft construction), then take the run length via
     `trailing_zeros` on the lane mask.
  3. Bulk-extend `value` from the run bytes in one
     `extend_from_slice`-class call (single growth event, not
     per-char), then reposition the cursor past the run.
  The existing scalar loop remains as the fallback for runs
  straddling a buffer boundary, non-contiguous cursor state, and the
  final partial word. Token text, offsets, and every error site stay
  byte-identical because the same characters are consumed in the same
  order.
- **Preserved refusal sites (load-bearing):** exponent zero-digit
  refusal `lexer.rs:456-465` (audit wave-2 F9: keeps `1e`/`1e+` from
  minting `"1e"^^xsd:double`) and the `55.` malformed-numeric
  refusal `lexer.rs:467-469` — the counter becomes the total run
  length, both checks run unchanged after the fast path.
- **Hard Bounds Applied:** fixed 8-byte word steps; one bounds check
  per word, not per char; zero new allocations — `value`'s existing
  `String` is reserved once up front for the run length and extended
  at most once per word.
- **Proposed Type Layout & Signatures:**
  ```rust
  impl Lexer {
      /// Consume [0-9]{1,8k} via SWAR; returns digits consumed.
      fn take_digit_run(&mut self, value: &mut String) -> usize { ... }
  }
  ```
  All three digit loops (:429, :441, :452) route through this one
  helper — the exponent loop feeds its `exponent_digits` counter from
  the returned run length. One job, one home.
- **Reuse-First:** single home is `purrdf_branchless::swar`
  (`byte_lane_in_range` swar.rs:28, `find_byte_in_word` swar.rs:51,
  `count_leading_eq_u8` swar.rs:279; ledger row
  `branchless-swar-word-kernels`, helpers-ledger.toml:3869).
  Per CLAUDE.md the lexical ring-fence stands — scanners call
  `purrdf_lex::terminals` and must not retype a range table; this
  ticket touches the n3 crate's own cursor, not `purrdf-lex` tables.
  If the census prefers the `purrdf-lex` altitude for a shared
  digit-run helper, route through `purrdf-lex` instead — decide with
  `python3 scripts/check-layers.py --home-for n3 lex branchless`
  before landing. Note the current `layers.toml` `purrdf-n3` row
  (layers.toml:178-181) has deps `["purrdf-hash", "purrdf-iri",
  "purrdf-lex", "purrdf-rdf"]` — n3→branchless is a NEW layer edge
  requiring a `layers.toml` row in the landing diff.
- **Status:** NOT yet implemented — the kernels
  `byte_lane_in_range` / `find_byte_in_word` / `count_leading_eq_u8`
  are Implemented in `purrdf_branchless::swar` (ledger row
  `branchless-swar-word-kernels`); the cursor-slice digit-run helper
  and the lexer wiring are new n3-side code, still unwritten
  (`lexer.rs:429-455` remain scalar loops, verified on disk
  2026-10-26).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** token-for-token identical lexer output
  vs. the current scalar lexer on a differential corpus: integers,
  decimals (`.5`, `4.e2`, `55.` statement-dot disambiguation),
  doubles (`1e2`, `1e+2`, `1E-3`), malformed (`1e`, `1e+`, `5.`
  before a statement dot), runs of length 7/8/9/16/17 (word-boundary
  crossings), and sign-prefixed values.
- **Valid-neighbor (refusal mirror):** the F9 refusals at
  lexer.rs:456-469 are load-bearing — the SWAR fast path must NOT
  swallow them. Required valid-neighbor set: `1e2`, `1e+2`, `1E-3`,
  `.5`, `4.e2`, `-0.0` must still lex successfully (fast path
  active); `1e`, `1e+`, and `55.` followed by a statement boundary
  must still be refused/segmented exactly as today. Both sides run in
  the same court.
- **Allocation Invariant:** one `reserve` of the exact run length per
  digit run; no per-character String growth on the SWAR path
  (alloc-probe over a fixed literal corpus).
- **Benchmark Target:** numeric-literal-heavy N3 lexer bench added to
  the `purrdf_testkit::bench` suites; report-only.
- **Layer Boundary:** `python3 scripts/check-layers.py` exit 0 with
  the resolved n3→branchless (or n3→lex→branchless) edge;
  `make check` green; `make conformance` N3 scoreboard unchanged
  (zero token deltas).

# [OPT-43] CSV Portable Stop-Scan Kernel to SWAR Lane Mask

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf-core/src/csv/scan.rs:130-158`
  (`find_portable`), tail helper at `scan.rs:111-115` (`find_in_tail`)
- **Subsystem:** Ingestion
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5,
  first half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** the portable kernel builds a 16-lane hit
  vector with a nested per-byte × per-needle loop, then OR-folds and
  `.position()`s it:
  ```rust
  let mut lanes = [0_u8; CHUNK];
  for (lane, &b) in lanes.iter_mut().zip(chunk) {
      let mut hit = false;
      for &needle in needles {
          hit |= b == needle;
  ```
  and `if lanes.iter().fold(0, |any, &lane| any | lane) != 0 { return
  lanes.iter().position(|&lane| lane != 0)` (scan.rs:140-153).
- **Hardware-Level Impact:** the inner needle loop is a data-dependent
  branch per (byte, needle) pair — up to 16×16 = 256 compare+branch pairs
  per 16-byte chunk (StopSet carries up to LANES needles). The `bool`
  lanes array defeats vectorization: LLVM cannot prove the loop trip
  count independent of data, so each chunk executes as a scalar ladder of
  compares; the OR-fold plus `.position()` then re-scans the lane array
  twice more. A SWAR form is one XOR/EOR mask build + `trailing_zeros`
  per chunk.
- **Baseline Cost:** UNVERIFIED — `crates/rdf-core/benches/csv.rs` exists
  but no stop-scan kernel bench rung is cited in it; extend it before
  landing (AGENTS.md: perf changes need a bench).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** load the chunk as `u64::from_le_bytes`
  (two words), build the per-needle zero-mask with the landed
  `purrdf_branchless::swar::find_byte_mask_word` (find_byte_mask_word
  form: broadcast needle, XOR, subtract-1 trick), OR-accumulate per
  needle into one `u64` per word, `!= 0` = clean-chunk test,
  `trailing_zeros()` = first-hit offset. Same chunk/tail split, same
  byte-exact offset answer. The kernel stays `#[inline(never)]` and
  remains the differential oracle for every explicit (SIMD) backend
  kernel — its formulation must keep agreeing with itself.
- **Hard Bounds Applied:** fixed CHUNK=16, LANES-bound needle count; two
  u64 loads per chunk, no allocation, no data-dependent branches inside
  the chunk loop (one exit branch per chunk, unavoidable and unchanged).
- **Proposed Type Layout & Signatures:**
  ```rust
  fn find_portable(set: &StopSet, haystack: &[u8]) -> Option<usize> {
      // per 16B chunk: w0, w1 = u64 loads;
      // m0 = needles.map(|n| swar::find_byte_mask_word(w0, n)).or_fold();
      // hit = (m0 | m1) != 0; first = (m0 | m1).trailing_zeros()
  }
  ```
  Signature unchanged (`pub(crate) fn find_portable(&StopSet, &[u8]) ->
  Option<usize>`); all callers via `arch.rs:75`/`backend.rs` untouched.
- **Reuse-First:** single home is `purrdf_branchless::swar`
  (`find_byte_mask_word`, scan.rs already re-derives nothing new); the
  csv module keeps the one stop-scan home per helpers-ledger
  `byte-search`/delimiter-scan rows. No second SWAR body may appear
  inside `crates/rdf-core/src/csv/`.
- **Status:** kernels **Implemented in
  `purrdf_branchless::swar::find_byte_mask_word`** (landed, no_std,
  zero-dep); this ticket is the rdf-core wiring. Note: rdf-core already
  depends on purrdf-branchless (crates/rdf-core/Cargo.toml:35).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** differential — new portable output equals
  old portable output byte-for-byte on: every StopSet width 0..=LANES,
  hits at every chunk offset, tails 0..15, and the existing wasm32
  differential corpus (backend.rs adapters keep passing unchanged).
- **Allocation Invariant:** zero allocations (kernel is
  allocation-free today; keep it so).
- **Benchmark Target:** add a `find_portable` stop-scan bench rung to
  `crates/rdf-core/benches/csv.rs` (before/after, report-only).
- **Layer Boundary:** no new layer edge (rdf-core→branchless exists);
  `python3 scripts/check-layers.py` exit 0; `make check` green.
- **Valid-neighbor (refusal mirror):** no refusals added — the kernel
  only relocates a hit detection. Required neighbor test: StopSets of
  width exactly LANES and needles 0x80/0xFF still resolve first-hit
  offsets identically (the lanes/u8 overflow corner the current bool
  form handles).

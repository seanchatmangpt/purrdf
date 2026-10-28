# [OPT-67] Branchless Kernel Candidates for columnar/arrow — Run Codec, Validity Runs, SWAR Reserved Check

> Renumbering note: requested as OPT-19; that ID is taken by
> `OPT-19-static-ast-admission-gate.md` (Agent 19). Next free ID assigned per
> the README renumbering convention.

## 1. TARGET CONTEXT
- **Crate & File:** hooks verified in `crates/arrow/src/header.rs:174-176 (re-pinned 2026-10-27; was 138-140 pre-edit)`
  (reserved-region byte loop); `crates/branchless/src/{packing,bitset,swar}.rs`
  (extension homes); LEB128 home per OPT-57
  (`crates/rdf-core/src/ir/pack/bits.rs:1052` `write_varint`, `:1066` region
  `read_varint`, consumed by `crates/rdf-core/src/artifact/identity.rs:66,323,437`)
- **Subsystem:** Egress
- **Inspection Agent:** ticket-writer lane (2026-10-27), from the bcinr
  unported list (OPT-42 §3.5, pin `1e92b213`) + the columnar/arrow review lane

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  1. **Reserved-region check is a byte loop.** `header.rs:174-176 (re-pinned 2026-10-27; was 138-140 pre-edit)`:
     `if bytes[100..128].iter().any(|&b| b != 0) { return
     Err(...ReservedNotZero); }` — 24 scalar loads with a data-dependent
     early-exit branch.
  2. **No run codec exists** for the CanonArrow slab writer (OPT-17 §3
     names the slab layout; no producer yet) — batch runs of identical
     dictionary ids would emit one value per slot.
  3. **No multiword validity-run scanner** — columnar validity bitmaps
     (word-per-64-rows) have no branchless run-length measurement across word
     boundaries anywhere in the workspace; `branchless::bitset` has set logic
     (`intersect/union/difference_u64_slices`) but no run measurement.
- **Hardware-Level Impact:** the byte loop mispredicts on the common all-zero
  case's early exit and spends 24 dependent loads for a check on bytes already
  resident in the same two cache lines as `columnar_digest`; the missing run
  codec and validity-run scanner force scalar per-element egress loops when the
  slab writer lands.
- **Baseline Cost:** reserved check: 24 iterations, branchy; run codec and
  scanner: absent (no baseline — bench precondition per AGENTS.md).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  1. **SWAR reserved-region check** (extend the landed `branchless::swar`
     horizontal family — `horizontal_and_u32` at `swar.rs:632` et al., per the
     README OPT-55 mapping): load `bytes[100..108]` and `bytes[108..116]` and
     `bytes[116..128]` as `u64`/`u64`/`u128` (unaligned little-endian loads via
     `u64::from_le_bytes`, zero-copy), OR-accumulate, single compare against 0.
     24 scalar loads + 24-iteration loop → 4 loads + 4 ORs + 1 compare,
     branchless, zero-alloc. Constant-time on all wire images.
  2. **varint/LEB128 run codec** — ROUTE, DO NOT PORT (OPT-57 law): the
     varint atom is `purrdf_rdf_core::ir::pack::bits::{write_varint,
     read_varint}`; the *new calculus* is the run framing
     `(varint(run_len), value)` around it. Home: extend the existing
     `branchless::packing` job's `entry_points` (helpers-ledger one-home-per-job;
     forbidden: a second varint body anywhere, bcinr's `leb128_{encode,decode}_u64`
     (`~/bcinr/crates/bcinr-logic/src/algorithms/leb128_encode_u64.rs:29`) are
     differential oracles only). Fixed-bound: run length bounded by the batch
     row count (≤ u32), value width ≤ 8 bytes; encode writes at most
     5 + 8 bytes per run.
  3. **Multiword validity-run scanner** (extend `branchless::bitset` — BMI trio
     + funnel shifts landed at `bitset.rs:380,398` per README OPT-54): given a
     validity bitmap as `&[u64]`, compute the length of the leading/maximal
     run of set (or clear) words using funnel-shift straddling across word
     boundaries — the straddling form is exactly the `field_extract2`/
     `field_deposit2` u128-window shape already landed at
     `packing.rs:253-289` (OPT-63 T-C). Fixed-bound: O(words), no allocation,
     early-exit-free (return the run length; caller branches once).
- **Hard Bounds Applied:** all three kernels: zero-alloc, fixed iteration
  bounds (4 loads / O(words) / 5+8 B per run), `forbid(unsafe_code)` per the
  branchless crate contract, no_std clean, wasm32 clean.
- **Proposed Type Layout & Signatures:**
  - `branchless::swar`:
    `pub const fn region_is_zero(bytes: &[u8]) -> bool` (slice form — the
  call site holds a `&[u8]` wire buffer; the const-generic form buys
  nothing for the single N=24 instantiation)
    — the minimal form the header site needs (`N = 24` at the call site;
    kernel folds the slice through u64 windows + tail mask, one compare).
  - `branchless::packing`: `pub fn encode_run(out: &mut impl Extend<u8>,
    run_len: u32, value: &[u8]) -> usize` and the decode twin, both routing
    through `purrdf_rdf_core::ir::pack::bits::{write_varint, read_varint}`.
    If routing creates a branchless→rdf-core layer edge (branchless is the
    zero-dep root-adjacent crate), the varint atom is inlined behind the
    kernel signature and proven byte-identical to the rdf-core home by
    differential test against it — the home-per-job law is satisfied by
    proven-identical delegation, not a sibling body. Layer row
    branchless→rdf-core is checked against `layers.toml` before adoption.
  - `branchless::bitset`: `pub fn validity_run_words(words: &[u64],
    of_set: bool) -> usize` — leading run length in bits across word
    boundaries via funnel-shift straddle.
- **Status lines:** `region_is_zero` and `validity_run_words` — landless,
  spec'd here. `branchless::packing` straddling kernels (prerequisite 3) are
  landed: `Implemented in purrdf::branchless::packing` (packing.rs:253-289).
  LEB128 home is landed and wired (rdf-core pack/bits per OPT-57 status note).

  u128-accumulator wasm caveat: u128 is available on wasm32 but
  emulated (compiler-rt); the kernel must treat any tail as two u64
  windows plus a u32 tail load OR-accumulated into the same u64
  accumulator for portable codegen. Correctness is unaffected — perf
  note only (branchless is not in the `make wasm` allowlist today, so
  the gate is unaffected). Landed: swar.rs:325 (slice form, const fn,
  u128 tail-shifted accumulator) + oracle suite; unwired into
  arrow/header.rs by layer decision (arrow stays purrdf-hash-only).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `region_is_zero` constant-time, no early exit;
  `validity_run_words` returns the run length, single caller-side branch;
  run-codec encode/decode round-trip identity over the fixed-bound corpus.
- **Over-refusal / valid-neighbor pairing:** the reserved-region tightening
  is already refusal-shaped (`ReservedNotZero`, header.rs:174-176 (re-pinned 2026-10-27; was 138-140 pre-edit)) — its
  neighbor court is the existing `Default` header (`reserved: [0u8; 24]`,
  header.rs:187) round-tripping `Ok` (pinned at
  `arrow_header_test.rs:189-196` region); any new refusal added by these
  kernels carries its paired valid-neighbor test in the same file.
- **Chicago oracles (crates/branchless/oracles/, Python over shared JSON
  vectors, gen/ref pattern):**
  - `test_swar.py`: `region_is_zero` vs a `bytes == b"\\x00" * N` oracle
    vector family; adversarial cases: nonzero in each of the 24 byte
    positions × single-bit patterns, plus the all-ones word.
  - `test_packing.py`: run codec vs a Python
    `struct`-based varint reference (bcinr `leb128_encode_u64` ported to the
    vector generator as the third formulation); cases: run_len 1, 127/128
    boundary, 2^21 boundary, empty value forbidden (typed refusal) with its
    valid neighbor (run_len ≥ 1, 1-byte value).
  - `test_bitset.py`: `validity_run_words` vs a bit-string reference over
    random + adversarial masks (all-set, all-clear, 1-bit runs at word
    boundaries, MSB/LSB straddles).
- **OPT-42 §4 gate requirements (all mandatory before "landed"):**
  `cargo test -p purrdf-branchless --locked` all targets green;
  `cargo clippy -p purrdf-branchless --all-targets` at `-D warnings` zero;
  wasm32 `--lib` exit 0; `check-layers.py` exit 0 (any new branchless edge
  checked); helpers census shows NO branchless-added FAILs; `~/bcinr` still
  `git status --porcelain` empty, pin unchanged `1e92b213`.
- **Benchmark Target:** extend the purrdf_testkit::bench suite (per AGENTS.md:
  a perf change needs a bench): reserved-region check vs the byte loop;
  validity-run scanner vs scalar bit-walk; run codec vs per-slot emission.
  No speedup asserted anywhere without its bench row.
- **Layer Boundary:** `check-layers.py` clean; branchless stays dep-free —
  the varint routing decision (kernel-side inline + differential proof vs
  layers.toml edge) is resolved at implementation time and recorded in the
  helpers-ledger row.

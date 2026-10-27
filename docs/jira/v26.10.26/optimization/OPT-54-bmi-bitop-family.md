# [OPT-54] Port BMI blsi/blsmsk/blsr and Funnel-Shift Micro-Kernels

## 1. TARGET CONTEXT
- **Crate & File:** `crates/branchless/src/bitset.rs` (iteration sites) and
  `crates/branchless/src/select.rs` (select path) as replacement sites; kernels land
  as `purrdf_branchless::bitset::{isolate_lowest, clear_lowest, mask_to_lowest}`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** OPT-42 ticket-writer lane (bcinr handoff §3.5)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** bcinr implements the BMI1 trio as portable
  single-expression kernels — `blsi_u64` = `val & val.wrapping_neg()`,
  `blsmsk_u64` = `val ^ val.wrapping_neg()`, `blsr_u64` = `val & (val - 1)`
  (`~/bcinr/crates/bcinr-logic/src/algorithms/blsi_u64.rs:24`,
  `blsmsk_u64.rs:24`, `blsr_u64.rs:24`). PurRDF bitset iteration re-derives these
  forms ad hoc at each loop site instead of calling one named kernel.
- **Hardware-Level Impact:** inline `x & -x` is 1 instr when LLVM folds it, but the
  unnamed form is re-derived per site and drifts; a funnel shift written as
  `(a >> s) | (b << (64-s))` carries an extra branch for `s == 0` that
  `u64::rotate`/`shr`-with-mask forms avoid (shifting by 64 is UB — the naive form
  must branch or mask; bcinr masks:
  `funnel_shift_right_u64.rs:22-25`).
- **Baseline Cost:** 0 measured — the ticket's bench is the measurement. The cost
  being removed is per-site drift and the s==0 guard branch on the naive funnel
  form.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Land the trio + two funnel shifts as `const fn`
  portable kernels in `purrdf_branchless::bitset` (trio: `isolate_lowest`,
  `clear_lowest`, `mask_to_lowest`) and `purrdf_branchless::packing` (funnel:
  `funnel_shr`, `funnel_shl`), then sweep bitset iteration sites
  (`crates/rdf-core/src/ir/pack/bits.rs` select/rank loops,
  `crates/branchless/src/bitset.rs` slice loops) to call them. No
  `target_feature` runtime dispatch — the portable forms compile to the BMI
  instructions on x86_64 with BMI1 target; the WASM build uses the same portable
  form.
- **Hard Bounds Applied:** zero-alloc, `const fn`, no runtime feature detection
  (single portable body, no vtable hop, no duplicate per-target bodies).
- **Proposed Type Layout & Signatures:**
  - `pub const fn isolate_lowest(x: u64) -> u64`
  - `pub const fn clear_lowest(x: u64) -> u64`
  - `pub const fn mask_to_lowest(x: u64) -> u64`
  - `pub const fn funnel_shr(hi: u64, lo: u64, s: u32) -> u64` (s <= 64, masked)
  - `pub const fn funnel_shl(hi: u64, lo: u64, s: u32) -> u64`

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** the funnel kernels compile with no branch on any `s`
  (objdump on x86_64: exactly one `shrd/shlx/or` group); the trio compile to the
  expected 1-2 instruction bodies.
- **Reference Invariant:** for all `u64` values in a quickcheck sweep against the
  bcinr reference implementations (blsi/blsmsk/blsr: reference fns at
  `blsi_u64.rs:35`, `blsmsk_u64.rs:38`, `blsr_u64.rs:35`) — bit-exact.
- **Over-refusal neighbor:** the existing `select_bit_u64`/`rank_u64` courts
  (`crates/branchless/src/bitset.rs:23-29` doc contract) must stay green — if the
  sweep changes any select/rank result on a valid bitset, the gate fails (the
  valid-neighbor requirement, refusal-class mirror).
- **Benchmark Target:** bitset iteration bench in `purrdf_testkit::bench`; extend
  the existing primitives bench suite (OPT-20/OPT-39 lineage).
- **Layer Boundary:** `check-layers.py` clean; ledger job `branchless-per-word-rank-select`
  extended (not duplicated) in `helpers-ledger.toml` — one home per job, no second
  `x & -x` body anywhere in the workspace (`check-shared-helpers.py` clean).

Status note: LANDED, UNVERIFIED (no-build machine). bcinr kernels were
landless-but-written upstream (`bcinr_logic::algorithms::{blsi,blsmsk,blsr}_u64`,
`funnel_shift_{left,right}_u64`); PurRDF now carries them. **Recorded deviation:**
the two funnel shifts landed in `crates/branchless/src/bitset.rs`
(`funnel_shift_right_u64` :380, `funnel_shift_left_u64` :398) — not in
`packing.rs` as this ticket's section 3 proposed. bitset is the iteration-site
home and already owned the per-word bit kernels, so the shifts joined their
consumers; `packing.rs` keeps the field/deposit and Morton family. One home per
job preserved — no second funnel body exists (checked 2026-10-26).

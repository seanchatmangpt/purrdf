# [OPT-53] Port Morton/Hilbert 2D/3D Space-Filling Key Kernels

## 1. TARGET CONTEXT
- **Crate & File:** `crates/geo-kernel/src/cells.rs:427-448` (`hilbert_path`) as the
  replacement site; kernels land in `crates/branchless/src/packing.rs`
  (Morton) and stay private-adjacent for `hilbert_path`
- **Subsystem:** Relational Sieve
- **Inspection Agent:** OPT-42 ticket-writer lane (bcinr handoff §3.5)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** `hilbert_path` is a `while size > 1` loop with a
  4-way `if/else` chain, tuple swap (`core::mem::swap`), and `path = (path << 2) | digit`
  serial dependent chain — one mispredicting quartet dispatch per level (30 levels
  at MAX_LEVEL, `crates/geo-kernel/src/cells.rs:332`).
- **Hardware-Level Impact:** ~30 iterations x 2 predicted branches + a data-dependent
  swap; per-call latency variance by curve digit; L1-resident but serially dependent.
- **Baseline Cost:** ~120-160 cycles per encode at level 30 (unmeasured — the bench
  in section 4 is the measurement, not this estimate).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Port bcinr's fixed-iteration Morton macros and the
  compact-then-compaction Hilbert decode as fixed-bound kernels into
  `purrdf_branchless::packing` (existing per-word bit-surgery home,
  `crates/branchless/src/packing.rs` — already carries the portable pext/pdep
  forms `compress_bits:207` / `deposit_bits:242`). Morton spread/compact MUST be
  composed over those existing kernels where the masks are static — no second
  pext/pdep body. Hilbert = bcinr's
  fixed-16-level unrolled encode with entry-rotation pre-transform
  (`~/bcinr/crates/bcinr-logic/src/algorithms/hilbert_curve_encode_u32.rs:27`),
  then `hilbert_path` becomes a thin adapter over it.
- **Hard Bounds Applied:** fixed 30-level bound, zero allocations, no loop-carried
  branches; all dispatches via mask-select (`0.wrapping_sub(cond)`), reuse-first via
  the `branchless-mask-minmax-select` ledger job's select idiom.
- **Proposed Type Layout & Signatures:**
  - `pub const fn morton_spread_2d(x: u32) -> u64`
  - `pub const fn morton_compact_2d(m: u64) -> u32`
  - `pub const fn morton_spread_3d(x: u32) -> u64` (into u64 holds 21 bits/axis; 3D
    encode takes `(u32, u32, u32) -> u64` via 21-bit spread)
  - `pub fn hilbert_encode_2d(x: u32, y: u32, level: u8) -> u64`
  - `pub fn hilbert_decode_2d(d: u64, level: u8) -> (u32, u32)`
  - `hilbert_path(x, y, level)` stays the geo-kernel API, re-expressed as a call
    into the `purrdf_branchless::packing` kernel.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `hilbert_encode_2d`/`morton_spread_*` contain zero
  data-dependent forward branches (object-dump inspection on x86_64, miri lane for
  the const paths).
- **Roundtrip Invariant:** decode(encode(x,y)) == (x,y) for all coordinates at
  levels 1..=16, plus geo-kernel's existing cell roundtrip courts
  (`crates/geo-kernel/src/cells.rs` tests module) stay green unchanged.
- **Over-refusal neighbor:** the full `CubeHilbertQ62V1` conformance set must produce
  byte-identical cell IDs before/after — any cell-ID change is an over-refusal-class
  regression of valid neighbors, gate fails.
- **Benchmark Target:** extend `purrdf_testkit::bench` with a morton/hilbert suite;
  report-only, per AGENTS.md perf discipline (no asserted speedup without bench).
- **Layer Boundary:** `check-layers.py` clean; ledger job extended under the
  existing v26.10.26 branchless-migration block in `helpers-ledger.toml`.

Reference: bcinr `crates/bcinr-logic/src/algorithms/{morton_encode_2d_u32.rs:22,
morton_decode_2d_u32.rs:22, morton_encode_3d_u32.rs:22,
hilbert_curve_encode_u32.rs:27, hilbert_curve_decode_u32.rs:28}`.

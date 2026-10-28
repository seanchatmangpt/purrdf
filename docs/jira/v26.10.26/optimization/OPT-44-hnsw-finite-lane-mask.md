# [OPT-44] HNSW Matrix Admission Finite-Check to NaN Lane Mask

## 1. TARGET CONTEXT
- **Crate & File:** `crates/hnsw/src/graph.rs:127-142`
  (`Vectors::first_non_finite`); consumer `graph.rs:192-198`
  (`assemble` refusal path)
- **Subsystem:** Memory Arena
- **Inspection Agent:** OPT-42 wave, ticket-writer lane (handoff §3.5,
  first half)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  ```rust
  Self::F32(data) => data
      .iter()
      .position(|value| !value.is_finite())
      .map(|index| (index, ComponentValue::Binary32Bits(...))),
  Self::F64(data) => data
      .iter()
      .position(|value| !value.is_finite())
      .map(...)
  ```
  (graph.rs:130-141). The whole flat row-major matrix is walked once,
  one `!is_finite` branch per component.
- **Hardware-Level Impact:** per-component branch that is never taken on
  healthy corpora: the predictor is ~100% not-taken, but the scalar
  loop still costs one compare+branch per element with no ILP across
  elements and no SIMD vectorization (`.position` on `&f32` with a
  closure LLVM declines to vectorize on the `Option` map). For a
  1M×128 f32 matrix that is 128M unpredictable-shaped iterations at
  admission time, serializing matrix assembly.
- **Baseline Cost:** UNVERIFIED — no admission-time validation bench
  exists; add one under `purrdf_testkit::bench` (AGENTS.md bench rule)
  before landing.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** reinterpret the flat buffer as u64 words
  (f64: one lane per u64; f32: 8-bit... precisely 4 lanes per u64) and
  build a **non-finite lane mask** with branchless compares: a lane is
  non-finite iff `(bits & ABS_MASK) == INF_PATTERN || exp_all_ones &&
  mantissa != 0` — equivalently `(bits << 1) > (INF << 1)` per lane
  (the classic one-compare finite test), computed SWAR-style or via a
  new `purrdf_branchless::simd_fallback::nonfinite_lane_mask_*` kernel.
  First hit = `mask.trailing_zeros()`; only that single component is
  captured as `ComponentValue` bits (existing scalar path). Healthy
  input: one mask build + one branch per 4-8 components.
- **Hard Bounds Applied:** fixed-bound word loads; zero allocation
  (mask built in registers); the captured-bits path stays scalar and
  cold.
- **Proposed Type Layout & Signatures:**
  ```rust
  // purrdf-branchless::simd_fallback (new, single home)
  pub fn nonfinite_mask_f32x8(word: u64) -> u64; // 1 bit per f32 lane
  pub fn nonfinite_mask_f64x1(x: u64) -> u64;    // 1 bit
  pub fn first_nonfinite_index(bits: &[u64], f32_lanes: bool) -> Option<usize>;
  ```
- **Reuse-First:** extend the `purrdf-branchless` simd_fallback/swar
  ledger rows — no in-crate SWAR body may be written in
  `crates/hnsw/`; hnsw gains a `purrdf-branchless` dependency edge (new
  layers.toml row, must pass `check-layers.py --home-for`).
- **Status:** NOT yet implemented —
  `purrdf_branchless::simd_fallback` has ct_* and pdep/pext kernels
  (simd_fallback.rs:41-330) but no float lane-mask kernel; the
  `(bits << 1) > threshold` formulation must land in branchless first
  (with its own f64 oracle tests incl. sign bit, subnormals, -0.0,
  NaN payloads), then wire here.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** `first_non_finite` via lane mask returns
  the identical `(index, ComponentValue)` as the scalar oracle on: all
  finite, NaN at each lane position, ±Inf, -0.0, subnormals, mixed
  f32/f64, and every `index % 4` alignment.
- **Allocation Invariant:** zero allocations on the healthy-input
  path (matrix assembly allocates nothing new).
- **Benchmark Target:** matrix-assembly admission bench (all-finite
  1M×128 f32) added to the testkit bench suites; report-only.
- **Layer Boundary:** new hnsw→branchless edge added to layers.toml;
  `python3 scripts/check-layers.py` exit 0; wasm32 `--lib` exit 0
  (the crate is wasm-clean today).
- **Valid-neighbor (refusal mirror):** over-refusal is the mirror bug
  here — a mask formulation that misclassifies a finite lane (e.g.
  -0.0, or f32 denormal flush) would reject a VALID matrix. Required:
  the paired valid-neighbor suite asserting every finite bit pattern
  (`f32::MIN`, subnormal, -0.0, smallest normal) still assembles
  successfully.

> STATUS CORRECTION (2026-10-27, W2 audit): the branchless half has LANDED —
> `first_nonfinite_index` at crates/branchless/src/simd_fallback.rs:430 and
> `first_nonfinite_f32` at :459 (differential test :743). The hnsw-side wiring
> remains open (crates/hnsw/src/graph.rs:127-140 still scalar; no branchless
> dep in hnsw Cargo.toml). The ticket's earlier "no float lane-mask kernel"
> claim is stale.

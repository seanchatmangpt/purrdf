# [OPT-20] Bench Coverage for Unwired Kernels + Helpers-Census Receipt

## 1. TARGET CONTEXT
- **Crate & File:** `crates/testkit/src/bench/` (mod.rs driver, stats.rs, estimates.rs, args.rs, store.rs, counter.rs); `crates/rdf-core/src/primitives.rs:9-48`; `crates/rdf-core/src/eytzinger.rs`; `crates/rdf-core/benches/` (no primitives/eytzinger target exists); `scripts/check-shared-helpers.py`; `Makefile:356-357,507-512`
- **Subsystem:** Attestation (bench harness + gates)
- **Inspection Agent:** Agent 20 (Sociable Chicago Bench & Layer Validation)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** The harness itself is statistically solid — 100 flat samples after a doubling warm-up sized to 5 s (`mod.rs:903-943` `fn sample`, `MIN_SAMPLE_SIZE = 10` at `:108`), 10,000 bootstrap percentile resamples of the median (`stats.rs:27-30`, Xoshiro256 seeded via `purrdf_hash::mix`, reproducible from the benchmark id via `fnv1a64` at `mod.rs:615`), Tukey 1.5/3.0 IQR outlier fences reported-never-removed (`stats.rs:95-115`), Hyndman-Fan type-7 quantiles, ±1% noise-band change verdicts, and hand-computed self-tests (`tests/bench.rs:34-624`). Timed paths are sleep-free; the only `sleep` is a 1 ms perf-ack poll outside timed regions (`counter.rs:183`). Chicago discipline is strong: real shipped collaborators on release-profile code, LUBM/WatDiv digest-pinned outside comparisons, `black_box` used correctly (`mod.rs:810-815`).
- **Hardware-Level Impact / Gaps:**
  1. **No ratio-fence check** — nothing criterion-style discredits a comparison when base and new samples disagree in outlier structure. Status: UNVERIFIED/absent.
  2. **The branchless kernels are dead AND unmeasured**: `swar_find_delimiter_u64`, `sort_quad_lanes_branchless`, `bitset_intersect_u64`, `clamped_scaling_u64` (`primitives.rs:9-48`) and `eytzinger.rs` (`build_eytzinger_layout`, `search_eytzinger_u64`) have zero production callers and zero bench coverage — only tiny unit tests (`tests/primitives_hotpath_tests.rs`). `duckdb/benches/sieve_bench.rs:21-61` contains an independent local SWAR reimplementation, not the shipped kernel.
  3. **Pre-existing live gate failure (NOT introduced by this inspection):** `python3 scripts/check-shared-helpers.py` exits 1 — 323 stale-capacity-proof FAIL lines, all one class, across rdf-core `ir/*` (builder, composite, dataset, frozen_terms, mutable, event_sink, global, ingest, paged/*, segmented/build, validate) and `turtle_render.rs`. `make helpers-hygiene` (`Makefile:507-512`) currently fails.
  4. Minor doc drift: 118 actual `[[bench]]` targets across 31 crates vs. 111 claimed.
- **Baseline Cost:** Layer validation verified clean: `python3 scripts/check-layers.py` → `OK: 270 first-party normal edges across 51 members match layers.toml` (exit 0). Unwired kernels carry no measured cost baseline at all.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:**
  - (a) Add a **primitives bench target** under `crates/rdf-core/benches/`: SWAR delimiter scan vs. `iter().position`, quad-lane sorting network vs. `sort_unstable`, `bitset_intersect_u64` word AND, `clamped_scaling_u64`; declared `Throughput::Elements`.
  - (b) Add an **eytzinger bench target**: `search_eytzinger_u64` vs. `binary_search` at dictionary sizes 10³–10⁷ across hit/miss/gaussian-skew query distributions, plus `build_eytzinger_layout` build throughput.
  - Both targets plug straight into `make bench` (`Makefile:357` already includes `-p purrdf-core`; the crate already carries 19 `[[bench]]` targets). Use `--save-baseline` for ratio tracking; until a ratio fence lands, cross-record comparisons must eyeball printed outlier counts.
  - (c) Design a ratio-fence check in `stats.rs`/`mod.rs`: a comparison whose base/new outlier classifications disagree is demoted to a "discredited" verdict rather than regressed/improved.
  - (d) File the 323-row helpers-census failure as its own remediation ticket — capacity proofs in rdf-core `ir/*` are stale, duplicate or incomplete vs. their sink closures; the census compiler must pass before `make check`-adjacent hygiene gates can be trusted for the optimization wave.
- **Hard Bounds Applied:** bench targets are report-only (never a gate, per `Makefile` / `docs/BENCHMARKS.md:5-9`); seed derivation stays pinned to benchmark ids for bit-reproducibility; no sleeps in timed paths.
- **Proposed Type Layout & Signatures:** follow existing `[[bench]]` conventions and `purrdf_testkit::bench` API; no new harness infrastructure beyond the ratio-fence classifier in `stats.rs`.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** each kernel's bench demonstrates its branchless/alloc-free claim (e.g., eytzinger search flat miss-cost vs. log-shaped binary search; SWAR ≥4× over scalar on escape-heavy inputs).
- **Allocation Invariant:** benches report-only; no bench may become a CI gate; timed regions stay sleep-free (no-sleep doctrine).
- **Benchmark Target:** baseline receipts saved for all four primitives + eytzinger build/search before any OPT ticket that wires them lands, so each wiring ticket has a before/after ratio.
- **Layer Boundary:** `check-layers.py` stays at 270 edges / 0 violations; `check-shared-helpers.py` moves from exit 1 (323 FAIL) to exit 0 via the separate remediation ticket — this ticket only receipts the failure and forbids new helpers-layer work while it stands.

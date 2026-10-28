<!--
SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
SPDX-License-Identifier: CC-BY-4.0
-->

# Build architecture: the fast lane over the 5-second local law

Status: DESIGN (wave v26.10.26, 2026-10-27). Companion to
`docs/design/full-tree-verification-lanes.md` (the serial correctness lanes)
and `docs/jira/v26.10.26/OPT-64-feedback-loop-sla.md` (the 5s enforcement
ticket). Every measured figure below carries its receipt; every unmeasured
figure is marked ESTIMATE and converts only when its first fast-lane run
appends a register row.

## 1. Constraints

- **5s SLA, hard kill.** Every local command runs under `scripts/bounded`
  (killed exit-124 at 5.0s wall; witnessed exit-124 at 5.024s with typed
  `BOUNDED-TIMEOUT[5s]` diagnosis — overnight receipt §4 (wave ledger), W1, ALIVE). The
  PreToolUse hook (`.claude/hooks/bounded_build_guard.py`) classifies and
  blocks build verbs; its corpus receipts 30 fire / 15 allow with a positive
  control (W3, rc 0).
- **CI-only gates.** `make check` / `make test` / clippy / conformance are
  CI-only (`.github/workflows/`); every Rust-executed result is UNVERIFIED
  locally until the remote CI run receipts it (overnight receipt §4, wave ledger).
- **RAM ceiling 48GB.** The host has 48GB; the fast lane's concurrency
  classes (jobs=8 compile, 4-way measurement) are chosen under this ceiling.
  A `rustc` OOM surfaces as SIGKILL — see §6.
- **Measurement-bound floor.** The floor of any end-to-end claim is the
  measured register, not the plan: a claim that cannot name its register row
  is UNVERIFIED. The bounded ladder budgets (`scripts/fb-budgets.json`: t1
  0.5s / t2 0.8s / t3 1.5s / total 2.5s, downward-only ratchet) are the law
  the fast lane sits beside, never under.

## 2. The two-lane model

| Property | Serial correctness lane | Fast lane |
|---|---|---|
| Purpose | Is the tree green? | How fast is it, measured? |
| Home | phase runners; `ci.yaml` shards | `scripts/run_benches_fast.py` |
| Jobs | 2 | 8 cold, 4-way measure |
| Target | warm `/tmp/purrdf-targets` leases | cold `/tmp/purrdf-fast-target` + `t<N>` leases |
| Receipt | register row (`register.txt`) | register-fast row (`register-fast.txt`) |
| Status | LANDED | LANDED (script), UNVERIFIED (first wave) |

The fast lane splits `cargo bench` into two phases (`scripts/run_benches_fast.py`
header, LANDED): **Mode A** compiles per crate with `cargo bench --no-run`
(offline, jobs=8, incremental off, sccache when present) into the shared cold
target, then enumerates bench binaries from `target/release/deps`; **Mode B**
runs each binary with a worker pool (max 4), each worker holding its own
`CARGO_TARGET_DIR` lease created on start and deleted at exit (the lane-lease
law, `scripts/lane-lease.sh`). The serial lane's jobs=2 / warm-shared-target
shape exists so a measurement never contends with a correctness compile for
RAM; the fast lane's jobs=8 + 4-way is the measured compromise under the 48GB
ceiling. ESTIMATE: the exact RAM-per-worker class is not yet receipted; the
first 4-way wave receipts it or trips SIGKILL (§6).

## 3. Config-poisoning guard

**Rule: the fast lane's configuration is env/wrapper-only.** All fast-lane
tuning lives in `scripts/sccache-fast.sh` (env vars exported in-process only)
and the phase runner's `build_env` — never in `Cargo.toml`, never in a repo
`.cargo/config.toml`, never in a shell profile.

Why repo `.cargo/config.toml` is rejected:

1. **Leak to the serial lane.** `.cargo/config.toml` is ambient: it would
   silently retune CI and every serial correctness lane (jobs, wrapper,
   cache dir), breaking their receipt identity — a register row would no
   longer name the lane that produced it.
2. **Wrapper-invalidation poisoning.** Toggling `RUSTC_WRAPPER` changes
   cargo's compiler fingerprint: flipping it on (or off) mid-tree forces a
   full rebuild (recorded in `scripts/sccache-fast.sh` header, sccache 0.16.0
   note). An ambient config makes that toggle repo-global; the wrapper-only
   placement keeps it opt-in per process.
3. **Reproducibility of receipts.** A receipt must be replayable from the
   named environment; an ambient repo config is an unnamed environment.

`sccache-fast.sh` sets `RUSTC_WRAPPER` (universally compatible surface; a
caller may override to `CARGO_WRAPPER`), `SCCACHE_DIR=/tmp/purrdf-sccache`
(20G), `CARGO_BUILD_JOBS=8`; it REFUSES typed (`SCCACHE-FAST[REFUSED]`) when
sccache is absent — a refusal is a claim too, and it is loud.

### 3.1 Measured precedence on this box

Measured on this machine (cargo 1.101.0-nightly): env vars beat
`.cargo/config.toml` discovered by walking up from the runner's cwd, which
beats `$CARGO_HOME/config.toml` (the user layer is currently empty). Config
discovery follows the runner's **cwd, not `CARGO_TARGET_DIR`** — target-dir
isolation does NOT isolate config. The repo's live `.cargo/config.toml`
(`[build] jobs = 2`) is read by every fresh cargo invocation the /tmp phase
runners spawn; only the fast lane's `CARGO_BUILD_JOBS=8` env override shields
`jobs`. Reference implementation: `run_benches_fast.py` `build_env()`.

Operational rule while any bench runner is alive: zero bytes under `.cargo/`
or in `Cargo.toml` / `Cargo.lock` may change; the repo config must gain no
`RUSTC_WRAPPER` / `rustflags` / `jobs` lines in flight (any of them
re-fingerprints the tree under the runners — §3.2 plus fingerprint churn,
§6).

### 3.2 Placement verdicts

| Placement | Verdict | Why |
|---|---|---|
| repo `.cargo/config.toml` | REJECT | ambient; poisons CI + serial lanes (above) |
| `$CARGO_HOME/config.toml` override | safe-scoped | user layer, outside the repo tree |
| pure process env | safe | named in the receipt's environment |
| wrapper script (`sccache-fast.sh`) | safe | exported in-process only, opt-in per run |

## 4. Lever ledger (the napkin table)

- **jobs 3-4x (3→8)** — basis: parallelism under the 48GB ceiling. Receipt:
  UNVERIFIED — no register-fast row yet.
- **warm target** — basis: warm link-class 0.11s (overnight receipt §3.1, measured precedence).
  Receipt: ALIVE class, lane-measured.
- **sccache 70-90% hit rate** — basis: sccache's published hit-rate class.
  Receipt: UNVERIFIED; ESTIMATE, never locally measured.
- **linker skipped (bench --no-run split)** — basis: the Mode A/B split
  removes the link from the measured phase. Receipt: LANDED (script),
  UNVERIFIED wave.
- **duckdb path patch (frozen, not source)** — basis: source DuckDB is a
  15-40 min banned class; frozen is download + ~0.11s link. Receipt: LANDED
  (`frozen-duckdb 2.0.2`, checksum-pinned, overnight receipt §3.1, measured
  precedence).
- **shard split (7 shards vs 1 job)** — basis: the one-job form compiled
  25 min and was cancelled at timeout (`ci.yaml:306-314`). Receipt: LANDED (CI).

The list is the napkin: basis = why the lever plausibly moves the floor;
receipt = its standing, never rounded up.

## 5. Floor estimates (ESTIMATE until register-fast receipts them)

| Class | Estimate | Standing |
|---|---|---|
| Cold wave compile (Mode A, jobs=8, sccache from cold) | 10-20 min | ESTIMATE |
| Warm wave compile (warm target, warm sccache) | 5-10 min | ESTIMATE |
| End-to-end 4-way (compile + 4-way measure) | 20-30 min | ESTIMATE |

These are planning numbers from the wave input, not measured receipts. The
first fast wave converts each to a measured class by appending rows to
`register-fast.txt` and naming this document's classes; until then the only
measured compile classes are CI's (25-min one-job cancel; shard split) and
the warm-link 0.11s class.

## 6. Failure taxonomy

- **SIGKILL / OOM** — signature: rc=137 on a bench binary. Response: retry
  once solo, then record red (wired in the runner); repeat solo = 4-way over
  the 48GB ceiling — drop to 2-way, re-receipt.
- **Lock contention** — signature: stalls across lanes sharing one target.
  Response: structural fix only (per-worker `CARGO_TARGET_DIR` leases),
  never a longer wait.
- **Fingerprint churn** — signature: full rebuilds after wrapper toggles.
  Response: sccache from cold only; never toggle `RUSTC_WRAPPER` mid-tree
  (§3.2).

Each class must produce a typed line in the register, never a silent
absence — a missing row is a refusal, not a pass.

## 7. Receipts and non-vacuity

Two register formats:

- **register.txt** (serial lane): one row per phase runner execution.
- **register-fast.txt** (fast lane): one row per bench binary, format
  `CRATE BENCH=NAME rc=R SECS=S log=<path>` (`scripts/run_benches_fast.py`
  `register_row`), appended per completion; `--resume` skips crates with
  existing green (rc=0) rows.

Non-vacuity rules:

1. **The register is the register of truth.** A green claim must name its
   row. A claim that cannot name a row is UNVERIFIED.
2. **Absence is not a pass.** `register-fast.txt` does not exist yet; until
   the first fast wave appends to it, the fast lane's standing is UNVERIFIED
   for every class in §5. (Same fail-closed shape as `scripts/fb-receipts.jsonl`
   — absent-by-design until first run, court exits 2 until then.)
3. **A fast lane that never fails typed is a vacuous gate.** The runner must
   be able to record red: rc=137 retries solo, typed errors propagate to the
   register. A court proving non-vacuity: inject a failing bench, require a
   red register row, restore byte-exact (the shim-court pattern). Such a
   court is REQUIRED before any end-to-end floor claim in §5 is upgraded from
   ESTIMATE.
4. **Retry-once-then-red is the only retry law.** Retrying until green
   launders a flaky class into a measured class — that is mock-laundering in
   a register format.

### 7.1 Register formats

Two register files exist; they are **separate series**, never summed. A
count that adds rows across both files is a miscount. (The wave's
purrdf-rdf "122 vs 123" episode ended at 123 across 7 binaries — the
122 recount had dropped the viz_layout footer; see the coverage
report's final recount and its correction history.)

- **`register.txt`** (serial lane, phase runners) — one row per phase-runner
  execution: `CRATE rc=R S s [cap=C] [note] log=PATH` (bare seconds, not
  `Ns=S`; some rc=0 rows carry a parenthesized `(footer N benchmarks
  measured; 0 failed)` field instead of seconds). Receipts:
  `docs/design/bench-coverage-report.md` §1d (rows summarized into a
  markdown table, not copied verbatim).
- **`register-fast.txt`** (fast lane, `scripts/run_benches_fast.py`) — one
  row per bench attempt, `CRATE BENCH=NAME rc=R SECS=S log=PATH`, appended
  under flock in a single `O_APPEND` write; `--resume` skips a crate only
  when every manifest bench has a green (rc=0) row. Producer:
  `register_row` (`scripts/run_benches_fast.py`).

**rc=124 has three distinct meanings — always disambiguate by lane before
reading the row.**

1. **Serial phase-runner timeout, with footer.** The runner killed at its
   wall budget; the paired log still closes with a typed
   `N benchmarks measured; 0 failed` footer, which — not the `rc=` string —
   is the receipt for the measured count.
2. **Phase-4 timeout, no footer.** Same rc, but the log has no measured
   footer: the row receipts only that the run was killed, and the crate
   counts as red (fix-queue), not partially green.
3. **Fast-lane CAP expiry.** `run_benches_fast.py` kills the bench at its
   CAP and normalizes the death to rc=124 (rc is never None); SIGKILL
   deaths are normalized to rc=137.

**rc=137 = OOM signature (fast lane).** A SIGKILL'd attempt records its own
red row, then is retried exactly ONCE, SOLO (pool drained to empty first);
the retry row is marked ` retry=1` and a green retry makes the bench green.
Retry-once-then-red is the only retry law (rule 4 above); repeat-solo
SIGKILL means 4-way exceeds the 48GB ceiling — drop to 2-way, re-receipt
(§6).

**Never-sum rule.** Counts are per-series: serial greens sum only over
`register.txt` rows, fast-lane greens only over `register-fast.txt` rows.
No documented total may combine the two; where both lanes measure the same
crate, the rows are independent measurements, not partitions of one total.

### 7.2 Report-only / capacity-probe tier

A third tier of ordinary `[[bench]]` targets exists that is **run under
`make bench` only** and never enters any measured count: rows from this tier
carry no measured count in any total, in either register series.

Two member shapes:

- **Report-only tables.** Benches whose output is a table read by eye, not
  a criterion measurement: the hnsw build/recall tables, and the purrdf-core
  `purremb_alloc` + `pack_restore_alloc` allocation probes. They run, they
  print, they are receipts for behavior (alloc shape, recall at a build
  config), never for a count of greens.
- **Capacity-probe rungs.** The geo-kernel geodesic / transforms / covers
  benches gated behind `GEO_GEODESIC_LARGE_BATCH` /
  `GEO_TRANSFORMS_LARGE_BATCH` / `GEO_COVERS_LARGE_RADIUS` environment
  gates, default OFF, checked strict (`=="1"`). With the gate unset they
  exit early; with the gate set they run their probe rungs (`batch4096`,
  `radius-400000`), which exist to answer "does this fit in capacity at
  all", not to produce a comparable measurement. Explicit run:

  ```sh
  GEO_GEODESIC_LARGE_BATCH=1 cargo bench -p purrdf-geo-kernel -- geodesic
  GEO_TRANSFORMS_LARGE_BATCH=1 cargo bench -p purrdf-geo-kernel -- transforms
  GEO_COVERS_LARGE_RADIUS=1 cargo bench -p purrdf-geo-kernel -- covers
  ```

  The hnsw scale knob is the narrowing counterpart:
  `PURRDF_HNSW_BENCH_SCALES` replaces the default ladder with a subset of
  the declared rungs (5k/50k/200k/1M rows × 4096 dims) — it can only take
  rungs away, never add them; a non-ladder value hard-fails
  (`crates/hnsw/tests/support/corpus.rs:156-185`,
  `crates/hnsw/README.md` §scales). Note: the geo-kernel `cells.rs`
  `assignment_batch` ladder still reaches batch4096 unconditionally —
  measured green in-phase (cheap grid assignment, unlike the projection
  math the gates protect), but it is the one ungated 4096 rung left.

**Exclusion-from-totals rule.** Tier members are never counted: not in the
serial register totals, not in fast-lane totals, not in any documented sum.
No measured footer in a tier log means no count exists for it — absence of a
footer is the signal, not a count to reconstruct. An unmeasured point is not
a claim: a tier row receipts that the bench ran (or was skipped by its gate),
never that a number was measured.

## 8. Hygiene and the fast-lane target family

The fast-lane target family — `/tmp/purrdf-fast-target`,
`/tmp/purrdf-fast-t0..t3`, `/tmp/purrdf-sccache` — is invisible to the
existing hygiene tooling: `scripts/target-hygiene.sh` scopes the repo root
only, and `scripts/lane-lease.sh` hardcodes the
`/tmp/purrdf-targets/lane-*` glob. Five rules close that gap:

1. **`--fast-scope` flag on hygiene.** `target-hygiene.sh status --fast-scope`
   enumerates all three paths and du-reports `/tmp/purrdf-sccache` against
   `PURRDF_FAST_BUDGET_GB` (default 30). The sccache cache is never
   `rmtree`d — it is self-evicting at its 20G `SCCACHE_CACHE_SIZE`; deleting
   it wholesale would evict the hit-rate lever, not reclaim waste.
2. **`prune --fast-scope` owns only the measurement leases.** It deletes
   `/tmp/purrdf-fast-t*` entries older than 1 day (they are per-run leases:
   older than a day = orphan by definition), via a fail-closed
   `rmtree_one` that refuses rather than best-effort-skips.
3. **Orchestrator signal contract.** The runner removes every lease it
   created on normal exit, SIGINT, and SIGTERM — the SIGTERM handler kills
   worker processes first, then cleans; `finally` covers SIGINT.
   SIGKILL/crash is the hygiene backstop (rule 2), never the primary
   cleanup path.
4. **`/tmp/purrdf-fast-target` is best-effort cache.** The macOS ~3-day
   `/tmp` reaper may delete it at any time; no correctness assumption rides
   on its persistence. A missing shared cold target means a cold compile,
   not a failure.
5. **Fast leases are a separate family.** They live outside
   `lane-lease.sh`'s contract — documented here, not silently absorbed by
   the `lane-*` glob.

## 9. 4-way measurement safety and the Uniform-Load Rule

**Proven safe (4-way concurrency).** No bench binds a port; zero `/tmp`
literals or lockfiles exist under `crates/*/benches`; testkit `fresh_path`
names are pid+counter+nanos with exclusive create; estimates stores are
partitioned per bench id. Wave rule: one binary per bench per wave.

**Honesty class.** The harness is wall-clock sampling with no CPU pinning
and no contention record. Hence the **Uniform-Load Rule**: a compared series
is all-parallel (fast lane — throughput-indicative only) or all-serial (gate
comparisons), never mixed. The ±1% noise bootstrap would misread
cross-regime load as regression. Tag baselines by regime
(`serial-<rev>` vs `par-<rev>`) — the estimates store does not do this
itself; it is operator discipline until wired.

**Thread caps are REQUIRED for parallel waves**: `RAYON_NUM_THREADS=4` per
binary. Caveat: `crates/sparql-eval/benches/regex_eval.rs:143-152` (and its
`expr_vm` / `query_eval` siblings) pin the rayon pool to
`available_parallelism()` in code and ignore the env var until changed —
a known follow-up, not a fast-lane blocker if those crates run in serial
waves.

## 10. fb-SLA noninterference

**Structural for all repo-driven paths.** `sccache-fast.sh` is
subtree-scoped and the fb chain (`scripts/fb-receipt.sh` → `fb-ladder.sh` →
`fb-court.py`) never touches it; the bounded 5s timer and the orchestrator's
3600s cap are disjoint; the lane-lease t3 target dir is disjoint from
`/tmp/purrdf-fast-target`.

**Residual leak, fail-closed.** A manually exported `RUSTC_WRAPPER` or
`CARGO_BUILD_JOBS` reaches t3's bounded `cargo check` unscrubbed. The
failure mode is typed TIMEOUT (exit-124 with `BOUNDED-TIMEOUT[5s]`), never
silent — fail-closed, so the law holds. Optional hardening: unset
`RUSTC_WRAPPER` and force `CARGO_BUILD_JOBS` in `run_t3`
(`scripts/fb-ladder.sh:111`).

## See Also

- `docs/design/full-tree-verification-lanes.md` — the serial correctness lanes
- `docs/jira/v26.10.26/OPT-64-feedback-loop-sla.md` — the 5s SLA ticket
- `scripts/run_benches_fast.py`, `scripts/sccache-fast.sh` — the fast-lane homes
- `scripts/lane-lease.sh` — per-lane target leases (serial family)
- `crates/sparql-eval/benches/` — the rayon-pinned bench caveat (§9)
- `scripts/target-hygiene.sh` — hygiene (repo scope; `--fast-scope` design §8)
- `docs/design/purrdf-bench-lane-laws.md` — what a bench lane's digest certifies

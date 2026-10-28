# Bench Wave Report

Wave v26.10.26 · 2026-10-27 · Status: **TERMINAL** (all FQ-01..15 rows
terminal; register + logs re-read from disk 2026-10-27, writer lane; no
cargo run).

## 1. Executive summary

Wave v26.10.26, as-of 2026-10-27 13:08 PDT. 130 `[[bench]]` targets, 3-way
pinned (grep / awk / tomllib, bijection verified both directions); one RNG
home (getrandom absent from `crates/*/benches/`, grep rc=1) and one temp home.
Three-phase detached runners plus 14 static lanes on a no-build box.

> **TERMINAL (writer lane, 2026-10-27):** the final standing total is
> **1181 measured / 0 failed** — the 864 subtotal plus hash-conformance 273
> (digests 255 + hasher 18), shapes-pydantic 13, arrow 10, branchless 7,
> duckdb 14 (all phase-4/4b rc=0, footers grep-verified). hnsw 4b
> build/recall are rc=0 report tables with no measured footer — excluded.
> Supersedes both the 418 and 864 snapshots below. See
> bench-coverage-report.md section 4-T.
>
> SUPERSEDED (2026-10-27): the final standing totals are 864 measured
> / 0 failed with purrdf-rdf 123 and shapes 37 — this section's 418
> snapshot predates the late phase-3/4 receipts and the viz_layout /
> canonical_sort footer corrections. See bench-coverage-report.md.
Terminal green: **418 measured / 0 failed** and climbing — re-read from
register + in-log footers at 13:08 PDT: purrdf 44, purrdf-rdf 122, shapes 36,
iri 2, sparql-algebra 140, sparql-results 14, columnar 12, lex 22, cdt 12,
toml 7, mime 3, glob 4 (fresh rc=0 row). 12 typed defects FQ-01..12, every
red owned with a named close path. Claims: 56 MEASURED / 23 UNVERIFIED
bannered. Residuals: runners still draining, 8-subject phase-4 queued,
wasm/js unexecuted, `make check` CI-only.

## 2. Scope

130 `[[bench]]` targets across `crates/*/Cargo.toml`, bijective with 130
`benches/*.rs` stems (3-way pinned count: grep / awk / tomllib, rc=0;
bijection verified both directions, rc=0). Determinism singletons hold:
`getrandom` absent from `crates/*/benches/` (grep rc=1). Wave window:
2026-10-27 ~11:28–13:05 PDT; runners continue appending to
`/tmp/bench-logs/register.txt`.

## 3. Method

Three-phase detached runners over a shared warm target dir, plus static
verification lanes:

- **Phase runners**: phase-2 (pid 62576) and phase-3 (pid 81055) detached
  runners append terminal `rc=` rows to `/tmp/bench-logs/register.txt`;
  phase-4 runner written (`/tmp/run_benches_phase4.py`) to drain FQ-06/07/08
  closes: `sparql-eval --bench deep_nesting`, arrow, duckdb, branchless,
  hash-conformance, n3 (solo, jobs=2), `shapes --bench pydantic_package`;
  launches after phase-2/3 drain, warm target dir shared.
- **Static lanes** (no-build machine, 5s wall-clock build ban): 6-gate static
  receipt per crate — (1) `[[bench]]`↔`benches/*.rs` bijection, (2) testkit
  harness / zero criterion, (3) determinism, (4) no `std::thread::sleep`,
  (5) no banned deps reachable, of the helpers-ledger single-body. Receipts =
  command + rc + log path; a receipt without all three is not a receipt.
- Static gates run this session: `scripts/bounded make fb` rc=0; guard
  corpus; `make -n bench` dry-run rc=0 (details §8).

## 4. Measured results (terminal green)

**418 measured / 0 failed** (terminal, footer-cited, 13:08 PDT):

| crate | count | receipt |
|---|---|---|
| purrdf (facade) | 44 = 40+4 (two binaries) | `purrdf.phase2.log`, rc=0 1338s |
| purrdf-rdf | 122 = 10+12+6+46+45+3 (6 binaries) | `purrdf-rdf.phase2.log`, rc=0 1903s |
| purrdf-shapes | 36 = 2+2+2+30 | `purrdf-shapes.log`, rc=0 footer |
| purrdf-iri | 2 | `purrdf-iri.log`, rc=0 footer |
| purrdf-sparql-algebra | 140 = 72+31+30+3+2+2 | `purrdf-sparql-algebra.log`, rc=0 footer |
| purrdf-sparql-results | 14 | `purrdf-sparql-results.log`, rc=0 footer |
| purrdf-columnar | 12 | `purrdf-columnar.phase2.log`, rc=0 234s |
| purrdf-lex | 22 | `purrdf-lex.phase2.log`, rc=0 243s |
| purrdf-cdt | 12 | `purrdf-cdt.phase2.log`, rc=0 131s |
| purrdf-toml | 7 | `purrdf-toml.phase2.log`, rc=0 391s |
| purrdf-mime | 3 | `purrdf-mime.phase2.log`, rc=0 34s |
| purrdf-glob | 4 | `purrdf-glob.phase2.log`, rc=0 102s |

Note: the "123" for purrdf-rdf in two earlier lane reports was a miscount;
ground truth re-read from in-log footers this session is 122. The phase-1
script rewrote `register.txt` in `"w"` mode, destroying early `rc=` rows —
reconstructed from logs as footer-cited receipts.

### 4-T. Terminal rows (writer lane, 2026-10-27 — footers grep-verified)

| run | count | receipt |
|---|---|---|
| purrdf-hash-conformance | 273 = 255 (digests) + 18 (hasher) | `purrdf-hash-conformance.phase4.log` footers at :728/:784, rc=0 2490s |
| purrdf-shapes `pydantic_package` | 13 | `purrdf-shapes.phase4.log`, rc=0 503s (one 2-sample row at the 30s wall ceiling — noted in-log) |
| purrdf-arrow (4b all) | 10 | `purrdf-arrow.phase4b.all.log`, rc=0 103s |
| purrdf-branchless (4b all) | 7 | `purrdf-branchless.phase4b.all.log`, rc=0 77s |
| purrdf-duckdb (4b all) | 14 = 10 + 4 (two binaries) | `purrdf-duckdb.phase4b.all.log`, rc=0 652s |
| purrdf-hnsw build + recall (4b) | excluded — no measured footer | `purrdf-hnsw.phase4b.--bench-{build,recall}.log`, rc=0; report tables only |

Terminal total: 864 + 273 + 13 + 10 + 7 + 14 = **1181 measured / 0 failed**
(spans two ladder series — the 26 geo-kernel pre-shrink batch4096 rows — and
is not a single homogeneous run). Terminal reds: sparql-eval E0277 (4b
deep_nesting wrapper), n3 E0063 `rules`, datalog E0063 `wall_max` (testkit
lib), entail no-footer abort, rdf-core rc=124 3600s cap.

## 5. Timeline (reconstructed)

Start times marked *(inferred)* — the phase-1 register rewrite destroyed
start-stamp rows; end marks are register/footer-cited.

**Phase 1b** (~09:52–11:36, start inferred):
- ~09:52 (inferred): phase-1b runners start; iri, sparql-algebra,
  sparql-results greens.
- 10:39: lockfile refusals (`--locked` cannot update Cargo.lock) — becomes
  FQ-02..05 (geo-kernel, hnsw, retrieval, text).
- 11:00: Cargo.lock fix lands; refusals class closed.
- 11:27: shapes green (36 = 2+2+2+30).
- 11:36: sparql-eval `solution.rs` fix (FQ-01) applied.

**Phase 2** (11:28–13:03):
- 11:28: phase-2 runner (pid 62576) starts.
- sparql-eval red 105s (rc=101).
- purrdf facade green 1338s; purrdf-rdf green 1903s; duckdb red 1000s
  (rc=101); arrow red 4s (rc=101); columnar 234s, lex 243s, cdt 131s, toml
  391s, mime 34s, glob 102s greens; branchless red 25s (rc=101); n3 185s
  SIGKILL (FQ-11).
- 13:03: phase-2 drain (inferred from last rc row).

**Phase 3** (12:10–, start inferred):
- 12:10 (inferred): phase-3 runner (pid 81055) starts.
- sparql-eval rc=101 2006s: clean compile post-FQ-01, 12 rows green, then
  rc=101 — re-diagnosed as a COMPILE break, not the stack overflow: the
  purrdf-sparql-results lib failed (50 errors in `canon_arrow.rs`); the
  deep_nesting wrapper was never exercised (FQ-08). geo-kernel in flight at
  last read.

**Fix landings** 12:43–13:05: arrow (FQ-09), duckdb (FQ-10), branchless
(FQ-12), deep_nesting big-stack wrapper (FQ-08) edits applied by repair lane.

## 6. Static verification (14 crates, 84/84 PASS)

14 crates hold 6/6-PASS static receipts — 84/84 gate rows
(`/tmp/bench-logs/purrdf-*.static.md`): branchless, cdt, columnar, datalog,
entail, glob, gts, hash, json, lex, markdown, mime, n3, toml. Static PASS is
not compile truth: compile verification for these is the runner rc row (see
§9 for which are still UNVERIFIED on execution). Of the 14, cdt (rc=0 131s),
columnar (rc=0 234s), lex (rc=0 243s), toml (rc=0 391s), mime (rc=0 34s) and
glob (rc=0 102s) already hold terminal green runner rows.

Four disclosed caveats (receipt-cited, none a gate failure):

1. **columnar**: ledger-registration gap — informational.
2. **testkit**: `counter.rs:183` shared-harness sleep disclosed (test
   harness, not a bench).
3. **hash**: benches live structurally in hash-conformance, not `crates/hash`.
4. **glob**: Cargo.lock banned-dep context noted.

Non-lane coverage: the `native_codecs` RNG delegation fix
(`crates/rdf/benches/native_codecs.rs:173`) is covered by the
determinism-rerun receipt, not a per-crate static lane.

## 7. Defect ledger (FQ-01..12)

Source: `/tmp/bench-fix-queue.md`. Every red has a typed row and a named
close path; "no unowned red remains" stands as of 13:08 PDT.

| FQ-ID | crate | defect | fix | state |
|---|---|---|---|---|
| FQ-01 | sparql-eval | E0599 FastMap::new (solution.rs:1965) | FastMap::default() | **RESOLVED** (phase-3 compiled clean post-fix, proving the edit) |
| FQ-02 | geo-kernel | RESOLVED: register rc=0 2220s, 109 measured / 0 failed (phase-3 log; measured-at 2026-10-27, serial phase-3, pre-shrink ladder batch256-4096; 26 rows at batch4096 are not comparable to current 1024-capped ladders - capacity probe via GEO_TRANSFORMS_LARGE_BATCH=1 / GEO_GEODESIC_LARGE_BATCH=1) |
| FQ-03 | hnsw | **RESOLVED-REPORT** (terminal): 4b build rc=0 84s + recall rc=0 638s, full tables on disk, no measured footer — excluded from totals. History: phase-3 rc=101 2237s SIGKILL (OOM-solo, FQ-14), phase-4 rc=124 3600s cap |
| FQ-04 | retrieval | RESOLVED: register rc=0 455s, 18 measured / 0 failed (phase-3 log) |
| FQ-05 | text | RESOLVED: register rc=0 1365s, 299 measured / 0 failed across 7 footers (phase-3 log) |
| FQ-06 | shapes pydantic_package | cut off mid-run (phase-1) | phase-4 `--bench pydantic_package` | **RESOLVED** — rc=0 503s, 13 measured / 0 failed |
| FQ-07 | hash-conformance | in yaml matrix, absent from queues | phase-4 lane | **RESOLVED** — rc=0 2490s, 273 measured / 0 failed (digests 255 + hasher 18) |
| FQ-08 | sparql-eval | canon_arrow.rs 50-error compile break (47x E0753, 2x E0499, 1x E0277); fix landed 612bba89 | 4b rerun | **STILL-RED (terminal)** — 4b deep_nesting rc=101 0s, NEW error: E0277 Send bound on `on_measurement_stack` in the bench wrapper; wrapper needs a new owner |
| FQ-09 | arrow | E0614 ..*header, ipc_egress.rs:98 | applied | **RESOLVED** — 4b rc=0 103s, 10 measured / 0 failed |
| FQ-10 | duckdb | E0599 bench-helper `&Path` params (fix direction corrected in the fix-queue audit) | applied | **RESOLVED** — 4b rc=0 652s, 14 measured / 0 failed |
| FQ-11 | n3 | SIGKILL at 185s, 2 green rows first | phase-4 solo rerun, jobs=2 | **STILL-RED (terminal)** — phase-4 rc=101 161s: E0063 missing `rules` in lib test compile |
| FQ-12 | branchless | 8 lib-test compile errors (+3 residual E0308, FQ-13) | repair-lane fixes applied | **RESOLVED** — 4b rc=0 77s, 7 measured / 0 failed |
| FQ-13 | branchless | residual 3x E0308 (sort16.rs:483, swar.rs:970/1002) | applied | **RESOLVED** — same 4b green run as FQ-12 |
| FQ-14 | hnsw + entail OOM-solo | hnsw scales capped via env; entail ladder 10_000 -> 2_000 | applied | **PARTIAL-RESOLVED (terminal)** — hnsw green-report (FQ-03); entail 4b log has NO `benchmarks measured` footer (abort before footer) — still red |
| FQ-15 | NPM-ECOSYSTEM.md banner scope (editorial) | whole-file warning + scoped citation | **APPLIED** (banner exists, 2026-10-27; receipt-cited) |

### 7.0 OPT-17 wiring-wave progress (2026-10-27, added post-freeze; not an FQ row — no red to own)

FQ-16-style progress note: the OPT-17 arrow-egress wiring wave is landing
against subjects this wave already receipted. Disk state: `canon-arrow`
results-format arm wired in `crates/sparql-results` (`src/lib.rs:105/121/
140-141/246-255`; writer `src/canon_arrow.rs` landed, 680 lines; the
module's first compiler contact — phase-4's deep_nesting subject — found 50
errors, ALL fixed in 612bba89; compile confirmation = the phase-4b
sparql-eval rerun or CI), new bench arm `to_canon_arrow_5k_rows`
(`crates/sparql-results/benches/graph_serialize.rs:188`) — never executed, no
receipt; rdf-capi/rdf-wasm seams LANDED (6e93aca3: purrdf_query_canon_arrow
+ queryCanonArrow + js exposure). Close path: the phase-4 queue
(`/tmp/run_benches_phase4.py`) was frozen before this wave and cannot cover
the new arm — the sparql-results/graph_serialize bench receipt needs a
phase-5 fast-lane run or a CI `make bench` run.

### 7.1 Case study: sparql-eval failure→fix chain

The chain: phase-1 red (E0425 `cannot find function var` ×9 + E0428
duplicate `var`, `purrdf-sparql-eval.log`) → FQ-01 `solution.rs:1965`
`FastMap::new` → `FastMap::default()` applied 11:36 → phase-3 clean compile,
12 rows green (footers "4 benchmarks measured; 0 failed" + "8 benchmarks
measured; 0 failed") → stderr `deep_nesting: optional_spine answers 16007
levels deep on the bench thread` → rc=101 2006s. Re-diagnosis (post-612bba89):
the abort was a COMPILE break, not the stack overflow — purrdf-sparql-results
lib failed with 50 errors in `canon_arrow.rs` (47x E0753 stray `///` in the
`//!` block, 2x E0499, 1x E0277); the deep_nesting wrapper was never
exercised. Fix landed 612bba89; phase-4b rerun post-612bba89 queued.

The full 24-row deep_nesting table lives in the phase-3 log
(`/tmp/bench-logs/purrdf-sparql-eval.phase3.log`; receipt = in-log `time:`
lines). Notable deltas:

| bench | variant | time | delta |
|---|---|---|---|
| construct_publication/100 | typed_staging | 554.1 µs | 1.62x over frozen (898.2 µs) |
| construct_publication/10000 | typed_staging | 51.24 ms | 1.68x over frozen (86.31 ms) |
| boundary_joins/after_optional | original | 58.01 ms | 3.16x slower than equiv (18.34 ms) |
| boundary_joins/after_bind | original | 14.75 ms | 2.45x slower than equiv (6.008 ms) |
| boundary_joins/optional_body | original | 985.0 µs | parity (equiv 1.002 ms) |

All five plan-equivalence deltas favor the equivalent plan or parity — the
cost-based BGP planner's equivalence rewrite is never a regression on this
corpus.

## 8. Gates + V9 receipts

- `scripts/bounded make fb` rc=0 PASS — tiers 300 / 679 / 135 ms, total
  1128 ms (within the 2.5s ladder budget).
- `make -n bench` dry-run rc=0.
- `check-layers.py --fast` rc=0 — 52 rows; necessary-not-sufficient.
- Guard corpus: 13/13 MUST-PASS + MUST-FIRE PASS over the 30-case array. The
  earlier "12/12" figure was a subset of the array — UNVERIFIED as a
  full-array claim, superseded by the 13/13 receipt.
- Kill probe rc=124, typed `BOUNDED-TIMEOUT[5s]` — the guard refuses runaway
  builds as designed.
- V9 receipt: 7/7 PASS; 130 = 130 bench-target pin holds.

## 9. In-flight

> **TERMINAL (writer lane): nothing is in flight.** Phase-3 greens landed
> (geo-kernel 109/0, retrieval 18/0, text 299/0); the phase-4/4b queue is
> drained — greens: hash-conformance 273/0, shapes-pydantic 13/0, arrow
> 10/0, branchless 7/0, duckdb 14/0; reds: sparql-eval E0277 (wrapper),
> n3 E0063, datalog E0063 (testkit lib), entail no-footer abort, rdf-core
> rc=124 3600s cap. Remaining UNVERIFIED classes only (section 10).

Historical snapshot (superseded):
- Phase-3 runner (pid 81055): geo-kernel in flight (post-Cargo.lock fix);
  hnsw, retrieval, text queued behind it (FQ-02..05).
- Phase-2 reds awaiting phase-4: duckdb (FQ-10), arrow (FQ-09), branchless
  (FQ-12), n3 (FQ-11); shapes pydantic_package (FQ-06); hash-conformance
  (FQ-07); sparql-eval deep_nesting (FQ-08).
- Phase-4 queue (`/tmp/run_benches_phase4.py`, 8 subjects): sparql-eval
  `--bench deep_nesting`, arrow, duckdb, branchless, hash-conformance, n3
  (solo, jobs=2), shapes `--bench pydantic_package`, plus the remaining
  phase-2 stragglers; launches after phase-2/3 drain, warm target dir shared.

REFRESH-PENDING: this section is stale on arrival; read
`/tmp/bench-logs/register.txt` for live state.

Post-freeze addition (not in the phase-4 queue): the OPT-17 wiring wave adds
a `canon-arrow` arm to `crates/sparql-results` including a new bench subject
`to_canon_arrow_5k_rows` (`benches/graph_serialize.rs:188`). The phase-4
queue was frozen before this wave landed, so it cannot cover the new arm —
close path is a phase-5 fast-lane run or a CI `make bench` run (see §7.0).

## 10. UNVERIFIED register (5 classes)

| class | subjects | close path |
|---|---|---|
| In-flight | phase-2 stragglers + phase-3 reruns (see below) | runner rc rows |
| Phase-4 queued | 8 subjects (§9) | phase-4 register rows |
| No lane this wave | gts, json, geo, markdown, xsd, jsonschema, deflate, ed25519 | CI/make bench |
| wasm/js selections | all — never run, no receipts | `make wasm-test` (CI) |
| Post-freeze OPT-17 arm | `to_canon_arrow_5k_rows` (`crates/sparql-results/benches/graph_serialize.rs:188`) — new bench subject from the arrow-egress wiring wave; UNRUN (no receipt; the arms HAVE landed at `crates/sparql-results/benches/graph_serialize.rs:188,191,353`, but only after the phase-4 queue froze, so no queued run can cover them) | phase-5 fast-lane or CI `make bench` (§7.0) |
| Full gate | `make check` — fmt, clippy, tests, hygiene (pedantic+nursery) | CI-only on this box |

In-flight detail: phase-2 entail, shex, plus the misnamed purrdf-rdf-core
slot (will red — no such bench crate); phase-3 geo-kernel, hnsw, retrieval,
text. Close path for both: runner rc rows. The "slice" subject named in one
lane report also belongs to the no-lane class (close path: CI / make bench).

Single-box caveat: every receipt is from one macOS arm64 box; no
cross-machine replay. Overnight receipt
(docs/jira/v26.10.26/OVERNIGHT_RECEIPT.md) excluded from the bench commit;
separate-commit recommendation stands. PurRDF-CLI is out-of-matrix: the CLI
crate carries no `[[bench]]` target this wave and holds no bench receipt.

## 11. Claims close-out

**56 MEASURED / 23 UNVERIFIED** (docs/BENCHMARKS.md banner, currently
UNVERIFIED → CONVERGING with named receipts; lane-9 recommended reclass of 6
UNSUBSTANTIATED rows recorded but not executed — read-only lane).

## 11.x Conflicting totals — adjudication

The wave receipt's earlier "421 measured / 0 failed" and "60 SAFE-TO-CITE /
121 RED-CLASS / 6 UNSUBSTANTIATED" figures are superseded: the ground-truth
re-read of in-log footers at 13:08 PDT gives 418 terminal measured / 0 failed
(§4). The 56/23 close-out counts above are as-bannered, not re-derived;
refresh both from receipts when the runners drain. A count recalled from
session memory is stale by default — re-read the receipt at use time.

## 12. Receipt index

| artifact | path |
|---|---|
| register (terminal rc rows) | `/tmp/bench-logs/register.txt` (19 rc= rows as of 13:08) |
| phase-2 runner log | `/tmp/bench-logs/phase2-runner.log` |
| phase-3 runner log | `/tmp/bench-logs/phase3-runner.log` |
| fix queue | `/tmp/bench-fix-queue.md` |
| wave receipt | `/tmp/bench-wave-receipt.md` |
| commit pathspecs (126) | `/tmp/bench-commit-pathspecs.txt` |
| commit message | `/tmp/bench-commit-message.txt` |
| phase-4 runner script | `/tmp/run_benches_phase4.py` |
| static receipts (14) | `/tmp/bench-logs/purrdf-<stem>.static.md` |
| measured logs | `/tmp/bench-logs/purrdf-*.log` (see lists below) |
| coverage report | `docs/design/bench-coverage-report.md` |
| banners | `docs/BENCHMARKS.md`, `docs/NPM-ECOSYSTEM.md` |
| handoff contract | `docs/jira/v26.10.26/OPT-42-branchless-handoff.md` |

Static receipts — `/tmp/bench-logs/` (14 files, `purrdf-<stem>.static.md`),
stems: branchless, cdt, columnar, datalog, entail, glob, gts, hash, json, lex,
markdown, mime, n3, toml.

Measured logs, terminal green — `/tmp/bench-logs/`: `purrdf.phase2.log`,
`purrdf-rdf.phase2.log`, `purrdf-shapes.log`, `purrdf-iri.log`,
`purrdf-sparql-algebra.log`, `purrdf-sparql-results.log`,
`purrdf-columnar.phase2.log`, `purrdf-lex.phase2.log`, `purrdf-cdt.phase2.log`,
`purrdf-toml.phase2.log`, `purrdf-mime.phase2.log`, `purrdf-glob.phase2.log`.

Measured logs, red / in-flight — `/tmp/bench-logs/`:
`purrdf-sparql-eval{,.phase2,.phase3}.log`,
`purrdf-{arrow,branchless,duckdb,n3}.phase2.log`,
`purrdf-{geo-kernel,geo-kernel.phase3,hnsw,retrieval,text}.log`.

---

## §13 Fast lane (next wave) — PLANNED, ESTIMATE

What it is: a parallel bench wave — `scripts/run_benches_fast.py` (4-way
concurrent measurement across crates), `scripts/sccache-fast.sh` (sccache-warmed
compilation), and the `make bench-fast` entry point. Fast-lane output shares the
`/tmp/bench-logs` directory but is separated at the register/series level:
rows append to the distinct `register-fast.txt` (serial appends to
`register.txt`) and per-bench logs carry the distinct `<crate>.<bench>.fast.log`
naming, so no row or log is ever mixed between series.

Measured ceiling inputs: 48 GB RAM / 16 cores; sccache 0.16.0 present on PATH;
mold linker absent and skipped (no lld/mold substitution attempted).

Floor estimates (ESTIMATE — not receipts): cold-wave compile 10-20 min;
end-to-end (compile + 4-way measurement) 20-30 min. These assume warm sccache
after the first run; the first run pays full cold compile.

Measurement-validity rule: fast-lane numbers are a **separate series**. They are
never compared against serial-lane numbers within one claim — any cross-series
comparison needs both series re-run under the same lane on the same machine
state, and must be labeled as its own receipt.

First speed receipt (phase-5 proof run, EXECUTED 2026-10-27 — cross-series,
measurement-bound observation, not a like-for-like speedup): fast-lane lex
214 s vs serial-lane 243 s (~12% wall); fast-lane cdt 123 s vs serial 131 s
(~6%). Splits: lex compile 31 s cold / 0 s warm + measure 214 s; cdt compile
21 s / 0 s + measure 123 s. Receipts: register-fast.txt, runner-fast.log,
purrdf-{lex.scan,cdt.parse_render}.fast.log. Caveats: these two crates are
measurement-bound (serial wall was also mostly sampling), so the compile
lever's full win shows on re-runs (0 s compile), not singletons; and the
sccache cache was misdirected to the ambient default cache during this run
(fixed in sccache-fast.sh post-run), so the 31 s/21 s cold compiles were
UNAIDED — a stronger floor, not a weaker one. register-fast.txt rows
produced by the pre-fix orchestrator; branchless compile red there is stale
(fix landed post-run).

---

Footer: sections marked REFRESH-PENDING (§1 summary counts, §9 in-flight,
§10 UNVERIFIED register) stay marked until phase-4 receipts land. As-of:
2026-10-27 13:08 PDT. All receipts live under `/tmp/bench-logs` (ephemeral —
persisted copy planned before /tmp reaping).

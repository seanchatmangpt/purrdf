# [OPT-5S] 5-Second Wall-Clock SLA — Mechanical Enforcement Ladder

## 1. TARGET CONTEXT
- **Crate & File:** `scripts/bounded` (94 lines);
  `.claude/hooks/bounded_build_guard.py` (142 lines);
  `scripts/test-bounded-guard.sh` (108 lines); `Makefile:1026-1066` (b-%/bt-%/
  lane/target-status/target-prune/ice recipes); `scripts/lane-lease.sh` (108
  lines); `scripts/target-hygiene.sh` (233 lines)
- **Subsystem:** Attestation (enforcement infrastructure for all lanes)
- **Inspection Agent:** Lane C14 (ticket-writer), 20-lane enforcement wave
  2026-10-26 (`docs/jira/v26.10.26/_LANES.md`)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** prose-only time discipline. Before this wave a
  build or test command ran unbounded: `cargo test` could hold a lane for
  minutes, orphan a target dir, and starve a 20-lane fan-out. The 5s law
  existed as intent, never as a gate that could exit 1.
- **Hardware-Level Impact:** an unbounded build holds the CPU for the full
  codegen tail (rustc/codegen-units/ld), pins a `CARGO_TARGET_DIR` lease
  (flock), and burns disk through incremental caches — measured this wave as 4
  corrupted `purrdf_sparql_algebra` incremental dirs removed by ice surgery
  (2026-10-26). Contention, not compute, was the dominant cost.
- **Baseline Cost:** unbounded (minutes-class per invocation); wave-wide the
  budget is now 5s per command, <=8s for the full guard corpus.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** law: any build/test >5s wall clock dies
  mechanically, three layers deep.
  1. **Hook (outer layer)** — PreToolUse `bounded_build_guard.py`
     (bounded_build_guard.py:54-118): fail-closed classify. Blocks `cargo`
     (except `fmt --check`/`metadata`), `rustc`, blocked `make` targets
     (`test|check|wasm|pytest|conformance`, bare `make`, unknown targets),
     opaque `.sh`, and any `timeout` lease other than the exact `-k N 5` form.
     Embedded shells (`sh -c`, `eval`, env-prefixed, cd-prefixed) are
     classified as the embedded command; unparseable text fails CLOSED
     (:76). Refusal is typed (`BOUNDED-BUILD-REFUSED`, exit 2) and names the
     corrected `scripts/bounded ...` rewrite (:130-136). A real bypass hole
     was found and patched this session: the earlier revision admitted
     `timeout -k 1 600 cargo test` (wrong-limit lease); now refused —
     corpus MUST-FIRE case at test-bounded-guard.sh:40.
  2. **Wrapper (inner layer)** — `scripts/bounded` (bounded:18): `LIMIT=5`,
     "No argument may extend the limit" (:3). coreutils `timeout -k <grace>
     5` (:49) with a 0.05s SIGKILL grace when the coreutils build supports
     fractional `-k` (:39-41), else 1s; SIGKILL hits the whole process group,
     so backgrounded children (`sleep 6 &`) are reaped (:44-48). Perl
     fork+alarm+setpgrp fallback when no coreutils `timeout` exists (:58-88),
     typed exit 13. Typed stderr line `BOUNDED-TIMEOUT[5s]` on breach, exit
     124 (:51-53). Documented residual: a setsid double-fork escapee survives
     the group kill (:8-13) — the hook is the second layer precisely for that.
  3. **Non-vacuity** — `scripts/test-bounded-guard.sh`: two-directional, 26
     cases (16 MUST-FIRE hook + 7 MUST-PASS hook + 3 wrapper kill/typed-line
     cases), <=8s budget, exit 1 on any vacuous case; a guard that cannot
     fire is the failure (:88-99).
- **Supporting surfaces** (verified on disk):
  - `Makefile:59-75` bounded-preflight: typed exit 1 if the wrapper is
    missing/non-executable; every heavy recipe depends on it.
  - `Makefile:1043-1048` — `bt-%` runs `lane-lease.sh exec` inside
    `scripts/bounded`; `b-%` runs bounded `cargo check` with
    `CARGO_INCREMENTAL=0`. The seconds variables (Makefile:1034-1035) are
    documentation only; bounded accepts no seconds argument.
  - `scripts/lane-lease.sh:36-39` — typed exit 3 `REFUSED ... HELD` on flock
    contention; fail-fast or bounded 50ms-poll `--wait` (:73-82), no
    unbounded queuing. Lease roots live in `/tmp/purrdf-targets/` (:30).
  - `scripts/target-hygiene.sh:29-30` — budget 20GB / prune 14 days;
    deletions only via `rmtree_one()` (python3 shutil.rmtree, one explicit
    dir per call, :38-42); ice surgery deletes only
    `target/debug/incremental/<crate>-*` (:14-16) — used 2026-10-26 to remove
    the 4 corrupted purrdf_sparql_algebra incremental dirs.
- **Hard Bounds Applied:** 5s hard ceiling, not parameterizable; 0.05s kill
  grace (probed, 1s fallback); <=8s guard-corpus budget; 500ms default lease
  wait; 20GB/14d disk budget.
- **Proposed Type Layout & Signatures:**
  - `bounded <command> [args...]` -> rc 0|124|137|13; usage error rc 64
    (bounded:23).
  - `bounded_build_guard.py` stdin JSON -> exit 0 (allow) | exit 2 +
    `{"decision":"block"}` with corrected command.
  - `lane-lease.sh exec [--wait <ms>] <name> -- CMD...` -> rc 0|2|3;
    `list` -> per-lane HELD/idle + size.
  - `target-hygiene.sh status [--fast]|audit|prune [--dry-run]|ice <crate>`.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** the guard corpus is two-directional and
  non-vacuous: `bash scripts/test-bounded-guard.sh` exits 0 with all 26 cases
  PASS — 16 must-fire, 7 must-pass, wrapper kills a 6s payload in the 4.0-7.0s
  window with the typed `BOUNDED-TIMEOUT[5s]: sleep 6` stderr line present
  (test-bounded-guard.sh:88-99).
- **Allocation Invariant:** n/a (infrastructure lane); the disk analog is the
  20GB target budget with HELD-lease-safe deletion and one-directory
  `rmtree_one()` calls.
- **Benchmark Target:** per-command 5s wall clock; guard corpus <=8s;
  lane-lease fail-fast refusal is immediate (typed exit 3 on HELD).
- **Standing honesty (2026-10-26 session):** `purrdf-branchless` compiles —
  37 `From`-in-const errors were found via a bounded census compile and fixed
  to 0 (session receipt; on-disk status UNVERIFIED by this lane). Full
  census/clippy/test runs are >5s and therefore UNVERIFIED on this machine;
  they are CI-bound. Heavy-operation statuses stay UNVERIFIED, never ALIVE,
  until a receipting runner (CI or other box) executes them.
- **Lane ownership for this wave** (`docs/jira/v26.10.26/_LANES.md`):
  L1..L8 branchless kernel files; L9-L12 repair-lane (path_relation,
  solution, seminaive join/factors); L13 geo-kernel cells; L14 ticket-writer
  (status flips); L15-L17 oracle suites; L18 `scripts/bounded` +
  `test-bounded-guard.sh`; L19 `target-hygiene.sh` + `lane-lease.sh`; L20
  `.cargo/config.toml` + Makefile b/bt/lane/target-prune/ice recipes.
  (Dispatch C-numbers map 1:1 onto the _LANES.md L-numbers.)
- **Layer Boundary:** `scripts/check-layers.py` clean (docs-only lane; no
  source edits; no new code homes — all enforcement reuses the L18/L19/L20
  surfaces, one home per job).

See Also: `docs/jira/v26.10.26/_LANES.md` ·
`docs/jira/v26.10.26/optimization/README.md` · `CLAUDE.md` (test ladder)

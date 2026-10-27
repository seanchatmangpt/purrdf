# [OPT-64] Tiered Feedback-Loop SLA — Receipts, Court and Drift Ratchet

## 1. TARGET CONTEXT
- **Crate & File:** `scripts/bounded`; `.claude/hooks/bounded_build_guard.py`;
  `scripts/test-bounded-guard.sh`; `scripts/fb-receipt.sh` + `scripts/fb-receipt.py`;
  `scripts/fb-budgets.json`; `scripts/fb-court.py`; `scripts/fb-ladder.sh`;
  `scripts/lane-lease.sh`; `scripts/target-hygiene.sh`; `scripts/bounded-fmt.sh`;
  `Makefile`; `AGENTS.md` §4; `CLAUDE.md` (This machine: no builds, ever)
- **Subsystem:** the machine verification substrate (the no-builds machine law)
- **Inspection Agent:** the 20-lane wave-1 fan-out as inspection agent (2026-10-26);
  ticket written by lane W11-docs-and-ticket

## 2. MECHANICAL PATHOLOGY
- **perl fallback exits 13, not 124:** `scripts/bounded:7,88,93` — the coreutils
  path exits 124 on breach, the perl fork+alarm fallback exits 13. Same breach, two
  exit codes: any court keying on rc 124 misses the perl-path breach.
- **No typed kill diagnosis:** `scripts/bounded:53,83` emits an untyped
  `BOUNDED-TIMEOUT` stderr string — no structured record of what was killed, which
  tier it belonged to, or how close to the ceiling the probe ran.
- **No receipt log yet:** `scripts/fb-receipts.jsonl` does not exist until the first
  `fb-ladder.sh` run appends it (court contract: missing log -> exit 2;
  `scripts/fb-court.py:9,35`).
- **lane-lease lacks warm/clean:** `scripts/lane-lease.sh` has exactly two
  subcommands (`cmd_exec` :56, `cmd_list` :101); warming or cleaning a lease requires
  ad hoc cargo invocations the wrapper cannot see or bound.
- **Hygiene is unbounded du plus per-eviction re-walks over ~15 GB:**
  `scripts/target-hygiene.sh` re-walks the target tree per eviction decision;
  `target/debug/incremental` alone is ~15 GB (CLAUDE.md standing rule).
- **bounded-fmt python timing overhead:** `scripts/bounded-fmt.sh:52,58-59` spawns
  three `python3 -c` processes per batch just to compute elapsed time.
- **Unbounded waits in support scripts:** `scripts/simd_asm_runtime.py` and
  `scripts/scale-corpus.sh` run waits outside any bounded lease at their call sites
  (wave-1 finding; both files verified present).
- **Bare-cargo Makefile targets:** `Makefile:317`
  (`cargo clippy -p purrdf-gts ...`) and `Makefile:326`
  (`cargo test -p purrdf-gts --test bounded_keyed_blobs --locked`) invoke cargo
  directly, outside `scripts/bounded`.
- **AGENTS.md was silent on the machine constraint:** before this wave the
  behavioral contract carried no machine-law section — the no-builds law lived only
  in CLAUDE.md. (Closed by this ticket's AGENTS.md §4.)
- **2 red oracle tests:** wave-1 reported 2 failing cases in
  `crates/branchless/oracles` (as reported by the wave; reproduce with
  `python3 -m pytest crates/branchless/oracles` before filing a defect — this lane
  did not execute pytest). Vector corpus measured 2026-10-26: 7,313 top-level
  entries across the 6 frozen files (incl. `manifest.json`'s 2).

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Tier table, normative ceilings** (downward-only ratchet
  `scripts/fb-budgets.json`): t1 static 0.5s / t2 oracle 0.8s / t3 leaf 1.5s warm /
  ladder total 2.5s; p95 over window 5, alarm at 3 breaches; hard law 5s
  single-probe via `scripts/bounded`.
- **FB grammar, one home:** `scripts/fb-receipt.sh` (`FB <tier> <name> <count> <ms>
  <status>`; rule:fb-receipt-format; sole sanctioned variant `scripts/fb-receipt.py`
  for Python gates). No other file emits an `FB ` line — a second body is a census
  violation.
- **JSONL court + downward-only ratchet:** `scripts/fb-court.py` appends one receipt
  per ladder run (PASS and FAIL — red runs are data) to `scripts/fb-receipts.jsonl`
  and checks the tail against `scripts/fb-budgets.json`; fail-closed (missing/torn
  -> exit 2; fewer than window receipts -> exit 1 warming-up; drift -> exit 1 +
  ALARM receipt).
- **Drift subcommand:** `fb-court.py check` runs the drift alarm over the window;
  loosening a budget is a reviewed diff to `fb-budgets.json`, never a local knob.
- **CI mirror:** `.github/workflows/bounded-gates.yml` runs the same ladder so the
  warming-up court warms in CI; colder stages stay CI-only
  (`PURRDF_BOUNDED_LOCAL=0`, owned by CI).
- **jobs=2 leases:** concurrent lanes share leases through `scripts/lane-lease.sh`
  (flock, `/tmp/purrdf-targets`); `warm`/`clean` subcommands make lease lifecycle
  inspectable without ad hoc cargo.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Corpus non-vacuity:** the neutered-guard mutant in `scripts/test-bounded-guard.sh`
  must make the gate refuse — a court that accepts a guard-less shell is vacuous and
  exits 1 as `VACUOUS GATE`-class failure.
- **Kill rc 124:** a coreutils-path breach exits 124 with the typed
  `BOUNDED-TIMEOUT[5s]` line; the perl fallback is normalized to 124 (same breach,
  same rc) or removed.
- **Receipt-count UNKNOWN invariant:** a rung emitting no receipt makes the ladder
  verdict UNKNOWN (fail-closed on silent skip), never a pass.
- **Court fail-closed exit 2** on missing/empty/torn log (`scripts/fb-court.py`).
- **Warm-up fails until 5 receipts:** fewer than window receipts exits 1 with the
  loud warming-up note; CI warms the court.
- **Budgets:** 0.5/0.8/1.5s per tier, 2.5s ladder total, p95 window 5, alarm at 3
  breaches (`scripts/fb-budgets.json`, downward-only).
- **Standing honesty:** Rust execution gates stay UNVERIFIED on this machine
  (no-builds law) until the remote CI run receipts them; local truth is the static
  gates + oracles + receipts, never "tests passed" by assertion.
- **Layer Boundary:** `python3 scripts/check-layers.py` stays green; no new code
  homes outside the sanctioned `scripts/fb-receipt.sh`/`fb-receipt.py` pair
  (helpers-ledger rule:fb-receipt-format).

See Also: `docs/jira/v26.10.26/OPT-5s-bounded-ceiling.md` ·
`docs/jira/v26.10.26/optimization/OPT-24-cold-path-and-noaction-register.md` ·
`docs/jira/v26.10.26/optimization/README.md` · `AGENTS.md` §4 · `CLAUDE.md`

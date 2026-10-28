#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""fb-court — the feedback-SLA regression court.

Owns scripts/fb-receipts.jsonl: one JSONL receipt per ladder run, appended on
PASS *and* FAIL (red runs are data — the drift alarm needs them). Checks the
downward-only budgets in scripts/fb-budgets.json over the tail of the log.

Fail-closed contract:
  * missing/empty log            -> exit 2 (court cannot see history)
  * torn/unparseable line        -> exit 2, line number quoted (tamper/crash
                                    evidence, never skipped)
  * fewer than `window` receipts -> exit 1 with a loud warming-up note
                                    (a permanently-warming court proves
                                    nothing; CI warms it)
  * sustained drift              -> exit 1, ALARM receipt appended

Subcommands:
  append --tier t1|t2|t3|total --metric <seconds> --verdict PASS|FAIL
  check
  init        (explicit, reviewable recovery for a missing log)
  --self-test (runs every path against a temp copy; exit 1 if any is wrong)
"""

from __future__ import annotations

import argparse
import fcntl
import json
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LOG = REPO / "scripts" / "fb-receipts.jsonl"
BUDGETS = REPO / "scripts" / "fb-budgets.json"
TIERS = ("t1", "t2", "t3", "total")


def _load_budgets() -> dict:
    with open(BUDGETS, encoding="utf-8") as fh:
        return json.load(fh)


def _read_receipts(path: Path) -> list[dict]:
    if not path.exists():
        _die_missing(path)
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines:
        _die_missing(path)
    receipts = []
    for i, line in enumerate(lines, start=1):
        if not line.strip():
            sys.stderr.write(
                f"fb: ERROR: torn receipt at {path}:{i} (empty line) — "
                "evidence of crash/tamper, refusing to pass silently\n"
            )
            raise SystemExit(2)
        try:
            rec = json.loads(line)
        except json.JSONDecodeError as exc:
            sys.stderr.write(
                f"fb: ERROR: torn receipt at {path}:{i} ({exc}) — "
                "refusing to pass silently\n"
            )
            raise SystemExit(2) from exc
        receipts.append(rec)
    return receipts


def _die_missing(path: Path) -> None:
    sys.stderr.write(
        f"fb: ERROR: receipt log {path} is missing or empty — the regression "
        "court cannot see history; refusing to pass silently (exit 2). "
        "Recovery: python3 scripts/fb-court.py --init (reviewable diff).\n"
    )
    raise SystemExit(2)


def cmd_append(tier: str, metric: float, verdict: str) -> int:
    receipt = {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "tier": tier,
        "metric_s": round(float(metric), 4),
        "verdict": verdict,
    }
    line = json.dumps(receipt, separators=(",", ":"))
    lock = Path(str(LOG) + ".lock")
    with open(lock, "w") as lock_fh:
        fcntl.flock(lock_fh, fcntl.LOCK_EX)
        with open(LOG, "a", encoding="utf-8") as fh:
            fh.write(line + "\n")
    _trim()
    print(f"fb: receipt appended tier={tier} metric_s={receipt['metric_s']} verdict={verdict}")
    return 0


def _trim(keep_per_tier: int = 200, rewrite_at: int = 400) -> None:
    """Amortized trim: rewrite only when a tier exceeds rewrite_at lines."""
    receipts = _read_receipts(LOG)
    counts: dict[str, int] = {}
    for rec in receipts:
        counts[rec.get("tier", "?")] = counts.get(rec.get("tier", "?"), 0) + 1
    if not any(c > rewrite_at for c in counts.values()):
        return
    kept: dict[str, list[str]] = {}
    out = []
    for line, rec in zip(
        LOG.read_text(encoding="utf-8").splitlines(), receipts
    ):
        tier = rec.get("tier", "?")
        kept.setdefault(tier, []).append(line)
    for tier, lines in kept.items():
        out.extend(lines[-keep_per_tier:])
    tmp = LOG.with_suffix(".jsonl.tmp")
    tmp.write_text("\n".join(out) + "\n", encoding="utf-8")
    tmp.replace(LOG)


def cmd_check(log_path: Path | None = None) -> int:
    path = log_path or LOG
    budgets = _load_budgets()
    receipts = _read_receipts(path)
    alarms = 0
    warmed_up = True
    # Receipt tiers are t1/t2/t3; budget keys are tier-1/tier-2/tier-3/total.
    budget_key = {"t1": "tier-1", "t2": "tier-2", "t3": "tier-3", "total": "total"}
    for tier in TIERS:
        cfg = budgets.get(budget_key[tier])
        if cfg is None:
            continue
        window, allowed = int(cfg["window"]), int(cfg["breaches"])
        budget = float(cfg["budget_s"])
        tail = [r for r in receipts if r.get("tier") == tier][-window:]
        if len(tail) < window:
            warmed_up = False
            print(
                f"fb: NOTE: only {len(tail)} of {window} receipts for "
                f"{tier} — court warming up ({len(tail)}/{window})"
            )
            continue
        breaches = sum(1 for r in tail if float(r["metric_s"]) > budget)
        p95 = sorted(float(r["metric_s"]) for r in tail)[int(0.95 * (len(tail) - 1))]
        if breaches >= allowed:
            alarms += 1
            print(
                f"fb: DRIFT ALARM: {tier} p95 over last {window} runs = "
                f"{p95}s ({breaches}/{window} above budget {budget}s) — "
                f"see {path}"
            )
            _append_alarm(tier, p95, log_path=path)
        else:
            print(f"fb: {tier} p95={p95}s budget={budget}s breaches={breaches}/{window} PASS")
    if alarms:
        return 1
    if not warmed_up:
        return 1
    print("fb: court PASS (all tiers warm, no drift alarm)")
    return 0


def _append_alarm(tier: str, p95: float, log_path: Path) -> None:
    receipt = {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "tier": tier,
        "metric_s": round(p95, 4),
        "verdict": "ALARM",
    }
    lock = Path(str(log_path) + ".lock")
    with open(lock, "w") as lock_fh:
        fcntl.flock(lock_fh, fcntl.LOCK_EX)
        with open(log_path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(receipt, separators=(",", ":")) + "\n")


def cmd_init() -> int:
    if LOG.exists() and LOG.stat().st_size > 0:
        print(f"fb: --init refused: {LOG} already has content (reviewable recovery only)")
        return 1
    LOG.parent.mkdir(parents=True, exist_ok=True)
    LOG.write_text("", encoding="utf-8")
    print(f"fb: initialized empty receipt log {LOG}")
    return 0


def cmd_self_test() -> int:
    """Exercise every court path on a temp copy. Fail closed if any is wrong."""
    failures = []

    def green(t3: float = 1.0) -> dict[str, list[float]]:
        return {"t1": [0.4] * 5, "t2": [0.6] * 5, "t3": [t3] * 5, "total": [2.0] * 5}

    def seed_all(path: Path, tiers: dict[str, list[float]]) -> None:
        with open(path, "a", encoding="utf-8") as fh:
            for tier, metrics in tiers.items():
                for m in metrics:
                    fh.write(
                        json.dumps(
                            {"ts": "seed", "tier": tier, "metric_s": m, "verdict": "PASS"},
                            separators=(",", ":"),
                        )
                        + "\n"
                    )

    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp) / "receipts.jsonl"

        def expect(name: str, log: Path, want_rc: int) -> None:
            rc = _run_with_log(["check"], log)
            tag = "PASS" if rc == want_rc else "FAIL"
            print(f"SELF-TEST[{tag}]: {name} rc={rc} want={want_rc}")
            if rc != want_rc:
                failures.append(name)

        # Missing log -> 2
        expect("missing-log-fails-closed", tmp_path, 2)
        tmp_path.write_text("", encoding="utf-8")
        # Empty log -> 2
        expect("empty-log-fails-closed", tmp_path, 2)
        # Warm-up: 3 of 5 receipts for every tier -> 1 (loud, never silent pass)
        partial = {t: ms[:3] for t, ms in green().items()}
        seed_all(tmp_path, partial)
        expect("warming-up-is-not-pass", tmp_path, 1)
        # Warm + green: full ladder under budget -> 0
        tmp_green = tmp_path.parent / "green.jsonl"
        seed_all(tmp_green, green())
        expect("warm-green-passes", tmp_green, 0)
        # Drift: 3 of last 5 t3 above budget -> 1 + ALARM receipt appended
        drift = green()
        drift["t3"] = [1.0, 1.0, 1.6, 1.6, 1.6]
        tmp_drift = tmp_path.parent / "drift.jsonl"
        seed_all(tmp_drift, drift)
        before = len(tmp_drift.read_text().splitlines())
        expect("drift-alarm-fires", tmp_drift, 1)
        after_lines = tmp_drift.read_text().splitlines()
        if len(after_lines) == before + 1 and '"verdict":"ALARM"' in after_lines[-1]:
            print("SELF-TEST[PASS]: alarm-receipt-appended")
        else:
            print("SELF-TEST[FAIL]: alarm-receipt-appended")
            failures.append("alarm-receipt-appended")
        # Single jitter must NOT alarm (1 of 5 above budget)
        jitter = green()
        jitter["t3"] = [1.0, 1.0, 1.0, 1.0, 1.6]
        tmp_jitter = tmp_path.parent / "jitter.jsonl"
        seed_all(tmp_jitter, jitter)
        expect("single-jitter-does-not-alarm", tmp_jitter, 0)
        # Torn line -> 2 with line number
        tmp_torn = tmp_path.parent / "torn.jsonl"
        seed_all(tmp_torn, green())
        with open(tmp_torn, "a", encoding="utf-8") as fh:
            fh.write("{not json\n")
        expect("torn-line-fails-closed", tmp_torn, 2)

    if failures:
        print(f"SELF-TEST RESULT: FAIL ({len(failures)}): {failures}")
        return 1
    print("SELF-TEST RESULT: PASS")
    return 0


def _run_with_log(argv: list[str], log_path: Path) -> int:
    """Run check/append against a temp log (monkey-free: explicit arg)."""
    if argv and argv[0] == "check":
        try:
            return cmd_check(log_path=log_path)
        except SystemExit as exc:
            return int(exc.code or 0)
    return 64


def main() -> int:
    parser = argparse.ArgumentParser(description="feedback-SLA regression court")
    parser.add_argument("--self-test", action="store_true")
    sub = parser.add_subparsers(dest="cmd")
    p_append = sub.add_parser("append")
    p_append.add_argument("--tier", required=True, choices=TIERS)
    p_append.add_argument("--metric", type=float, required=True)
    p_append.add_argument("--verdict", default="PASS", choices=["PASS", "FAIL", "ALARM"])
    sub.add_parser("check")
    sub.add_parser("init")
    args = parser.parse_args()

    if args.self_test:
        return cmd_self_test()
    if args.cmd == "append":
        return cmd_append(args.tier, args.metric, args.verdict)
    if args.cmd == "check":
        return cmd_check()
    if args.cmd == "init":
        return cmd_init()
    parser.print_help()
    return 64


if __name__ == "__main__":
    raise SystemExit(main())

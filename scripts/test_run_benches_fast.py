#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Shim-court tests for run_benches_fast.py (stdlib unittest, no mocks).

Real collaborators: real fake bench binaries (/bin/sh scripts) executed by
the real measure_phase worker pool through the real spawn / drain /
register_row path. The module's LOGS/REGISTER globals are pointed at a
scratch directory — configuration injection, not a mock: the register rows
asserted on are written by the real code under test through its real
flock/O_APPEND path. Restores every touched file byte-exact (sha256
before/after) and asserts /tmp lease + store surfaces are absent after the
court; a court that cannot restore is FAIL.
"""
from __future__ import annotations

import argparse
import glob
import hashlib
import importlib.util
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

_HERE = os.path.dirname(os.path.abspath(__file__))
_SPEC = importlib.util.spec_from_file_location(
    "run_benches_fast", os.path.join(_HERE, "run_benches_fast.py"))
rbf = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(rbf)

GREEN_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "1 benchmarks measured; 0 failed"
exit 0
"""

RC101_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
exit 101
"""

POISON_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "POISON: a resume-skipped bench must never be spawned" >&2
exit 101
"""

SLEEPY_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
sleep 4
echo "1 benchmarks measured; 0 failed"
exit 0
"""

# rc=137 SIGKILL simulator; appends start/end events to $CONCUR so the
# concurrency window of each invocation is observable.
RC137_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "a start $(date +%s.%N)" >> "$CONCUR"
sleep 1
echo "a end $(date +%s.%N)" >> "$CONCUR"
exit 137
"""

# true signal death: SIGKILLs itself, so the subprocess returncode is the
# POSIX -9 form, not a 137 shell exit (G1 classification).
SIGKILL_SELF_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "s start $(date +%s.%N)" >> "$CONCUR"
kill -9 $$
echo "s end $(date +%s.%N)" >> "$CONCUR"
"""

# dies -9 on the first run only (marker file), so the solo retry goes green
# — the G4 observable: a killed attempt retried green must exit 0.
DIE_ONCE_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
if [ -f "$DIE_ONCE_MARKER" ]; then
  echo "1 benchmarks measured; 0 failed"
  exit 0
fi
touch "$DIE_ONCE_MARKER"
kill -9 $$
"""

SLOW_GREEN_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "b start $(date +%s.%N)" >> "$CONCUR"
sleep 3
echo "b end $(date +%s.%N)" >> "$CONCUR"
echo "1 benchmarks measured; 0 failed"
exit 0
"""

FAST_GREEN_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "c start $(date +%s.%N)" >> "$CONCUR"
echo "c end $(date +%s.%N)" >> "$CONCUR"
echo "1 benchmarks measured; 0 failed"
exit 0
"""

LEASE_PROBE_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "lease=$CARGO_TARGET_DIR" >> "$CONCUR"
echo "1 benchmarks measured; 0 failed"
exit 0
"""

# G5/G12 probe: records its own pid and its full argv.
ARGV_PID_PROBE_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
echo "argv $*" >> "$CONCUR"
echo "pid $$" >> "$CONCUR"
echo "1 benchmarks measured; 0 failed"
exit 0
"""

MIN_MANIFEST = """[package]
name = "fakecrate"
version = "0.0.0"
edition = "2021"

[[bench]]
name = "fb"
harness = false
"""


def sha256(path: str) -> str:
    with open(path, "rb") as fh:
        return hashlib.sha256(fh.read()).hexdigest()


def write_bench(scratch: str, name: str, body: str) -> str:
    binary = os.path.join(scratch, name)
    with open(binary, "w") as fh:
        fh.write(body)
    os.chmod(binary, 0o755)
    return binary


class BenchFastShimCourt(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.mkdtemp(prefix="purrdf-bench-fast-court-")
        self.old_logs, self.old_register = rbf.LOGS, rbf.REGISTER
        self.old_runner_log = rbf.RUNNER_LOG
        self.old_cap = rbf.CAP
        self.old_max_workers = rbf.MAX_WORKERS
        self.old_crates_root = rbf.CRATES_ROOT
        self.old_deps_dir = rbf.DEPS_DIR
        self.old_lease_root = rbf.LEASE_ROOT
        rbf.LOGS = self.scratch
        rbf.REGISTER = os.path.join(self.scratch, "register-fast.txt")
        rbf.RUNNER_LOG = os.path.join(self.scratch, "runner-fast.log")
        # point the lease/probe root into the scratch dir: the court must be
        # hermetic against other lanes' real runs sharing /tmp on this box
        rbf.LEASE_ROOT = os.path.join(self.scratch, "leases")
        os.makedirs(rbf.LEASE_ROOT, exist_ok=True)
        os.makedirs(rbf.LOGS, exist_ok=True)

    def tearDown(self) -> None:
        rbf.LOGS, rbf.REGISTER = self.old_logs, self.old_register
        rbf.RUNNER_LOG = self.old_runner_log
        rbf.CAP = self.old_cap
        rbf.MAX_WORKERS = self.old_max_workers
        rbf.CRATES_ROOT = self.old_crates_root
        rbf.DEPS_DIR = self.old_deps_dir
        rbf.LEASE_ROOT = self.old_lease_root
        shutil.rmtree(self.scratch, ignore_errors=True)

    def _register_text(self) -> str:
        if not os.path.exists(rbf.REGISTER):
            return ""
        with open(rbf.REGISTER) as fh:
            return fh.read()

    def _register_lines(self, bench: str) -> list[str]:
        return [ln for ln in self._register_text().splitlines()
                if f"BENCH={bench} " in ln]

    # 1. GREEN PATH: real rc=0 row through real spawn/drain/register_row.
    def test_green_bench_yields_rc0_row_and_exit_zero(self) -> None:
        binary = write_bench(self.scratch, "green-bench", GREEN_BENCH)
        rc = rbf.measure_phase("fakecrate", [("bg", binary)], resume=False)
        self.assertEqual(rc, 0, "a green bench must make measure_phase return 0")
        lines = self._register_lines("bg")
        self.assertEqual(len(lines), 1, "exactly one register row for bg")
        self.assertIn("fakecrate BENCH=bg rc=0", lines[0])
        log_path = os.path.join(self.scratch, "fakecrate.bg.fast.log")
        with open(log_path) as fh:
            self.assertIn("1 benchmarks measured; 0 failed", fh.read(),
                          "bench stdout must land in the real log file")

    # 2a. resume: half-green crate — only the non-green bench re-runs.
    def test_resume_runs_only_non_green_bench(self) -> None:
        with open(rbf.REGISTER, "w") as fh:
            fh.write("fakecrate BENCH=a rc=0 SECS=1 log=seeded\n")
        poison = write_bench(self.scratch, "poison-a", POISON_BENCH)
        green = write_bench(self.scratch, "green-b", GREEN_BENCH)
        rc = rbf.measure_phase(
            "fakecrate", [("a", poison), ("b", green)], resume=True)
        self.assertEqual(rc, 0)
        lines_a = self._register_lines("a")
        self.assertEqual(len(lines_a), 1, "bench a must NOT re-spawn under resume")
        self.assertIn("log=seeded", lines_a[0], "a's row must be the seeded one")
        lines_b = self._register_lines("b")
        self.assertEqual(len(lines_b), 1, "bench b must run and register a row")
        self.assertIn("rc=0", lines_b[0])

    # 2b. resume: all-green crate — nothing spawns at all.
    def test_resume_all_green_spawns_nothing(self) -> None:
        with open(rbf.REGISTER, "w") as fh:
            fh.write("fakecrate BENCH=a rc=0 SECS=1 log=x\n"
                     "fakecrate BENCH=b rc=0 SECS=1 log=y\n")
        poison_a = write_bench(self.scratch, "poison-a2", POISON_BENCH)
        poison_b = write_bench(self.scratch, "poison-b2", POISON_BENCH)
        seed = ("fakecrate BENCH=a rc=0 SECS=1 log=x\n"
                "fakecrate BENCH=b rc=0 SECS=1 log=y\n")
        rc = rbf.measure_phase(
            "fakecrate", [("a", poison_a), ("b", poison_b)], resume=True)
        self.assertEqual(rc, 0)
        self.assertNotIn("POISON", self._register_text(),
                         "no bench may spawn when every row is already green")
        self.assertEqual(self._register_text(), seed,
                         "all-green resume must append zero new rows")

    # 3. rc=137: two rows (killed attempt + retried row) and the solo gate.
    def test_rc137_records_two_rows_and_retries_solo(self) -> None:
        concur = os.path.join(self.scratch, "concur.log")
        os.environ["CONCUR"] = concur
        rbf.MAX_WORKERS = 2
        try:
            b137 = write_bench(self.scratch, "bench-137", RC137_BENCH)
            slow = write_bench(self.scratch, "bench-slow", SLOW_GREEN_BENCH)
            fast = write_bench(self.scratch, "bench-fast", FAST_GREEN_BENCH)
            # queue order: a(137) first; b holds a worker 3s so `c` is still
            # queued when a dies — the solo gate must hold c (and the retry)
            # until the pool has drained to EMPTY.
            rc = rbf.measure_phase(
                "fakecrate", [("a", b137), ("b", slow), ("c", fast)],
                resume=False)
        finally:
            os.environ.pop("CONCUR", None)
        self.assertEqual(rc, 1, "a bench that dies 137 twice is red")
        lines_a = self._register_lines("a")
        self.assertEqual(len(lines_a), 2, "rc=137 must record two rows for a")
        self.assertIn("rc=137", lines_a[0])
        self.assertNotIn("retry=1", lines_a[0], "first attempt is unmarked")
        self.assertIn("rc=137", lines_a[1])
        self.assertIn("retry=1", lines_a[1], "second row must be retry-marked")
        # solo-gating observable: parse the concurrency log; the SECOND
        # `a` invocation is the retry. No fresh spawn (`c`) may start inside
        # the retry's [start, end] window.
        with open(concur) as fh:
            events = [ln.split() for ln in fh.read().splitlines() if ln.strip()]
        a_starts = [float(t) for kind, ev, t in events
                    if kind == "a" and ev == "start"]
        self.assertEqual(len(a_starts), 2, "a must run exactly twice")
        retry_start = a_starts[1]
        a_ends = [float(t) for kind, ev, t in events if kind == "a" and ev == "end"]
        retry_end = a_ends[1]
        c_starts = [float(t) for kind, ev, t in events
                    if kind == "c" and ev == "start"]
        self.assertEqual(len(c_starts), 1)
        self.assertGreaterEqual(
            c_starts[0], retry_end,
            "bench c must not spawn while the rc=137 retry is in flight")
        # G2 strengthened: the retry must overlap NO other bench window at
        # all — the pool is drained to empty before it spawns.
        for kind in ("b", "c"):
            starts = [float(t) for k, ev, t in events
                      if k == kind and ev == "start"]
            ends = [float(t) for k, ev, t in events
                    if k == kind and ev == "end"]
            for s, e in zip(starts, ends):
                overlaps = not (retry_start >= e or retry_end <= s)
                self.assertFalse(
                    overlaps,
                    f"retry [{retry_start}, {retry_end}] overlaps bench "
                    f"{kind} window [{s}, {e}]; retry is not solo")

    # 3b. G1+G4: a true signal death (raw returncode -9) classifies as
    #     killed — normalized to a rc=137 row and retried once; a green
    #     retry makes the whole phase green (exit 0).
    def test_sigkill_raw_minus9_classifies_as_137_and_green_retry_is_green(self) -> None:
        marker = os.path.join(self.scratch, "die-once.marker")
        os.environ["DIE_ONCE_MARKER"] = marker
        concur = os.path.join(self.scratch, "concur.log")
        os.environ["CONCUR"] = concur
        try:
            die_once = write_bench(self.scratch, "bench-die-once", DIE_ONCE_BENCH)
            rc = rbf.measure_phase("fakecrate", [("s", die_once)], resume=False)
        finally:
            os.environ.pop("CONCUR", None)
            os.environ.pop("DIE_ONCE_MARKER", None)
        self.assertEqual(rc, 0, "a -9 death retried green must make the phase green (G4)")
        lines = self._register_lines("s")
        self.assertEqual(len(lines), 2, "killed attempt + retried row")
        self.assertIn("rc=137", lines[0],
                      f"raw -9 must normalize to a rc=137 row; got {lines[0]!r}")
        self.assertIn("rc=0", lines[1])
        self.assertIn("retry=1", lines[1])
        self.assertNotIn("rc=-9", self._register_text(),
                         "raw negative signal rc must never reach the register")

    # 3b'. a bench that dies -9 on EVERY run (retry included) stays red.
    def test_sigkill_every_run_stays_red(self) -> None:
        killer = write_bench(self.scratch, "bench-kill-always", SIGKILL_SELF_BENCH)
        rc = rbf.measure_phase("fakecrate", [("k", killer)], resume=False)
        self.assertEqual(rc, 1, "a bench killed -9 twice is red")
        lines = self._register_lines("k")
        self.assertEqual(len(lines), 2)
        self.assertIn("rc=137", lines[0])
        self.assertIn("rc=137", lines[1])
        self.assertIn("retry=1", lines[1])

    # 3c. G3: _normalize_rc contract — rc is never None, CAP kill -> 124,
    #     forced kill -> 137, signal deaths normalize to 137.
    def test_normalize_rc_never_none(self) -> None:
        cases = [
            (None, False, 137),   # forced kill, wait expired
            (None, True, 124),    # CAP kill, wait expired
            (-9, False, 137),     # raw POSIX signal death
            (-15, False, -15),    # non-SIGKILL signals pass through raw
            (137, False, 137),    # shell-relayed SIGKILL
            (0, False, 0),
            (101, False, 101),
            (124, True, 124),     # CAP kill records 124 even at rc 124
            (137, True, 124),     # CAP kill wins over the 137 classification
        ]
        for raw, cap, want in cases:
            self.assertEqual(rbf._normalize_rc(raw, cap), want,
                             f"_normalize_rc({raw!r}, cap={cap}) != {want}")
        for raw in (None, -9, -15, 0, 1, 101, 124, 137):
            for cap in (False, True):
                self.assertIsInstance(rbf._normalize_rc(raw, cap), int,
                                      f"rc={raw!r} cap={cap}: never None (G3)")
        # the register itself can never carry an rc=None row
        self.assertNotIn("rc=None", self._register_text())

    # 4. CAP: a run exceeding the (injected) cap is killed and recorded.
    def test_cap_kill_records_row(self) -> None:
        rbf.CAP = 1
        binary = write_bench(self.scratch, "sleepy-bench", SLEEPY_BENCH)
        rc = rbf.measure_phase("fakecrate", [("capbench", binary)], resume=False)
        self.assertNotEqual(rc, 0, "a CAP-killed bench is not green")
        lines = self._register_lines("capbench")
        self.assertEqual(len(lines), 1, "CAP kill records exactly one row")
        # Documented contract (module docstring): "killed -> rc=124 row".
        self.assertIn("rc=124", lines[0],
                      f"CAP kill must record rc=124 per the docstring contract; "
                      f"got row: {lines[0]!r}")

    # 5. classify_rc101: footer -> failed, `error:` -> refused.
    def test_classify_rc101_footer_vs_error(self) -> None:
        footer_log = os.path.join(self.scratch, "footer.log")
        with open(footer_log, "w") as fh:
            fh.write("running...\n2 benchmarks measured; 1 failed\n")
        error_log = os.path.join(self.scratch, "error.log")
        with open(error_log, "w") as fh:
            fh.write("error: harness=true unsupported\n")
        self.assertEqual(rbf.classify_rc101("c", "b", footer_log), "failed")
        self.assertEqual(rbf.classify_rc101("c", "b", error_log), "refused")

    # 6. lease law: leases removed after measure_phase; a live foreign
    #    occupant is not evicted (free_lease skips the occupied slot).
    def test_leases_removed_after_phase_and_occupied_lease_skipped(self) -> None:
        binary = write_bench(self.scratch, "lease-probe", LEASE_PROBE_BENCH)
        # part 1: after a normal phase, no lease dir naming our pid remains
        rc = rbf.measure_phase("fakecrate", [("l1", binary)], resume=False)
        self.assertEqual(rc, 0)
        for i in range(1, rbf.MAX_WORKERS + 1):
            lease = os.path.join(rbf.LEASE_ROOT, f"purrdf-fast-t{i}")
            pid_file = os.path.join(lease, "owner.pid")
            if os.path.exists(pid_file):
                with open(pid_file) as fh:
                    self.assertNotEqual(
                        fh.read().strip(), str(os.getpid()),
                        f"{lease} was not removed after measure_phase")
        # part 2: a lease held by a LIVE foreign pid is occupied -> skipped
        foreign = subprocess.Popen(["/bin/sleep", "30"])
        lease1 = os.path.join(rbf.LEASE_ROOT, "purrdf-fast-t1")
        os.makedirs(lease1, exist_ok=True)
        with open(os.path.join(lease1, "owner.pid"), "w") as fh:
            fh.write(f"{foreign.pid}\n")
        concur = os.path.join(self.scratch, "lease.log")
        os.environ["CONCUR"] = concur
        try:
            rc = rbf.measure_phase("fakecrate", [("l2", binary)], resume=False)
        finally:
            os.environ.pop("CONCUR", None)
            foreign.kill()
            foreign.wait()
            shutil.rmtree(lease1, ignore_errors=True)
        self.assertEqual(rc, 0)
        with open(concur) as fh:
            text = fh.read()
        self.assertIn("lease=" + os.path.join(rbf.LEASE_ROOT, "purrdf-fast-t2"),
                      text,
                      f"occupied t1 must be skipped; worker saw: {text!r}")

    # 7. G5: owner.pid records the WORKER pid, and the occupancy probe
    #    free/skips against it (an owner.pid naming the orchestrator would
    #    make every lease look occupied forever).
    def test_owner_pid_records_worker_pid(self) -> None:
        concur = os.path.join(self.scratch, "pid.log")
        os.environ["CONCUR"] = concur
        try:
            binary = write_bench(self.scratch, "pid-probe", ARGV_PID_PROBE_BENCH)
            rc = rbf.measure_phase("fakecrate", [("p1", binary)], resume=False)
        finally:
            os.environ.pop("CONCUR", None)
        self.assertEqual(rc, 0)
        with open(concur) as fh:
            text = fh.read()
        worker_pid = int(next(ln.split()[1] for ln in text.splitlines()
                              if ln.startswith("pid ")))
        self.assertNotEqual(worker_pid, os.getpid(),
                            "the bench must have run in a child process")
        # occupancy probe semantics over owner.pid: an exited worker pid
        # reads FREE, a live pid reads OCCUPIED (worker-pid driven, G5).
        lease = os.path.join(rbf.LEASE_ROOT, "purrdf-fast-t1")
        os.makedirs(lease, exist_ok=True)
        try:
            with open(os.path.join(lease, "owner.pid"), "w") as fh:
                fh.write(f"{worker_pid}\n")
            self.assertFalse(rbf._lease_occupied(lease),
                             "an exited worker pid must read as a free lease")
            live = subprocess.Popen(["/bin/sleep", "30"])
            try:
                with open(os.path.join(lease, "owner.pid"), "w") as fh:
                    fh.write(f"{live.pid}\n")
                self.assertTrue(rbf._lease_occupied(lease),
                                "a live worker pid must read as occupied")
            finally:
                live.kill()
                live.wait()
        finally:
            shutil.rmtree(lease, ignore_errors=True)

    # 8. G12: passthrough flags reach each spawned binary after --bench.
    def test_bench_flag_passthrough_reaches_spawned_argv(self) -> None:
        concur = os.path.join(self.scratch, "argv.log")
        os.environ["CONCUR"] = concur
        binary = write_bench(self.scratch, "argv-probe", ARGV_PID_PROBE_BENCH)
        try:
            rc = rbf.measure_phase(
                "fakecrate", [("ap", binary)], resume=False,
                bench_args=["--quick", "--exact", "--filter", "foo",
                            "--filter", "bar", "--baseline", "nightly"])
        finally:
            os.environ.pop("CONCUR", None)
        self.assertEqual(rc, 0)
        with open(concur) as fh:
            text = fh.read()
        self.assertIn("--bench --quick --exact --filter foo --filter bar "
                      "--baseline nightly", text,
                      f"passthrough flags must follow --bench in the argv; "
                      f"got: {text!r}")
        # default (no flags) behavior stays identical: bare `--bench` only
        concur2 = os.path.join(self.scratch, "argv2.log")
        os.environ["CONCUR"] = concur2
        try:
            rc = rbf.measure_phase("fakecrate", [("ap", binary)], resume=False)
        finally:
            os.environ.pop("CONCUR", None)
        self.assertEqual(rc, 0)
        with open(concur2) as fh:
            self.assertIn("--bench\n", fh.read(),
                          "flagless invocation must pass exactly --bench")

    # 8b. G12: the baseline group is exclusive and validated; the CLI
    #     parser forwards and _bench_forward_args validates direct callers.
    def test_baseline_flags_exclusive_and_forwarded(self) -> None:
        parser = rbf.build_parser()
        argv = ["run", "--quick", "--filter", "alpha",
                "--save-baseline", "main", "fakecrate"]
        args = parser.parse_args(argv)
        self.assertEqual(rbf._bench_forward_args(args),
                         ["--quick", "--filter", "alpha", "--save-baseline", "main"])
        with self.assertRaises(SystemExit):
            parser.parse_args(
                ["run", "--save-baseline", "a", "--baseline", "b", "fakecrate"])
        # direct-call validation raises the typed error, not argparse
        conflict = argparse.Namespace(
            quick=False, exact=False, filter=[], save_baseline="a",
            baseline="b", baseline_lenient=None)
        with self.assertRaises(rbf.BenchError):
            rbf._bench_forward_args(conflict)
        # help text documents every passthrough flag
        help_text = parser.format_help()
        for flag in ("--quick", "--exact", "--filter", "--save-baseline",
                     "--baseline", "--baseline-lenient",
                     "--allow-stale-binaries"):
            self.assertIn(flag, help_text, f"--help must document {flag}")

    # 9. G15: standalone measure refuses unresolved binaries unless
    #    --allow-stale-binaries is passed explicitly.
    def test_standalone_measure_stale_binary_refusal(self) -> None:
        crate_root = os.path.join(self.scratch, "crates", "fakecrate")
        os.makedirs(crate_root)
        with open(os.path.join(crate_root, "Cargo.toml"), "w") as fh:
            fh.write(MIN_MANIFEST)
        rbf.CRATES_ROOT = os.path.join(self.scratch, "crates")
        rbf.DEPS_DIR = os.path.join(self.scratch, "deps")
        # no compile log, no deps output -> refusal with a typed error
        with self.assertRaises(rbf.BenchError) as ctx:
            rbf.compiled_binaries("fakecrate")
        self.assertIn("compile", str(ctx.exception))
        self.assertIn("--allow-stale-binaries", str(ctx.exception))
        # explicit opt-in: the deps-glob fallback applies (stale allowed)
        os.makedirs(rbf.DEPS_DIR)
        stale = os.path.join(rbf.DEPS_DIR, "fb")
        with open(stale, "wb") as fh:
            fh.write(b"\xcf\xfa\xed\xfe" + b"\x00" * 16)
        pairs = rbf.compiled_binaries("fakecrate", allow_stale=True)
        self.assertEqual(pairs, [("fb", stale)])
        # and the default remains refused even with a stale binary present
        with self.assertRaises(rbf.BenchError):
            rbf.compiled_binaries("fakecrate")

    # 10. findings 19+20: the --list probe pins PURRDF_BENCH_HOME to the
    #     shared store and its probe dir never survives the probe.
    def test_preflight_store_isolated_and_probe_dir_removed(self) -> None:
        binary = write_bench(self.scratch, "preflight-probe", GREEN_BENCH)
        probe_dir = rbf.lease_env_dir("pp")
        try:
            rbf._preflight_binary("fakecrate", "pp", binary)
            self.assertFalse(
                os.path.isdir(probe_dir),
                f"probe dir must be removed after the probe (finding 20): "
                f"{probe_dir}")
        finally:
            shutil.rmtree(probe_dir, ignore_errors=True)


class RestoreCourt(unittest.TestCase):
    """Non-vacuity: the court must not disturb the real register/logs nor
    leave /tmp lease or store surfaces behind."""

    def test_real_files_byte_exact_after_court(self) -> None:
        witnesses = [p for p in (rbf.REGISTER, rbf.RUNNER_LOG) if os.path.exists(p)]
        before = {p: sha256(p) for p in witnesses}
        # /tmp is SHARED with other lanes' real runs on this box, so the
        # leak check is a delta: only surfaces the court CREATED (absent
        # before, present after) are a court leak; pre-existing ones are
        # foreign state and not attributable here.
        tmp_surfaces = ("/tmp/purrdf-bench-store",
                        *glob.glob("/tmp/purrdf-fast-t[0-9]*"))
        absent_before = {p for p in tmp_surfaces if not os.path.exists(p)}
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(BenchFastShimCourt)
        result = unittest.TextTestRunner(verbosity=0).run(suite)
        self.assertEqual(result.errors, [])
        self.assertEqual(result.failures, [])
        for path, digest in before.items():
            if os.path.exists(path):
                self.assertEqual(sha256(path), digest,
                                 f"{path} was modified by the court")
        # the court itself runs against LEASE_ROOT in its own scratch dir;
        # assert that root fully purged (strong, hermetic lease-law form)
        self.assertFalse(
            os.path.exists(os.path.join(rbf.LEASE_ROOT, "purrdf-fast-t1")),
            "court leaked lease dirs inside its own LEASE_ROOT")
        leaked = sorted(p for p in absent_before if os.path.exists(p))
        self.assertEqual(
            leaked, [],
            f"court created /tmp lease/store surfaces: {leaked}")


if __name__ == "__main__":
    unittest.main()

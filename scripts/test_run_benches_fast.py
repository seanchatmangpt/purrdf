#!/usr/bin/env python3
"""Shim-court tests for run_benches_fast.py (stdlib unittest, no mocks).

Real collaborators: a real fake bench binary (a /bin/sh script that answers
`--list` with exit 0 and exits 101 on a real `--bench` run) executed by the
real measure_phase worker pool. The module's LOGS/REGISTER globals are
pointed at a scratch directory — configuration injection, not a mock: the
register rows asserted on are written by the real code under test through
its real flock/O_APPEND path. Restores every touched file byte-exact
(sha256 before/after); a court that cannot restore is FAIL.
"""
from __future__ import annotations

import hashlib
import importlib.util
import os
import shutil
import sys
import tempfile
import unittest

_HERE = os.path.dirname(os.path.abspath(__file__))
_SPEC = importlib.util.spec_from_file_location(
    "run_benches_fast", os.path.join(_HERE, "run_benches_fast.py"))
rbf = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(rbf)

FAKE_BENCH = """#!/bin/sh
if [ "$1" = "--list" ]; then exit 0; fi
exit 101
"""


def sha256(path: str) -> str:
    with open(path, "rb") as fh:
        return hashlib.sha256(fh.read()).hexdigest()


class BenchFastShimCourt(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.mkdtemp(prefix="purrdf-bench-fast-court-")
        self.old_logs, self.old_register = rbf.LOGS, rbf.REGISTER
        self.old_runner_log = rbf.RUNNER_LOG
        rbf.LOGS = self.scratch
        rbf.REGISTER = os.path.join(self.scratch, "register-fast.txt")
        rbf.RUNNER_LOG = os.path.join(self.scratch, "runner-fast.log")
        os.makedirs(rbf.LOGS, exist_ok=True)

    def tearDown(self) -> None:
        rbf.LOGS, rbf.REGISTER = self.old_logs, self.old_register
        rbf.RUNNER_LOG = self.old_runner_log
        shutil.rmtree(self.scratch, ignore_errors=True)

    def _register_text(self) -> str:
        with open(rbf.REGISTER) as fh:
            return fh.read()

    def test_fake_bench_rc101_is_red_row(self) -> None:
        binary = os.path.join(self.scratch, "fake-bench")
        with open(binary, "w") as fh:
            fh.write(FAKE_BENCH)
        os.chmod(binary, 0o755)
        rc = rbf.measure_phase("fakecrate", [("b101", binary)], resume=False)
        self.assertEqual(rc, 1, "a red bench must make measure_phase return 1")
        text = self._register_text()
        self.assertIn("fakecrate BENCH=b101 rc=101", text,
                      "register must record the real rc=101 row")

    def test_resume_requires_every_bench_green(self) -> None:
        with open(rbf.REGISTER, "w") as fh:
            fh.write("c BENCH=a rc=0 SECS=1 log=x\n")
        targets = [("a", False), ("b", False)]
        self.assertFalse(
            rbf.crate_has_green_row("c", targets),
            "a crate with only SOME benches green must NOT be resume-skipped")
        with open(rbf.REGISTER, "a") as fh:
            fh.write("c BENCH=b rc=0 SECS=1 log=y\n")
        self.assertTrue(rbf.crate_has_green_row("c", targets))

    def test_harness_true_target_refused(self) -> None:
        manifest = os.path.join(self.scratch, "Cargo.toml")
        with open(manifest, "w") as fh:
            fh.write('[[bench]]\nname = "h"\nharness = true\n')
        with self.assertRaises(rbf.BenchError):
            rbf.bench_names(manifest)


class RestoreCourt(unittest.TestCase):
    """Non-vacuity: the court must not disturb the real register/logs."""

    def test_real_files_byte_exact_after_court(self) -> None:
        witnesses = [p for p in (rbf.REGISTER, rbf.RUNNER_LOG) if os.path.exists(p)]
        before = {p: sha256(p) for p in witnesses}
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(BenchFastShimCourt)
        result = unittest.TextTestRunner(verbosity=0).run(suite)
        self.assertEqual(result.errors, [])
        self.assertEqual(result.failures, [])
        for path, digest in before.items():
            if os.path.exists(path):
                self.assertEqual(sha256(path), digest,
                                 f"{path} was modified by the court")


if __name__ == "__main__":
    unittest.main()

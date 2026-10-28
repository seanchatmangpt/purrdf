#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Sanctioned Python adapter for the fb-receipt grammar.

The ONE home of the grammar is scripts/fb-receipt.sh (see its header). This
module exists only because Python gates (pytest oracles) cannot source bash —
layering, per helpers-ledger doctrine. It must stay a thin formatter: same
grammar, no second opinion about what a receipt is.

Rule:fb-receipt-format — these two files are the only sources in the repo
allowed to emit a line beginning with "FB ".
"""

from __future__ import annotations

import fcntl
import re
import time

GRAMMAR = re.compile(
    r"^FB t[123] [a-z0-9-]+ [0-9]+ [0-9]+ "
    r"(PASS|FAIL|TIMEOUT|SKIP:[a-z0-9-]+|exit-[0-9]+)$"
)


def now_ms() -> int:
    return int(time.time() * 1000)


def emit(tier: str, name: str, count: int, ms: int, status: str) -> str:
    """Return (and print) one conforming receipt line."""
    t = f"t{tier}" if tier in ("1", "2", "3") else tier
    line = f"FB {t} {name} {int(count)} {int(ms)} {status}"
    if not GRAMMAR.match(line):
        raise ValueError(f"fb-receipt: nonconforming line: {line!r}")
    print(line, flush=True)
    return line


def append_jsonl(path: str, line: str, lock_path: str | None = None) -> None:
    """flock-locked single-line append to a JSONL receipt log."""
    lock = lock_path or f"{path}.lock"
    with open(lock, "w") as lock_fh:
        fcntl.flock(lock_fh, fcntl.LOCK_EX)
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(line.rstrip("\n") + "\n")


if __name__ == "__main__":
    ok = emit("t2", "oracles", 7296, 570, "PASS")
    assert GRAMMAR.match(ok)
    assert GRAMMAR.match("FB t1 fmt 12 84 PASS")
    assert GRAMMAR.match("FB t3 bounded 1 8 exit-0")
    assert GRAMMAR.match("FB t3 bounded 1 5024 TIMEOUT")
    assert GRAMMAR.match("FB t3 leaf-probe 1 970 SKIP:cargo-tree-unavailable")
    for bad in ("FB t4 x 0 0 PASS", "FB t1 FMT 0 0 PASS", "FB t1 fmt -1 0 PASS"):
        assert not GRAMMAR.match(bad), bad
    print("fb-receipt.py self-test PASS")

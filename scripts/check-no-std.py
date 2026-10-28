#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Out-of-band no_std / alloc / branching auditor for crates/branchless.

Lane C17. Independent of cargo: pure regex + brace-matching scan over
crates/branchless/src/*.rs. Exit 0 only when the no_std / forbid(unsafe_code)
attributes are present, no `extern crate std` exists outside `#[cfg(test)]`
spans, no heap tokens appear outside cfg(test), and no `unsafe` appears in
module files (lib.rs is the only allow site by construction of the crate).

Control-flow tokens inside `const fn` bodies outside cfg(test) are classified
WARN-with-count (compile-time-bound while-loops over scan kernels are
tolerated in this crate), never FAIL.

Usage: python3 scripts/check-no-std.py [src-dir]
Timing: reads each file once, single regex/match pass per file.
"""

from __future__ import annotations

import re
import sys
import time
from pathlib import Path

DEFAULT_SRC = Path(__file__).resolve().parent.parent / "crates" / "branchless" / "src"

BRANCH_TOKENS = [
    ("if", r"\bif\b"),
    ("match", r"\bmatch\b"),
    ("while", r"\bwhile\b"),
    ("loop", r"\bloop\b"),
]

LINE_START = [0]


_MASK = re.compile(
    r'//[^\n]*'                       # line comment
    r'|/\*.*?\*/'                     # block comment (non-greedy, DOTALL off:
                                      # doc comments do not span lines here)
    r'|r#*"[^"]*?"#*'                 # raw string
    r"|'(\\.|[^\\'])'"                # char literal
    r'|"(\\.|[^"\\\n])*"'             # string literal
    r"|b?'(\\.|[^\\'])'",             # byte literal (covered above; kept for clarity)
    re.DOTALL,
)
_NON_NL = re.compile(r"[^\n]")


def _strip_strings_comments(src: str) -> str:
    """Blank out comments and string/char literals so token scans see only
    code. Positions (and thus line numbers) preserved."""
    def repl(m: re.Match) -> str:
        return _NON_NL.sub(" ", m.group(0))
    return _MASK.sub(repl, src)


def _line_of(src: str, pos: int) -> int:
    """1-based line number of pos."""
    lo, hi = 0, len(src)
    # binary search over precomputed newline index would be faster per-call,
    # but each pos is queried once; use cached prefix computation via find.
    return src.count("\n", 0, pos) + 1


def _cfg_test_spans(clean: str) -> list[tuple[int, int]]:
    """Spans of items whose brace-balanced body follows a #[cfg(test)] attr.
    Brace matching from the attribute onward; naive but line-preserving."""
    spans: list[tuple[int, int]] = []
    for m in re.finditer(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", clean):
        # find first '{' after the attribute (skipping the item signature)
        depth = 0
        start = None
        for i in range(m.end(), len(clean)):
            if clean[i] == "{":
                if start is None:
                    start = i
                depth += 1
            elif clean[i] == "}" and start is not None:
                depth -= 1
                if depth == 0:
                    spans.append((start, i + 1))
                    break
        # attribute with no braces (e.g. cfg(test) on a use) — spanless
    return spans


def _in_spans(pos: int, spans: list[tuple[int, int]]) -> bool:
    return any(a <= pos < b for a, b in spans)


def _const_fn_spans(clean: str) -> list[tuple[int, int]]:
    """Spans of `const fn` bodies (brace-matched)."""
    spans: list[tuple[int, int]] = []
    for m in re.finditer(r"(?<![A-Za-z0-9_])const\s+fn\b", clean):
        start = clean.find("{", m.end())
        if start < 0:
            continue
        depth = 0
        for i in range(start, len(clean)):
            if clean[i] == "{":
                depth += 1
            elif clean[i] == "}":
                depth -= 1
                if depth == 0:
                    spans.append((start, i + 1))
                    break
    return spans


HEAP_PREFILTER = [
    ("Vec::", "Vec::", r"\bVec\s*::"),
    ("Box::", "Box::", r"\bBox\s*::"),
    ("String::", "String::", r"\bString\s*::"),
    ("format!", "format!", r"\bformat\s*!"),
    ("vec!", "vec!", r"\bvec\s*!"),
    ("collect::<Vec", "collect::<Vec", r"collect\s*::\s*<\s*Vec"),
    ("to_vec", ".to_vec(", r"\.to_vec\s*\("),
    ("HashMap", "HashMap", r"\bHashMap\b"),
    ("HashSet", "HashSet", r"\bHashSet\b"),
]


BRANCH_RE = re.compile("|".join(f"(?P<{l}>{p})" for l, p in BRANCH_TOKENS))


def audit_file(path: Path) -> dict:
    raw = path.read_text(encoding="utf-8")
    clean = _strip_strings_comments(raw)
    test_spans = _cfg_test_spans(clean)
    const_spans = _const_fn_spans(clean)

    res: dict = {
        "path": path,
        "no_std": bool(re.search(r"#\s*!?\s*\[\s*no_std\s*\]", clean)),
        "forbid_unsafe": bool(re.search(r"#\s*!?\s*\[\s*forbid\s*\(\s*unsafe_code\s*\)\s*\]", clean)),
        "extern_std": [],
        "heap": [],
        "branch": [],
        "unsafe_hits": [],
    }

    for m in re.finditer(r"extern\s+crate\s+std\s*;", clean):
        if not _in_spans(m.start(), test_spans):
            res["extern_std"].append(_line_of(clean, m.start()))

    # Heap tokens: literal-prefilter (C-speed str.find) before regex, so the
    # regex only runs over files that actually contain a candidate literal.
    for label, lit, pat in HEAP_PREFILTER:
        idx = clean.find(lit)
        if idx < 0:
            continue
        for m in re.finditer(pat, clean):
            if not _in_spans(m.start(), test_spans):
                res["heap"].append((label, _line_of(clean, m.start())))

    # Branch tokens: scan only const-fn body substrings (the only places they
    # are classified), excluding bodies nested inside cfg(test) spans.
    for a, b in const_spans:
        if _in_spans(a, test_spans):
            continue
        body = clean[a:b]
        for m in BRANCH_RE.finditer(body):
            res["branch"].append((m.lastgroup, _line_of(clean, a + m.start())))

    for m in re.finditer(r"(?<![A-Za-z0-9_])unsafe(?![A-Za-z0-9_])", clean):
        res["unsafe_hits"].append(_line_of(clean, m.start()))
    return res


def main(argv: list[str]) -> int:
    src_dir = Path(argv[1]) if len(argv) > 1 else DEFAULT_SRC
    files = sorted(src_dir.glob("*.rs"))
    if not files:
        print(f"NO_FILES: {src_dir}")
        return 2

    t0 = time.perf_counter()
    results = [audit_file(p) for p in files]
    elapsed_ms = (time.perf_counter() - t0) * 1000.0

    lib = next((r for r in results if r["path"].name == "lib.rs"), None)
    fails: list[str] = []
    warns: list[str] = []

    if lib is None:
        fails.append("LIB-MISSING: no lib.rs in scan set")
    else:
        if not lib["no_std"]:
            fails.append("NO_STD-MISSING: lib.rs lacks #![no_std]")
        else:
            print("OK-ATTR no_std: lib.rs declares #![no_std]")
        if not lib["forbid_unsafe"]:
            fails.append("FORBID-UNSAFE-MISSING: lib.rs lacks #![forbid(unsafe_code)]")
        else:
            print("OK-ATTR forbid(unsafe_code): lib.rs declares #![forbid(unsafe_code)]")

    for r in results:
        rel = r["path"]
        name = r["path"].name
        for ln in r["extern_std"]:
            fails.append(f"NO_STD-VIOLATION: {rel}:{ln} extern crate std outside cfg(test)")
        if name != "lib.rs":
            for ln in r["unsafe_hits"]:
                fails.append(f"UNSAFE-VIOLATION: {rel}:{ln} unsafe outside lib.rs allow sites")
        for label, ln in r["heap"]:
            fails.append(f"HEAP-VIOLATION: {rel}:{ln} {label} outside cfg(test)")

    for r in results:
        for label, ln in r["branch"]:
            warns.append(
                f"BRANCH-WARN: {r['path']}:{ln} `{label}` in const fn body "
                "(compile-time-bound scan loops tolerated as WARN, not FAIL)"
            )

    for w in warns:
        print(w)
    for f in fails:
        print(f)

    print(
        f"SUMMARY: files={len(files)} branch_warn={len(warns)} "
        f"heap_fail={sum(len(r['heap']) for r in results)} "
        f"extern_std_fail={sum(len(r['extern_std']) for r in results)} "
        f"unsafe_fail={sum(len(r['unsafe_hits']) if r['path'].name != 'lib.rs' else 0 for r in results)}"
    )
    print(f"TIMING: {elapsed_ms:.2f} ms for {len(files)} modules (budget 80 ms)")
    if elapsed_ms >= 80.0:
        print("PERF-FAIL: scan exceeded 80 ms budget")
        return 1
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

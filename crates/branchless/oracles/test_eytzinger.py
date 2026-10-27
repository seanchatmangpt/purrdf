"""Eytzinger oracle tests pinning the three session discoveries.

Layers:
  1. hand-computed subtree-size tables as literals (layout-length convention:
     the argument is the Eytzinger array length, n + 1);
  2. the len == 0 contract: the Rust kernel asserts ``len >= 1``; the Python
     oracle (gen.oracle._subtree_sizes) silently returns ``[]``. This module
     DOCUMENTS the Python behavior as the pinned contract and records the
     divergence from the Rust assert (see DIVERGENCE note below);
  3. the perfect-tree masking witness: for n = 7 (a perfect tree, array length
     2^3) a lower_bound descent never reads the wrong leaf entries, so the
     slot-count-convention table is indistinguishable from the correct one at
     every target — this is WHY green suites hid the original conflict. For
     n = 6 the same corruption diverges at target 7 (5 vs 6).

Dispatch contract mirrors test_kernels.py: ref.eytzinger when importable,
gen.oracle formulation A otherwise.

DIVERGENCE note for the Rust lane: gen.oracle.build_subtree_sizes accepts
len == 0 and returns [] (no exception); the Rust kernel
crates/branchless/src/eytzinger.rs asserts len >= 1. The Python oracle is
total; the Rust kernel is partial. If the Python side is ever tightened to
raise ValueError, test_subtree_sizes_len0_contract must flip in the same
commit.
"""

from __future__ import annotations

import bisect
import importlib
import time

import pytest

from gen import oracle

# ---------------------------------------------------------------------------
# dispatch: ref.eytzinger when importable, formulation A otherwise
# ---------------------------------------------------------------------------

try:  # pragma: no cover - depends on lane availability
    _ref = importlib.import_module("ref.eytzinger")  # type: ignore[import-not-found]
except ImportError:
    _ref = oracle  # formulation-A fallback (same policy as test_kernels.py)


def build_subtree_sizes(length: int) -> list[int]:
    """Reference subtree-size table for an Eytzinger array of ``length``."""
    return oracle._subtree_sizes(length) if _ref is oracle else _ref.build_subtree_sizes(length)


def build_layout(src: list[int]) -> list[int]:
    """Reference Eytzinger layout ( formulation A recursive in-order)."""
    return oracle._layout(src) if _ref is oracle else _ref.build_eytzinger_layout(src)


# ---------------------------------------------------------------------------
# branchless-descent formulation B: a DIFFERENT formulation than the
# generator's linear-count lower_bound — real subtree-size descent.
# ---------------------------------------------------------------------------


def lower_bound_descent(eyt: list[int], sizes: list[int], target: int) -> int:
    """Eytzinger lower_bound by subtree-size descent (formulation B).

    Walks slots from the root; on a right turn (eyt[k] < target) the whole
    left subtree of k plus k itself is below the target, so the running
    count gains sizes[2k] + 1. Returns the number of elements < target,
    i.e. bisect_left. Never reads sizes[2k+1], which is exactly why a
    corrupted right-child leaf entry can be masked (see the witness test).
    """
    res = 0
    k = 1
    while k < len(eyt):
        if eyt[k] < target:
            res += (sizes[2 * k] if 2 * k < len(sizes) else 0) + 1
            k = 2 * k + 1
        else:
            k = 2 * k
    return res


# ---------------------------------------------------------------------------
# 1. hand-computed literal tables
# ---------------------------------------------------------------------------

LITERAL_TABLES = [
    (2, [0, 1]),                    # n=1
    (3, [0, 2, 1]),                 # n=2
    (4, [0, 3, 1, 1]),              # n=3
    (7, [0, 6, 3, 2, 1, 1, 1]),     # n=6
    (8, [0, 7, 3, 3, 1, 1, 1, 1]),  # n=7
]


@pytest.mark.parametrize("length,expected", LITERAL_TABLES)
def test_subtree_sizes_literals(length: int, expected: list[int]) -> None:
    """Hand-computed tables must be reproduced exactly (layout-length arg)."""
    assert build_subtree_sizes(length) == expected


def test_subtree_sizes_single_arg_is_layout_length() -> None:
    """The argument is the array length (n+1), not n: sizes(2) is n=1."""
    assert build_subtree_sizes(2) == [0, 1]


# ---------------------------------------------------------------------------
# 2. len == 0 contract
# ---------------------------------------------------------------------------


def test_subtree_sizes_len0_contract() -> None:
    """DOCUMENTED contract: the Python oracle is total — len == 0 returns [].

    DIVERGENCE from the Rust kernel, which asserts len >= 1. This test pins
    the Python side; if the oracle is tightened to raise ValueError, flip
    this assertion in the same commit (see module docstring).
    """
    assert build_subtree_sizes(0) == []


def test_subtree_sizes_len0_not_confused_with_n1() -> None:
    """[] (len 0) and [0] (n=0, layout length 1) are distinct answers."""
    assert build_subtree_sizes(0) == []
    assert build_subtree_sizes(1) == [0]


# ---------------------------------------------------------------------------
# 3. perfect-tree masking witness
# ---------------------------------------------------------------------------


def _eytzinger_of_ramp(n: int) -> list[int]:
    return build_layout(list(range(1, n + 1)))


def test_perfect_tree_masks_wrong_table_n7() -> None:
    """n=7: the WRONG table (slot-count convention) is undetectable.

    Correct (layout-length) table: [0,7,3,3,1,1,1,1]. Wrong table fixes the
    root to n=6's value and zeroes the last leaf: [0,6,3,2,1,1,1,0]. The
    descent never reads sizes[1] or sizes[2k+1], and index 7 = 2^3 - 1 is
    only ever a right child, so every target yields the same answer — the
    original session's green suite hid the convention conflict precisely
    because its only fixture was a perfect tree.
    """
    eyt = _eytzinger_of_ramp(7)
    good = [0, 7, 3, 3, 1, 1, 1, 1]
    bad = [0, 6, 3, 2, 1, 1, 1, 0]
    assert good != bad
    for target in range(0, 10):  # below min, every element, above max
        want = bisect.bisect_left(list(range(1, 8)), target)
        assert lower_bound_descent(eyt, bad, target) == want, (
            f"n=7 target={target}: wrong table was detectable (masking broke)")
        assert lower_bound_descent(eyt, good, target) == want


def test_imperfect_tree_exposes_wrong_table_n6() -> None:
    """n=6: the same corruption MUST diverge — target 7 gives 5, not 6.

    The descent reads sizes[6] (slot 6 is slot 3's left child in the
    length-7 array); the wrong table zeroes it, so the final right turn at
    slot 3 undercounts by one: 5 vs bisect_left's 6.
    """
    eyt = _eytzinger_of_ramp(6)
    assert eyt == [0, 4, 2, 6, 1, 3, 5]
    good = [0, 6, 3, 2, 1, 1, 1]
    bad = [0, 5, 3, 1, 1, 1, 0]
    assert lower_bound_descent(eyt, good, 7) == 6
    assert lower_bound_descent(eyt, bad, 7) == 5
    assert lower_bound_descent(eyt, bad, 7) != lower_bound_descent(eyt, good, 7)


# ---------------------------------------------------------------------------
# 4. layout + search differential vs bisect / linear scan
# ---------------------------------------------------------------------------

SIZES = [0, 1, 2, 3, 5, 6, 7, 9, 12, 13, 16, 31, 33]


@pytest.mark.parametrize("n", SIZES)
def test_layout_and_lower_bound_differential(n: int) -> None:
    """Eytzinger descent over real layouts must equal bisect_left for all
    in-range targets plus ±bounds, and layout must be a sorted-preserving
    permutation of the source with a zero sentinel slot."""
    src = list(range(1, n + 1))
    eyt = build_layout(src)
    sizes = build_subtree_sizes(n + 1)

    assert len(eyt) == n + 1
    assert eyt[0] == 0
    assert sorted(eyt[1:]) == src  # permutation of the source
    assert len(sizes) == n + 1
    assert sizes[0] == 0
    if n >= 1:
        assert sizes[1] == n  # root covers everything

    for target in [0, *src, n + 1]:  # -bound, every element, +bound
        want = bisect.bisect_left(src, target)
        got = lower_bound_descent(eyt, sizes, target)
        assert got == want, f"n={n} target={target}: descent {got} != bisect {want}"


def test_differential_total_runtime_under_100ms() -> None:
    """Whole-module budget witness: the differential sweep itself must stay
    under 100 ms (the full module runs well under it; see pytest report)."""
    t0 = time.perf_counter()
    for n in SIZES:
        src = list(range(1, n + 1))
        eyt = build_layout(src)
        sizes = build_subtree_sizes(n + 1)
        for target in range(0, n + 2):
            assert (lower_bound_descent(eyt, sizes, target)
                    == bisect.bisect_left(src, target))
    elapsed_ms = (time.perf_counter() - t0) * 1000.0
    assert elapsed_ms < 100.0, f"differential sweep took {elapsed_ms:.1f} ms"

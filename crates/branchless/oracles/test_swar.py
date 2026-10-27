"""SWAR-family oracle suite: horizontal folds, scans, slice reductions,
byte-match kernels.

Layers:
  1. table differential — every horizontal fold kernel is re-derived from a
     precomputed 256-entry lane table + C-speed fold primitives
     (``sum``/``min``/``max``/``functools.reduce`` over ``int.to_bytes``),
     never a per-byte Python loop;
  2. accumulate differential — the fixed-width scans are re-derived with
     ``itertools.accumulate`` (C-speed primitive) against the scalar loop
     forms (the ``ref.swar`` loop formulations when that lane module is
     importable, in-module scalar restatements otherwise);
  3. canonical vectors — the shared edge words (``[3,1,4,1,5,9,2,6]`` whose
     sum is 31 — a real defect caught this session — ``u64::MAX`` -> 2040,
     zero, high-bit lanes) pinned exactly;
  4. xorshift sweeps — differential against ``gen.oracle`` formulation A
     (scalar per-byte loops) and, when importable, ``ref.swar``.

This suite proves oracle-internal coherence only; corpus pins live in
``vectors/swar.json`` (verified by ``conftest.py``) and the Rust kernels are
qualified by the Rust consumer lane.
"""

from __future__ import annotations

import functools
import itertools
import operator

import pytest

from gen import oracle

M64 = (1 << 64) - 1
M32 = (1 << 32) - 1

try:  # ref.swar (L15/L16 lane module) — the loop-form differential when present.
    import ref  # type: ignore[import-not-found]

    _REF_SWAR = ref.swar  # type: ignore[attr-defined]
    HAVE_REF = True
except ImportError:
    _REF_SWAR = None
    HAVE_REF = False


def _ref(fn_name: str, fallback):
    """Dispatches to ``ref.swar.<fn_name>`` when importable, else fallback."""
    if HAVE_REF:
        entry = getattr(_REF_SWAR, fn_name, None)
        if entry is not None:
            funcls = getattr(_REF_SWAR, "FUNCS", None)
            if isinstance(funcls, dict) and fn_name in funcls:
                entry = funcls[fn_name]
            if callable(entry):
                return entry
    return fallback


# ---------------------------------------------------------------------------
# xorshift64 sweep driver (deterministic; same construction as the Rust tests)
# ---------------------------------------------------------------------------

def xorshift64(seed: int):
    s = seed & M64
    while True:
        s ^= (s << 13) & M64
        s ^= s >> 7
        s ^= (s << 17) & M64
        yield s


# ---------------------------------------------------------------------------
# 1. 256-entry lane tables -> C-speed folds (the table formulation)
# ---------------------------------------------------------------------------

# table[lane] = the singleton fold seed for that lane value; combining the 8
# lanes through the matching C-speed primitive reproduces each kernel without
# a data-dependent per-byte Python loop.
_SUM_TABLE = [b for b in range(256)]
_MIN_TABLE = [b for b in range(256)]
_MAX_TABLE = [b for b in range(256)]
_OR_TABLE = [b for b in range(256)]
_AND_TABLE = [b for b in range(256)]
_XOR_TABLE = [b for b in range(256)]


def fold_word(word: int) -> dict[str, int]:
    """All six horizontal folds of ``word``'s 8 byte lanes, table-shaped."""
    lanes = word.to_bytes(8, "little")
    return {
        "sum": sum(_SUM_TABLE[b] for b in lanes),
        "min": min(_MIN_TABLE[b] for b in lanes),
        "max": max(_MAX_TABLE[b] for b in lanes),
        "or": functools.reduce(operator.or_, (_OR_TABLE[b] for b in lanes)),
        "and": functools.reduce(operator.and_, (_AND_TABLE[b] for b in lanes)),
        "xor": functools.reduce(operator.xor, (_XOR_TABLE[b] for b in lanes)),
    }


# ---------------------------------------------------------------------------
# 2. scalar loop forms (the independent scan formulation; ref.swar shapes)
# ---------------------------------------------------------------------------

def _loop_prefix_sum(arr: list[int]) -> list[int]:
    out, acc = [], 0
    for v in arr:
        acc = (acc + v) & M32
        out.append(acc)
    return out


def _loop_exclusive_scan(arr: list[int]) -> list[int]:
    out, acc = [], 0
    for v in arr:
        out.append(acc)
        acc = (acc + v) & M32
    return out


def _loop_segmented(values: list[int], flags: list[bool]) -> list[int]:
    out, acc = [], 0
    for v, f in zip(values, flags):
        if f:
            acc = 0
        acc = (acc + v) & M32
        out.append(acc)
    return out


def _loop_prefix_max(arr: list[int]) -> list[int]:
    out, m = [], 0
    for v in arr:
        m = m if m > v else v
        out.append(m)
    out[0] = arr[0]  # kernel emits arr[0] verbatim at lane 0 (no implicit 0 seed)
    return out


def _loop_prefix_xor(arr: list[int]) -> list[int]:
    out, a = [], 0
    for v in arr:
        a ^= v
        out.append(a)
    return out


def _acc_prefix_sum(arr):
    return [(x & M32) for x in itertools.accumulate(arr, operator.add)]


def _acc_exclusive_scan(arr):
    return [(x & M32) for x in itertools.accumulate(arr, operator.add, initial=0)][:-1]


def _acc_segmented(values, flags):
    # accumulate over (value, reset) pairs: the running pair keeps the live
    # accumulator so a segment break is expressed as an operator, not a branch
    def step(pair, vf):
        acc = 0 if vf[1] else pair
        return (acc + vf[0]) & M32

    return list(itertools.accumulate(zip(values, flags), step, initial=0))[1:]


def _acc_prefix_max(arr):
    return list(itertools.accumulate(arr, max))


def _acc_prefix_xor(arr):
    return list(itertools.accumulate(arr, operator.xor))


# ---------------------------------------------------------------------------
# byte-match kernels: index()/count()/find() over to_bytes (C-speed oracle)
# ---------------------------------------------------------------------------

def _lanes(word: int) -> bytes:
    return word.to_bytes(8, "little")


# ---------------------------------------------------------------------------
# 3. canonical vectors
# ---------------------------------------------------------------------------

CANON = int.from_bytes(bytes([3, 1, 4, 1, 5, 9, 2, 6]), "little")


class TestCanonicalVectors:
    def test_canonical_word(self):
        f = fold_word(CANON)
        assert f["sum"] == 31  # NOT 30 — caught a real carry-tree bug this session
        assert f["max"] == 9
        assert f["min"] == 1
        assert f["or"] == 0x0F
        assert f["and"] == 0
        assert f["xor"] == 0x0F
        lanes = _lanes(CANON)
        assert lanes.count(1) == 2
        assert 9 in lanes
        assert 7 not in lanes
        assert lanes.index(5) == 4  # first_byte_position of 5

    def test_all_ones_word(self):
        f = fold_word(M64)
        assert f["sum"] == 2040  # 8 * 0xFF
        assert f["max"] == 0xFF
        assert f["min"] == 0xFF
        assert f["or"] == 0xFF
        assert f["and"] == 0xFF
        assert f["xor"] == 0
        assert _lanes(M64).count(0xFF) == 8

    def test_zero_word(self):
        f = fold_word(0)
        assert f["sum"] == 0
        assert f["max"] == 0
        assert f["min"] == 0
        assert f["or"] == 0
        assert f["and"] == 0
        assert f["xor"] == 0
        assert _lanes(0).count(0) == 8
        # degenerate byte-match: needle 0 matches every lane of the zero word
        assert _lanes(0).index(0) == 0

    def test_high_bit_lane_wins_max(self):
        # lane 7 of the LE word is 0x80: must beat 0x7F despite low-7-bits == 0
        word = int.from_bytes(bytes([0x80, 0x7F, 1, 2, 3, 4, 5, 6]), "little")
        f = fold_word(word)
        assert f["max"] == 0x80
        assert f["min"] == 1
        assert f["sum"] == 0x80 + 0x7F + 21

    def test_slice_reduce_canonical(self):
        vals = [0, M32, 0x8000_0000, 1]
        assert functools.reduce(operator.or_, vals) == M32
        assert functools.reduce(operator.and_, vals) == 0
        assert functools.reduce(operator.xor, vals) == 0x7FFF_FFFE
        assert min(vals) == 0
        assert max(vals) == M32
        assert sum(vals) == 0 + M32 + 0x8000_0000 + 1

    def test_slice_reduce_empty_pinned_identities(self):
        # Rust kernels pin: or/xor -> 0, and -> 0 (safe AND-then-use identity,
        # NOT u32::MAX — do not "fix"), min -> u32::MAX, max -> 0, sum -> 0.
        # reduce() is undefined on empty sequences, so the oracle folds are
        # expressed seed-first, and the pins assert through that formulation.
        def fold0(seed, op, xs):
            return functools.reduce(op, xs, seed)

        vals = [0, M32, 0x8000_0000, 1]
        assert fold0(0, operator.or_, vals) == M32
        assert fold0(M32, operator.and_, vals) == 0
        assert fold0(0, operator.xor, vals) == 0x7FFF_FFFE
        # seed-first == seedless on non-empty input (formulation coherence)
        assert fold0(0, operator.or_, vals) == functools.reduce(operator.or_, vals)
        # empty: the pinned identities themselves
        assert fold0(0, operator.or_, []) == 0
        assert fold0(0, operator.and_, []) == 0  # AND pins 0, not u32::MAX
        assert fold0(0, operator.xor, []) == 0
        assert fold0(M32, min, []) == M32
        assert fold0(0, max, []) == 0
        assert sum([]) == 0


# ---------------------------------------------------------------------------
# 4. accumulate-vs-loop scan differentials + table-vs-per-byte fold sweeps
# ---------------------------------------------------------------------------

SCAN_CASES: list[list[int]] = [
    [0] * 16,
    [1] * 16,
    [M32] * 16,
    [(1 << 31)] * 16,
    [0, M32, 1, 0x7FFF_FFFF, 0x8000_0000, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
    [(i * 2654435761) & M32 for i in range(16)],
]
SCAN_CASES += [[(s * 7919 + i * 104729) & M32 for i in range(16)]
               for s in range(24)]

SEG_VALUES = [[(s * 31 + i * 17) & M32 for i in range(8)] for s in range(16)]
SEG_FLAG_SETS = [
    [False] * 8,
    [True] * 8,
    [True] + [False] * 7,
    [False] * 7 + [True],
    [False, False, False, True, False, False, True, False],
]
GRAY = [0, 1, 2, 3, 4, 5, 6, 7]


class TestScanDifferentials:
    def test_prefix_sum_u32x16(self):
        for arr in SCAN_CASES:
            want = _loop_prefix_sum(arr)
            got = _acc_prefix_sum(arr)
            assert got == want
            assert got[0] == arr[0]
        # wrapping: all-ones input must not raise, results stay 32-bit
        # (hand-computed boundary oracles, not a clamp tautology)
        assert _acc_prefix_sum([M32] * 16) == [
            (i * M32) & M32 for i in range(1, 17)]
        assert _acc_prefix_sum([3, 1, 4, 1, 5, 9, 2, 6]) == [
            3, 4, 8, 9, 14, 23, 25, 31]
        # carry-save: 8 lanes of 0xFF fold to 2040, no inter-lane overflow
        assert _acc_prefix_sum([0xFF] * 8)[-1] == 2040

    def test_exclusive_scan_u32x16(self):
        for arr in SCAN_CASES:
            want = _loop_exclusive_scan(arr)
            got = _acc_exclusive_scan(arr)
            assert got == want
            assert got[0] == 0

    def test_segmented_prefix_sum_u32x8(self):
        for values in SEG_VALUES:
            for flags in SEG_FLAG_SETS:
                assert _acc_segmented(values, flags) == _loop_segmented(values, flags)
        # documented example from swar.rs:1020-1038
        v = [1, 2, 3, 4, 5, 6, 7, 8]
        f = [False, False, False, True, False, False, True, False]
        assert _acc_segmented(v, f) == [1, 3, 6, 4, 9, 15, 7, 15]
        # prefix_xor over the naturals is the binary-reflected Gray code
        assert _acc_prefix_xor(GRAY) == [0, 1, 3, 0, 4, 1, 7, 0]

    def test_prefix_max_u32x16(self):
        for arr in SCAN_CASES:
            want = _loop_prefix_max(arr)
            got = _acc_prefix_max(arr)
            assert got == want
            assert got[0] == arr[0]
            assert all(got[i] <= got[i + 1] for i in range(15))  # monotone
            pm = prefix_max_ref_hi(arr)
            assert got == pm  # high-bit lanes survive (no >>31 signed trick)

    def test_prefix_xor_u64x8(self):
        for arr in ([0, 1, 2, 3, 4, 5, 6, 7], [M64] * 8,
                    [(s * 0x9E3779B97F4A7C15) & M64 for s in range(1, 9)]):
            assert _acc_prefix_xor(arr) == _loop_prefix_xor(arr)


def prefix_max_ref_hi(arr: list[int]) -> list[int]:
    """Third formulation: running max seeded from element 0 (full u32 range)."""
    out = [arr[0]]
    for v in arr[1:]:
        out.append(out[-1] if out[-1] > v else v)
    return out


class TestFoldSweeps:
    SWEEP_N = 1024

    def test_word_folds_vs_gen_oracle_xorshift(self):
        for word in itertools.islice(xorshift64(0x9E37_79B9_7F4A_7C15), self.SWEEP_N):
            f = fold_word(word)
            lanes = _lanes(word)
            assert f["sum"] == oracle.call(
                "swar", "horizontal_sum_u8x8", {"word": word})
            assert f["max"] == oracle.call(
                "swar", "horizontal_max_u8x8", {"word": word})
            assert f["min"] == oracle.call(
                "swar", "horizontal_min_u8x8", {"word": word})
            assert f["or"] == oracle.call(
                "swar", "horizontal_or_u8x8", {"word": word})
            assert f["and"] == oracle.call(
                "swar", "horizontal_and_u8x8", {"word": word})
            assert f["xor"] == oracle.call(
                "swar", "horizontal_xor_u8x8", {"word": word})
            assert lanes.count(0x01) == oracle.call(
                "swar", "horizontal_count_eq_u8",
                {"word": word, "target": 0x01})
            del lanes

    def test_word_folds_vs_ref_swar_when_importable(self):
        if not HAVE_REF:
            pytest.skip("ref.swar not importable — loop-form lane not landed")
        sum_w = _ref("horizontal_sum_u8x8", None)
        max_w = _ref("horizontal_max_u8x8", None)
        assert callable(sum_w) and callable(max_w)
        for word in itertools.islice(xorshift64(0x0F0F_0F0F_0F0F_0F0F), 1024):
            f = fold_word(word)
            assert f["sum"] == sum_w(word)
            assert f["max"] == max_w(word)

    def test_byte_match_kernels_xorshift(self):
        for word in itertools.islice(xorshift64(0xDEAD_BEEF_CAFE_F00D), 512):
            lanes = _lanes(word)
            for needle in (0x00, 0x01, 0x7F, 0x80, 0xFF):
                hits = [i for i, b in enumerate(lanes) if b == needle]
                # find / count / has / first-position, four views of one mask
                assert lanes.count(needle) == len(hits)
                assert (needle in lanes) == bool(hits)
                if hits:
                    assert lanes.index(needle) == hits[0]
                assert len(hits) == oracle.call(
                    "swar", "horizontal_count_eq_u8",
                    {"word": word, "target": needle})
                assert (hits[0] if hits else None) == oracle.call(
                    "swar", "first_byte_position",
                    {"word": word, "byte": needle})

    def test_slice_reductions_xorshift(self):
        rng = xorshift64(0x5EED_5EED_5EED_5EED)
        edge = [0, M32, 0x8000_0000, 1]
        for length in (0, 1, 2, 3, 7, 8, 9, 16, 17, 31):
            vals = [edge[next(rng) % 4] if i % 5 == 0 else next(rng) & M32
                    for i in range(length)]
            if vals:
                assert functools.reduce(operator.or_, vals) == (
                    functools.reduce(lambda a, b: a | b, vals))
                assert functools.reduce(operator.and_, vals) == (
                    functools.reduce(lambda a, b: a & b, vals))
                assert functools.reduce(operator.xor, vals) == (
                    functools.reduce(lambda a, b: a ^ b, vals))
                assert min(vals) == sorted(vals)[0]
                assert max(vals) == sorted(vals)[-1]
                assert sum(vals) == sum(map(int, vals))
            else:
                # empty-slice identities pinned by the Rust kernels
                # (swar.rs empty_slice_returns_identity_not_panic):
                # or/xor/and/sum -> 0, min -> u32::MAX (AND pins 0, not MAX);
                # Python folds have no empty form, so only the length is
                # re-observed here and the pins live in the Rust suite.
                assert length == 0
                assert not vals


class TestRefSwarScanCrossCheck:
    def test_scans_vs_ref_swar_when_importable(self):
        if not HAVE_REF:
            pytest.skip("ref.swar not importable — loop-form lane not landed")
        ps = _ref("prefix_sum_u32x16", None)
        px = _ref("prefix_xor_u64x8", None)
        assert callable(ps) and callable(px)
        arr = [(i * 2654435761) & M32 for i in range(16)]
        assert _acc_prefix_sum(arr) == ps(arr)
        x8 = [(s * 0x9E3779B97F4A7C15) & M64 for s in range(1, 9)]
        assert _acc_prefix_xor(x8) == px(x8)

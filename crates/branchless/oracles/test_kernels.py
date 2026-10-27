"""Corpus-vs-oracle coherence suite for the purrdf-branchless kernels.

Layers:
  1. dispatch — every case's fn must resolve to a callable oracle (ref.* when
     the L15/L16 lane modules are importable, formulation A from gen.oracle
     otherwise);
  2. coherence — every pinned case must reproduce its ``expect`` exactly
     (structural equality on the 0x-hex / Option / f32-bits encoding);
  3. second formulation — every boundary/corner case is additionally
     re-derived with an in-test formulation distinct from the generator's
     (rank sorts vs std sorted(), closed forms vs bit loops, explicit-stack
     traversal vs recursion, binary search vs linear count).

This suite proves corpus<->oracle coherence only; a Rust kernel that
disagrees with the corpus is caught by the Rust consumer lane.
"""

from __future__ import annotations

import functools
import importlib
import struct

import pytest

from gen import oracle

M64 = oracle.M64
M32 = oracle.M32
MODULES = ["bitset", "swar", "eytzinger", "sort_small", "packing"]


# ---------------------------------------------------------------------------
# decode
# ---------------------------------------------------------------------------

def dec_in(args: dict, types: list) -> list:
    vals = []
    for name, ty in types:
        v = args[name]
        if ty in ("u64s", "u32s", "u8s"):
            vals.append([int(x, 16) for x in v])
        elif ty == "bool":
            vals.append(bool(int(v, 16)))
        else:
            vals.append(int(v, 16))
    return vals


def dec_out(ty: str, v):
    if ty in ("u64s", "u32s", "u8s"):
        return [int(x, 16) for x in v]
    if ty == "opt_us":
        if v is None or "none" in v:
            return None
        return int(v["some"], 16)
    if ty == "f32b":
        return struct.unpack("<f", struct.pack("<I", int(v["bits_f32"], 16)))[0]
    if ty == "bool":
        return v
    return int(v, 16)


# ---------------------------------------------------------------------------
# dispatch: ref.* (lanes L15/L16) when importable, formulation A otherwise
# ---------------------------------------------------------------------------

REF_TABLE: dict[str, dict[str, object]] = {}
REF_STATUS = "unavailable"


def _probe_ref() -> None:
    global REF_STATUS
    for module in MODULES:
        try:
            mod = importlib.import_module(f"ref.{module}")
        except ImportError:
            continue
        entry_table = {}
        for fn in oracle.SPECS[module]:
            entry = None
            funcls = getattr(mod, "FUNCS", None)
            if isinstance(funcls, dict) and fn in funcls:
                entry = funcls[fn]
            elif hasattr(mod, fn):
                entry = getattr(mod, fn)
            if entry is None:
                continue
            entry_table[fn] = entry
        if entry_table:
            REF_TABLE[module] = entry_table
    REF_STATUS = (f"ref modules for {sorted(REF_TABLE)}" if REF_TABLE
                  else "no ref.<module> importable — dispatching via gen.oracle")


_probe_ref()


def run_oracle(module: str, fn: str, vals: list):
    """Dispatches one case through ref.* when available, else formulation A."""
    types = oracle.arg_types(module, fn)
    if module in REF_TABLE and fn in REF_TABLE[module]:
        entry = REF_TABLE[module][fn]
        if callable(entry):
            return entry(*vals)
        if isinstance(entry, dict):
            for key in ("f", "fn", "call", "oracle"):
                if callable(entry.get(key)):
                    return entry[key](*vals)
        raise TypeError(f"unusable ref entry for {module}.{fn}: {entry!r}")
    return oracle.call(module, fn, {name: v for (name, _), v in zip(types, vals)})


# ---------------------------------------------------------------------------
# formulation B — second in-test derivation for the boundary/corner tier.
# Deliberately different idiom per family: closed forms where formulation A
# loops, rank-sort where A calls sorted(), explicit-stack traversal where A
# recurses, binary search where A counts linearly, shift-based lane access
# where A uses to_bytes.
# ---------------------------------------------------------------------------

def _b_rank_sort(items):
    out = [0] * len(items)
    for i, x in enumerate(items):
        r = sum(1 for j, y in enumerate(items) if y < x or (y == x and j < i))
        out[r] = x
    return out


def _b_inorder_slots(n):
    """Explicit-stack in-order visit sequence of the complete 1-based tree."""
    order = []
    todo = [(1, 0)]
    while todo:
        k, phase = todo.pop()
        if k > n:
            continue
        if phase == 0:
            todo.append((k, 1))
            todo.append((2 * k, 0))
        else:
            order.append(k)
            todo.append((2 * k + 1, 0))
    return order


def _b_layout(src):
    n = len(src)
    dest = [0] * (n + 1)
    for i, slot in enumerate(_b_inorder_slots(n)):
        dest[slot] = src[i]
    return dest


def _b_perm(src):
    n = len(src)
    perm = [0] * (n + 1)
    for i, slot in enumerate(_b_inorder_slots(n)):
        perm[slot] = i
    return perm


def _b_perm_inverse(perm):
    inv = [0] * len(perm)
    for slot in range(1, len(perm)):
        inv[perm[slot]] = slot
    return inv


def _b_subtree_sizes(length):
    # top-down memo recursion instead of A's bottom-up loop
    from functools import lru_cache

    n = length - 1

    @lru_cache(maxsize=None)
    def size(k):
        if k > n:
            return 0
        return 1 + size(2 * k) + size(2 * k + 1)

    return [size(k) if k >= 1 else 0 for k in range(length)]


def _b_bisect_left(src, target):
    lo, hi = 0, len(src)
    while lo < hi:
        mid = (lo + hi) // 2
        if src[mid] < target:
            lo = mid + 1
        else:
            hi = mid
    return lo


def _b_eyt_search(eyt, target):
    src = sorted(eyt[1:])
    i = _b_bisect_left(src, target)
    if i >= len(src) or src[i] != target:
        return None
    perm = _b_perm(src)
    # slot holding sorted index i
    for slot in range(1, len(perm)):
        if perm[slot] == i:
            return slot
    return None


def _b_eyt_lower_bound(eyt, sizes, target):
    return _b_bisect_left(sorted(eyt[1:]), target)


def _b_lanes_shift(word):
    return [(word >> (8 * i)) & 0xFF for i in range(8)]


def _b_find_word(word, byte):
    out = 0
    for i, lane in enumerate(_b_lanes_shift(word)):
        if lane == byte:
            out += 0x80 << (8 * i)
    return out


def _b_classify(word):
    out = 0
    for i, b in enumerate(_b_lanes_shift(word)):
        c = 0
        c |= 0x01 * (0x30 <= b <= 0x39)
        c |= 0x02 * (0x41 <= b <= 0x5A or 0x61 <= b <= 0x7A)
        c |= 0x04 * (0x09 <= b <= 0x0D or b == 0x20)
        c |= 0x08 * (0x41 <= b <= 0x5A)
        c |= 0x10 * (0x61 <= b <= 0x7A)
        out |= c << (8 * i)
    return out


def _b_parse8(word):
    digits = []
    for i in range(8):
        b = (word >> (8 * i)) & 0xFF
        if not 0x30 <= b <= 0x39:
            return None
        digits.append(b - 0x30)
    return functools.reduce(lambda a, d: 10 * a + d, digits, 0)


def _b_hexval(b):
    if 0x30 <= b <= 0x39:
        return b - 0x30
    if 0x41 <= b <= 0x46:
        return b - 0x41 + 10
    if 0x61 <= b <= 0x66:
        return b - 0x61 + 10
    return None


def _b_parse4hex(word):
    vals = [_b_hexval((word >> (8 * i)) & 0xFF) for i in range(4)]
    if any(v is None for v in vals):
        return None
    return functools.reduce(lambda a, v: (a << 4) | v, vals, 0)


def _b_field_deposit(word, value, k, offset):
    k = max(1, min(k, 32))
    offset &= 31
    fm = (1 << k) - 1
    return (word & ((M64 ^ ((fm << offset) & M64)))) | ((value & fm) << offset)


def _b_field_extract(word, k, offset):
    k = max(1, min(k, 32))
    offset &= 31
    return (word >> offset) & ((1 << k) - 1)


def _b_compress(value, mask):
    sel = [i for i in range(64) if (mask >> i) & 1]
    bits = "".join(str((value >> i) & 1) for i in sel)
    return int(bits[::-1], 2) if bits else 0


def _b_deposit(value, mask):
    srcs = [i for i in range(64) if (mask >> i) & 1]
    out = 0
    for pos, i in enumerate(srcs):
        out |= ((value >> pos) & 1) << i
    return out


def _f32(x: float) -> float:
    return struct.unpack("<f", struct.pack("<f", x))[0]


SECOND: dict[tuple[str, str], object] = {
    # --- bitset -------------------------------------------------------------
    ("bitset", "set_bit_u64"): lambda x, pos: x | (1 << (pos & 63)),
    ("bitset", "clear_bit_u64"): lambda x, pos: x & (M64 ^ (1 << (pos & 63))),
    ("bitset", "rank_u64"): lambda x, pos: bin(
        x & ((1 << (min(pos, 63) + 1)) - 1)).count("1"),
    ("bitset", "rank_exclusive_u64"): lambda x, pos: bin(
        x & ((1 << (pos & 63)) - 1)).count("1"),
    ("bitset", "select_bit_u64"): lambda x, n: next(
        (b for b in range(64)
         if bin(x & ((1 << (b + 1)) - 1)).count("1") == n + 1 and (x >> b) & 1),
        None),
    ("bitset", "xor_u64"): lambda x, y: x ^ y,
    ("bitset", "not_u64"): lambda x: M64 ^ x,
    ("bitset", "and_not_u64"): lambda x, y: x & (M64 ^ y),
    ("bitset", "intersect_u64"): lambda x, y: x & y,
    ("bitset", "union_u64"): lambda x, y: x | y,
    ("bitset", "is_subset_u64"): lambda x, y: int(x & (M64 ^ y) == 0),
    ("bitset", "popcount_u64"): lambda x: sum((x >> i) & 1 for i in range(64)),
    ("bitset", "reverse_bits_u64"): lambda x: functools.reduce(
        lambda acc, b: (acc << 1) | b, ((x >> i) & 1 for i in range(64)), 0),
    ("bitset", "parity_u32"): lambda x: functools.reduce(
        lambda a, b: a ^ b, ((x >> i) & 1 for i in range(32))),
    ("bitset", "isolate_lowest_u64"): lambda x: x & -x & M64,
    ("bitset", "clear_lowest_u64"): lambda x: x & (x - 1),
    ("bitset", "mask_to_lowest_u64"): lambda x: M64 if x == 0 else (
        (bl := x & -x) | (bl - 1)),
    ("bitset", "funnel_shift_right_u64"): lambda hi, lo, s: (
        lo if s & 63 == 0 else (((hi << 64) | lo) >> (s & 63)) & M64),
    ("bitset", "funnel_shift_left_u64"): lambda hi, lo, s: (
        (((hi << 64) | lo) << (s & 63)) >> 64) & M64,
    ("bitset", "parity_u64_slice"): lambda a: functools.reduce(
        lambda p, q: p ^ q, a, 0).bit_count() & 1,
    ("bitset", "hamming_u64_slices"): lambda a, b: sum(
        (x ^ y).bit_count() for x, y in zip(a, b)),
    ("bitset", "any_bit_set_u64_slice"): lambda a: functools.reduce(
        lambda p, q: p | q, a, 0) != 0,
    ("bitset", "intersect_u64_slices"): lambda a, b: [
        x & y for x, y in zip(a, b)] + a[min(len(a), len(b)):],
    ("bitset", "union_u64_slices"): lambda a, b: [
        x | y for x, y in zip(a, b)] + a[min(len(a), len(b)):],
    ("bitset", "difference_u64_slices"): lambda a, b: [
        x & (M64 ^ y) for x, y in zip(a, b)] + a[min(len(a), len(b)):],
    ("bitset", "jaccard_u64_slices"): lambda a, b: (
        (lambda i, u: _f32(_f32(i) / _f32(u + (u == 0))))(
            sum((x & y).bit_count() for x, y in zip(a, b)),
            sum((x | y).bit_count() for x, y in zip(a, b)))),
    # --- swar ---------------------------------------------------------------
    ("swar", "byte_lane_in_range"): lambda w, lo, hi: (
        0 if hi > 0x7F else _b_lane_range(w, lo, hi)),
    ("swar", "find_byte_in_word"): _b_find_word,
    ("swar", "count_byte_in_word"): lambda w, b: bin(_b_find_word(w, b)).count("1"),
    ("swar", "has_byte_in_word"): lambda w, b: _b_find_word(w, b) != 0,
    ("swar", "first_byte_position"): lambda w, b: next(
        (i for i in range(8) if (w >> (8 * i)) & 0xFF == b), None),
    ("swar", "to_lower_ascii_word"): lambda w: w | (_b_find_word_range(w, 0x41, 0x5A) >> 2),
    ("swar", "to_upper_ascii_word"): lambda w: w & (M64 ^ (_b_find_word_range(w, 0x61, 0x7A) >> 2)),
    ("swar", "classify_bytes"): _b_classify,
    ("swar", "parse_8_decimal_digits"): _b_parse8,
    ("swar", "parse_4_hex_digits"): _b_parse4hex,
    ("swar", "find_byte_mask_word"): _b_find_word,
    ("swar", "find_first_of_word"): lambda w, b: next(
        (i for i in range(8) if (w >> (8 * i)) & 0xFF == b), 8),
    ("swar", "horizontal_sum_u8x8"): lambda w: sum(_b_lanes_shift(w)),
    ("swar", "horizontal_max_u8x8"): lambda w: max(_b_lanes_shift(w)),
    ("swar", "horizontal_min_u8x8"): lambda w: min(_b_lanes_shift(w)),
    ("swar", "horizontal_or_u8x8"): lambda w: functools.reduce(
        lambda p, q: p | q, _b_lanes_shift(w)),
    ("swar", "horizontal_and_u8x8"): lambda w: functools.reduce(
        lambda p, q: p & q, _b_lanes_shift(w)),
    ("swar", "horizontal_xor_u8x8"): lambda w: functools.reduce(
        lambda p, q: p ^ q, _b_lanes_shift(w)),
    ("swar", "horizontal_count_eq_u8"): lambda w, t: _b_lanes_shift(w).count(t),
    ("swar", "swar_find_all_positions"): lambda bs, t: sum(
        (1 << i for i, b in enumerate(bs[:64]) if b == t), 0),
    ("swar", "count_leading_eq_u8"): lambda bs, t: len(bs) - len(
        list(__import__("itertools").dropwhile(lambda b: b == t, bs))),
    ("swar", "is_all_ascii"): lambda bs: max(bs, default=0) < 0x80,
    ("swar", "skip_spaces"): lambda bs: len(bs) - len(bytes(bs).lstrip(b" ")),
    ("swar", "count_byte_in_slice"): lambda bs, t: functools.reduce(
        lambda n, b: n + (b == t), bs, 0),
    ("swar", "find_first_byte_in_slice"): lambda bs, t: (
        (lambda ix: None if ix < 0 else ix)(bytes(bs).find(bytes([t])))),
    # --- eytzinger ----------------------------------------------------------
    ("eytzinger", "build_eytzinger_layout"): _b_layout,
    ("eytzinger", "build_layout_permutation"): _b_perm,
    ("eytzinger", "layout_permutation_inverse"): _b_perm_inverse,
    ("eytzinger", "build_subtree_sizes"): _b_subtree_sizes,
    ("eytzinger", "search_eytzinger_u64"): _b_eyt_search,
    ("eytzinger", "lower_bound_eytzinger"): _b_eyt_lower_bound,
    # --- sort_small ---------------------------------------------------------
    ("sort_small", "sort2"): _b_rank_sort,
    ("sort_small", "sort4"): _b_rank_sort,
    ("sort_small", "sort5"): _b_rank_sort,
    ("sort_small", "sort6"): _b_rank_sort,
    ("sort_small", "sort7"): _b_rank_sort,
    ("sort_small", "sort8"): _b_rank_sort,
    ("sort_small", "sort_bytes_u64"): lambda v: int.from_bytes(
        bytes(_b_rank_sort(list(v.to_bytes(8, "little")))), "little"),
    ("sort_small", "sort4_lanes_u16"): lambda v: sum(
        lane << (16 * i) for i, lane in enumerate(_b_rank_sort(
            [(v >> (16 * i)) & 0xFFFF for i in range(4)]))),
    ("sort_small", "sort_small_by_key"): lambda keys: [
        keys[i] for _, i in sorted((k, i) for i, k in enumerate(keys))],
    # --- packing ------------------------------------------------------------
    ("packing", "field_deposit"): _b_field_deposit,
    ("packing", "field_extract"): _b_field_extract,
    ("packing", "elias_fano_split"): lambda v, w: (
        (w := w & 63,
         (v & ((1 << w) - 1)) | (1 << (w + min(v >> w, 63 - w))))[1]),
    ("packing", "delta_encode_u32x2"): lambda v, p: (
        (((v & M32) - (p & M32)) & M32) | ((((v >> 32) - (p >> 32)) & M32) << 32)),
    ("packing", "delta_decode_u32x2"): lambda d, p: (
        (((d & M32) + (p & M32)) & M32) | ((((d >> 32) + (p >> 32)) & M32) << 32)),
    ("packing", "transpose8x8"): lambda w: functools.reduce(
        lambda acc, ij: acc | (((w >> (ij[0] * 8 + ij[1])) & 1) << (ij[1] * 8 + ij[0])),
        [(i, j) for i in range(8) for j in range(8)], 0),
    ("packing", "transpose64x64_combined"): lambda a, b: functools.reduce(
        lambda acc, ij: acc | (((a ^ b >> 0) >> (ij[0] * 8 + ij[1])) & 1) << (ij[1] * 8 + ij[0]),
        [(i, j) for i in range(8) for j in range(8)], 0),
    ("packing", "delta_swap"): lambda w, m, s: (
        lambda sh, lo, hi: (w ^ lo ^ hi ^ ((lo ^ hi) << sh)) & M64)(
        s & 63, w & m, (w >> (s & 63)) & m),
    ("packing", "bit_swap"): lambda w, sel: (lambda i, j, bi, bj: (
        w & (M64 ^ (1 << i)) & (M64 ^ (1 << j))) | (bi << j) | (bj << i))(
        sel & 63, (sel >> 6) & 63, (w >> (sel & 63)) & 1, (w >> ((sel >> 6) & 63)) & 1),
    ("packing", "permute_step8"): lambda w, m: (lambda lo, hi: (
        w ^ lo ^ hi ^ ((lo ^ hi) << 8)) & M64)(w & m, (w >> 8) & m),
    ("packing", "benes_network"): lambda w, sel: functools.reduce(
        lambda s, sr: (lambda sh, rot: (lambda m: (lambda t: s ^ t ^ ((t << sh) & M64))(
            ((s >> sh) ^ s) & m))((sel << rot | (sel >> (64 - rot))) & M64 if rot else sel))(
            *sr),
        [(1, 11), (2, 23), (4, 31), (8, 43), (16, 53), (32, 53)], w),
    ("packing", "compress_bits"): _b_compress,
    ("packing", "deposit_bits"): _b_deposit,
}


def _b_lane_range(w, lo, hi):
    out = 0
    for i in range(8):
        b = (w >> (8 * i)) & 0xFF
        if lo <= b <= hi and b >> 7 == 0:
            out |= 0x80 << (8 * i)
    return out


def _b_find_word_range(w, lo, hi):
    return _b_lane_range(w, lo, hi)


def run_second(module: str, fn: str, vals: list):
    f = SECOND[(module, fn)]
    return f(*vals)


# ---------------------------------------------------------------------------
# tests
# ---------------------------------------------------------------------------

def test_dispatch_resolves(corpus):
    assert corpus, "empty corpus"
    for module, by_fn in corpus.items():
        for fn, cases in by_fn.items():
            assert cases, f"{module}.{fn}: no cases"
            # resolve through the dispatch path (raises on a broken entry)
            types = oracle.arg_types(module, fn)
            vals = dec_in(cases[0]["in"], types)
            got = run_oracle(module, fn, vals)
            assert got is not None or oracle.out_type(module, fn).startswith(
                ("opt", "bool")), f"{module}.{fn}: dispatch returned None unexpectedly"


def test_ref_status_reported():
    """Loud witness of which oracle table served the dispatch."""
    print(f"\ndispatch source: {REF_STATUS}")
    assert isinstance(REF_STATUS, str) and REF_STATUS


@pytest.mark.oracle_budget(ms=50)  # full-corpus differential: 7296 cases, ~17ms
def test_corpus_vs_oracle_coherence(corpus):
    failures = []
    checked = 0
    for module, by_fn in corpus.items():
        for fn, cases in by_fn.items():
            types = oracle.arg_types(module, fn)
            out_ty = oracle.out_type(module, fn)
            for case in cases:
                checked += 1
                vals = dec_in(case["in"], types)
                try:
                    got = run_oracle(module, fn, vals)
                except Exception as exc:  # noqa: BLE001 — reported per-case
                    failures.append(f"{case['id']}: dispatch raised {exc!r}")
                    continue
                want = dec_out(out_ty, case["expect"]["out"])
                if got != want:
                    failures.append(
                        f"{case['id']}: in={case['in']} oracle={got!r} "
                        f"pinned={want!r}")
                if len(failures) >= 8:
                    break
    assert not failures, (
        f"{len(failures)}+ corpus/oracle disagreements after {checked} cases:\n"
        + "\n".join(failures))


def test_boundary_second_formulation(corpus):
    failures = []
    checked = 0
    for module, by_fn in corpus.items():
        for fn, cases in by_fn.items():
            types = oracle.arg_types(module, fn)
            out_ty = oracle.out_type(module, fn)
            for case in cases:
                if case["id"].rsplit(".", 2)[-2] == "fuzz":
                    continue
                checked += 1
                vals = dec_in(case["in"], types)
                try:
                    got = run_second(module, fn, vals)
                except Exception as exc:  # noqa: BLE001
                    failures.append(f"{case['id']}: second formulation raised {exc!r}")
                    continue
                want = dec_out(out_ty, case["expect"]["out"])
                if got != want:
                    failures.append(
                        f"{case['id']}: in={case['in']} second={got!r} "
                        f"pinned={want!r}")
                if len(failures) >= 8:
                    break
    assert checked > 500, f"boundary/corner tier unexpectedly small: {checked}"
    assert not failures, (
        f"{len(failures)}+ second-formulation disagreements after {checked} cases:\n"
        + "\n".join(failures))

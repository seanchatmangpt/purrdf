"""Generates the pinned shared-vector corpus under vectors/.

Run from the oracles directory:

    python3 -m gen.gen_vectors

Walks a per-function case plan (a hand-picked boundary/corner sweep plus a
deterministic xorshift64 fuzz tier) over the formulation-A oracles in
``gen.oracle``, encodes every case as 0x-hex strings, and writes
``vectors/<module>.json`` plus a blake3-pinned ``vectors/manifest.json``.
"""

from __future__ import annotations

import json
from pathlib import Path

from gen import oracle

VECTORS = Path(__file__).resolve().parent.parent / "vectors"

SCHEMA = 1
SEEDS = {
    "bitset": 0x9E37_79B9_7F4A_7C15,
    "swar": 0xDEAD_BEEF_CAFE_F00D,
    "eytzinger": 0x0123_4567_89AB_CDEF,
    "sort_small": 0x5EED_1A7E_5EED_1A7E,
    "packing": 0x2545_F491_4F6C_DD1D,
}
# Dispatch cap: the whole corpus must stay under 2 MB on disk. The per-module
# fuzz tier is trimmed uniformly until the cap holds (the plan's 4096/module
# figure is the ceiling, not the contract; the cap wins).
FUZZ_PER_MODULE = 1300
MAX_CORPUS_BYTES = 2_000_000

M64 = oracle.M64
M32 = oracle.M32


class Xorshift:
    """xorshift64 (<<13, >>7, <<17) — the construction used by the Rust tests."""

    def __init__(self, seed: int):
        self.s = seed & M64

    def next(self) -> int:
        s = self.s
        s ^= (s << 13) & M64
        s ^= s >> 7
        s ^= (s << 17) & M64
        s &= M64
        self.s = s
        return s


# ---------------------------------------------------------------------------
# encoding
# ---------------------------------------------------------------------------

def enc_in(ty: str, v):
    if ty in ("u64s", "u32s", "u8s"):
        return [hex(x) for x in v]
    if ty == "bool":
        return hex(int(v))
    return hex(v)


def enc_out(ty: str, v):
    if ty in ("u64s", "u32s"):
        return [hex(x) for x in v]
    if ty == "bool":
        return bool(v)
    if ty == "opt_us":
        return {"some": hex(v)} if v is not None else {"none": None}
    if ty == "f32b":
        return {"bits_f32": oracle.f32_bits(v)}
    return hex(v)


def _corner(tys, vals) -> bool:
    for (_n, ty), v in zip(tys, vals):
        if ty in oracle.TYPE_MAX and v in (0, oracle.TYPE_MAX[ty]):
            return True
        if ty == "us" and v == 0:
            return True
        if ty in ("u64s", "u32s") and (not v or v[0] in (0, M64, M32)):
            return True
    return False


# ---------------------------------------------------------------------------
# boundary sweeps (hand-picked degenerate/adversarial inputs)
# ---------------------------------------------------------------------------

W = [0, 1, M64, 1 << 63, 0x8000_0000_0000_0001, 0xF0F0_F0F0_F0F0_F0F0]
POSW = [0, 1, 7, 31, 62, 63, 64, 100]
ALPHA = [0, 1, M64, 0x0123_4567_89AB_CDEF]

BOUNDARY: dict[str, dict[str, list[tuple]]] = {
    "bitset": {
        "set_bit_u64": [(w, p) for w in [0, M64, 1 << 63] for p in POSW[:7]],
        "clear_bit_u64": [(w, p) for w in [0, M64, 1 << 63] for p in POSW[:7]],
        "rank_u64": [(w, p) for w in W[:4] for p in POSW[:6]],
        "rank_exclusive_u64": [(w, p) for w in W[:4] for p in POSW[:6]],
        "select_bit_u64": [(w, n) for w in W[:5] for n in [0, 1, 3, 63, 64, 70]],
        "xor_u64": [(a, b) for a in W[:4] for b in [0, M64, 0x0F0F_F0F0_F0F0_F0F0]],
        "not_u64": [(w,) for w in W],
        "and_not_u64": [(a, b) for a in W[:4] for b in [0, M64, 0b0101]],
        "intersect_u64": [(a, b) for a in W[:3] for b in [0, M64, 0xF0F0_F0F0_F0F0_F0F0]],
        "union_u64": [(a, b) for a in W[:3] for b in [0, M64, 0xF0F0_F0F0_F0F0_F0F0]],
        "is_subset_u64": [(x, y) for x, y in [(0, 0), (0, M64), (0b0100, 0b0111),
                                              (0b1000, 0b0111), (M64, M64), (M64, 0)]],
        "popcount_u64": [(w,) for w in W],
        "reverse_bits_u64": [(w,) for w in W],
        "parity_u32": [(x,) for x in [0, 1, M32, 0xFFFF, 0x7FFF, 0xAAAA_AAAA]],
        "isolate_lowest_u64": [(w,) for w in W],
        "clear_lowest_u64": [(w,) for w in W],
        "mask_to_lowest_u64": [(w,) for w in W],
        "funnel_shift_right_u64": [(h, l, s) for h in ALPHA for l in [0, M64]
                                   for s in [0, 1, 63, 64, 127]],
        "funnel_shift_left_u64": [(h, l, s) for h in ALPHA for l in [0, M64]
                                  for s in [0, 1, 63, 64, 127]],
        "parity_u64_slice": [(a,) for a in [[], [0], [M64], [0xF0F0, 0x0F0F], [1, 1]]],
        "hamming_u64_slices": [(a, b) for a, b in [([], []), ([M64], [0]), ([0b1100], [0b0011]),
                                                   ([1, 2], [3, 2]), ([1], [1, 2])]],
        "any_bit_set_u64_slice": [(a,) for a in [[], [0, 0], [0, 1], [M64]]],
        "intersect_u64_slices": [(a, b) for a, b in [([], []), ([0b1111, 0b1010], [0b0101, 0b1100]),
                                                     ([M64], []), ([M64], [0])]],
        "union_u64_slices": [(a, b) for a, b in [([], []), ([0b0101, 0b1010], [0b1010, 0b0101]),
                                                 ([0], [M64])]],
        "difference_u64_slices": [(a, b) for a, b in [([], []), ([0b0111], [0b0101]),
                                                      ([M64, 0], [0, M64])]],
        "jaccard_u64_slices": [(a, b) for a, b in [([], []), ([0xFF], [0xFF]),
                                                   ([0b1100], [0b0011]), ([M64, 0], [0, M64]),
                                                   ([0b1110], [0b0111])]],
    },
    "swar": {
        "byte_lane_in_range": [
            (int.from_bytes(bs, "little"), lo, hi)
            for bs, lo, hi in [(b"1A2f", 0x30, 0x39), (b"A5 z   ", 0x41, 0x5A),
                               (b"A5 z   ", 0x61, 0x7A), (b" \t\r\n", 0x09, 0x0D),
                               (b"aA0: ~", 0x30, 0x39), (b"\x80\x7f", 0, 0x7F)]],
        "find_byte_in_word": [(w, b) for w in [int.from_bytes(b"hello wo", "little"),
                                               int.from_bytes(b"abcabcab", "little"), 0, M64]
                              for b in [ord("l"), ord("o"), ord("z"), 0, 0xFF]],
        "count_byte_in_word": [(w, b) for w in [int.from_bytes(b"hello wo", "little"), 0, M64]
                               for b in [ord("l"), 0, 0xFF]],
        "has_byte_in_word": [(w, b) for w in [int.from_bytes(b"hello wo", "little"), 0]
                             for b in [ord("o"), ord("z"), 0]],
        "first_byte_position": [(w, b) for w in [int.from_bytes(b"hello wo", "little"), 0, M64]
                                for b in [ord("h"), ord("o"), ord("z")]],
        "to_lower_ascii_word": [(w,) for w in [int.from_bytes(b"HELLO WO", "little"),
                                               int.from_bytes(b"aBcDeFgH", "little"),
                                               0, M64]],
        "to_upper_ascii_word": [(w,) for w in [int.from_bytes(b"hello wo", "little"),
                                               int.from_bytes(b"aBcDeFgH", "little"),
                                               0, M64]],
        "classify_bytes": [(w,) for w in [int.from_bytes(b"A5 z    ", "little"),
                                          int.from_bytes(b"\t\n 0aZ", "little"),
                                          0, M64]],
        "parse_8_decimal_digits": [(int.from_bytes(bs, "little"),) for bs in
                                   [b"12345678", b"00000000", b"99999999", b"1234567X",
                                    b"/2345678", b":0000000", b"0"*7 + b"9"]],
        "parse_4_hex_digits": [(int.from_bytes(bs, "little"),) for bs in
                               [b"1A2f", b"0000", b"FFFF", b"ffff", b"dead", b"BEEF",
                                b"1G2f", b"1a2g", b"1a:2", b"\x00\x00\x00\x00"]],
        "find_byte_mask_word": [(w, b) for w in [int.from_bytes(b"abcabcab", "little"),
                                                 int.from_bytes(b"aaaaaaaa", "little"), 0]
                                for b in [ord("a"), ord("c"), ord("z")]],
        "find_first_of_word": [(w, b) for w in [int.from_bytes(b"abcabcab", "little"), 0, M64]
                               for b in [ord("c"), ord("z"), 0]],
        "horizontal_sum_u8x8": [(w,) for w in [0, M64,
                                               int.from_bytes([3, 1, 4, 1, 5, 9, 2, 6], "little"),
                                               0x8080_8080_8080_8080]],
        "horizontal_max_u8x8": [(w,) for w in [0, M64,
                                               int.from_bytes(
                                                   [0x80, 0x7F, 1, 2, 3, 4, 5, 6], "little"),
                                               0x8080_8080_8080_8080]],
        "horizontal_min_u8x8": [(w,) for w in [0, M64,
                                               int.from_bytes(
                                                   [0x80, 0x7F, 1, 2, 3, 4, 5, 6], "little")]],
        "horizontal_or_u8x8": [(w,) for w in [0, M64, 0x0F1F_2F3F_4F5F_6F7F]],
        "horizontal_and_u8x8": [(w,) for w in [0, M64, 0x0F1F_2F3F_4F5F_6F7F]],
        "horizontal_xor_u8x8": [(w,) for w in [0, M64, 0x0F1F_2F3F_4F5F_6F7F]],
        "horizontal_count_eq_u8": [(w, t) for w in [0, M64,
                                                    int.from_bytes([3, 1, 4, 1, 5, 9, 2, 6],
                                                                   "little")]
                                   for t in [0, 1, 0x7F, 0x80, 0xFF]],
        "swar_find_all_positions": [(bs, t) for bs, t in
                                    [(b"hello world, hello!", ord("l")),
                                     (b"a" * 64, ord("a")),
                                     (b"a" * 65, ord("a")),
                                     (b"", ord("x")),
                                     (b"\x00" * 9, 0)]],
        "count_leading_eq_u8": [(bs, t) for bs, t in [(b"aaabcd", ord("a")), (b"xxxx", ord("x")),
                                                      (b"", ord("x")), (b"aaaaaaaab", ord("a")),
                                                      (b"b", ord("a"))]],
        "is_all_ascii": [(bs,) for bs in [b"hello world", b"caf\xe9", b"", b"\x7f", b"\x80"]],
        "skip_spaces": [(bs,) for bs in [b"   hello", b"hello", b"  ", b"", b" " * 20 + b"x"]],
        "count_byte_in_slice": [(bs, t) for bs, t in [(b"hello world, hello!", ord("l")),
                                                      (b"", ord("l")), (b"\x00" * 8, 0),
                                                      (b"a" * 64 + b"a", ord("a"))]],
        "find_first_byte_in_slice": [(bs, t) for bs, t in [(b"hello world, hello!", ord("o")),
                                                           (b"hello", ord("z")), (b"", ord("a")),
                                                           (b"\x00" * 8 + b"x", ord("x"))]],
    },
    "eytzinger": {
        "build_eytzinger_layout": [(src,) for src in
                                   [[], [42], [10, 20], [1, 2, 3, 4], [1, 2, 3, 4, 5, 6],
                                    [1, 2, 3, 4, 5, 6, 7], [5, 5, 5, 9, 9, 12, 30],
                                    [100, 250, 400, 550, 700, 850, 1000]]],
        "build_layout_permutation": [(src,) for src in
                                     [[], [42], [10, 20], [1, 2, 3, 4], [1, 2, 3, 4, 5, 6],
                                      [1, 2, 3, 4, 5, 6, 7], [5, 5, 5, 9, 9, 12, 30],
                                      [100, 250, 400, 550, 700, 850, 1000]]],
        "layout_permutation_inverse": [(perm,) for perm in
                                       [oracle._perm(src) for src in
                                        [[], [42], [10, 20], [1, 2, 3, 4],
                                         [1, 2, 3, 4, 5, 6], [1, 2, 3, 4, 5, 6, 7],
                                         [5, 5, 5, 9, 9, 12, 30],
                                         [100, 250, 400, 550, 700, 850, 1000]]]],
        "build_subtree_sizes": [((n + 1),) for n in range(0, 8)] + [((16 + 1),), ((33 + 1),)],
        "search_eytzinger_u64": [
            (oracle._layout(src), t)
            for src, targets in [
                ([42], [0, 41, 42, 43]),
                ([10, 20, 30], [0, 10, 25, 30, 31, M64]),
                ([1, 2, 3, 4, 5, 6, 7], [0, 1, 4, 7, 8, M64]),
                (list(range(1, 13)), [0, 6, 12, 13, M64]),
                (list(range(1, 16)), [0, 8, 15, 16, M64]),
            ] for t in targets],
        "lower_bound_eytzinger": [
            (oracle._layout(src), oracle._subtree_sizes(len(src) + 1), t)
            for src, targets in [
                ([5, 5, 5, 9, 9, 12, 30], [0, 4, 5, 6, 9, 10, 12, 13, 30, 31, M64]),
                ([10, 20, 30, 40, 50], [1, 10, 11, 50, 51]),
                ([1, 2, 3, 4, 5, 6], [0, 6, 7]),
                ([], [0]),
            ] for t in targets],
    },
    "sort_small": {
        "sort2": [([1, 2],), ([2, 1],), ([0, M32],), ([M32, 0],), ([7, 7],)],
        "sort4": [([1, 2, 3, 4],), ([4, 3, 2, 1],), ([5] * 4,),
                  ([0, M32, 7, M32],), ([0] * 4,)],
        "sort5": [([1, 2, 3, 4, 5],), ([5, 3, 1, 4, 2],), ([9] * 5,),
                  ([0, M32, 0, M32, 1],)],
        "sort6": [([1, 2, 3, 4, 5, 6],), ([6, 5, 4, 3, 2, 1],), ([5] * 6,),
                  ([0, M32, 7, 7, 0, M32],)],
        "sort7": [([1, 2, 3, 4, 5, 6, 7],), ([7, 6, 5, 4, 3, 2, 1],), ([3] * 7,),
                  ([0, M32, 1, M32 - 1, 0, 1, M32],)],
        "sort8": [([8, 3, 6, 1, 7, 2, 5, 4],), ([1, 2, 3, 4, 5, 6, 7, 8],),
                  ([9] * 8,), ([M32, 0, M32, 0, M32 // 2, 1, M32 - 1, 2],),
                  ([0] * 8,)],
        "sort_bytes_u64": [(0x0102_0304_0506_0708,), (0x0807_0605_0403_0201,), (0,),
                           (M64,), (0x00_FF_00_7F_80_01_02_03,)],
        "sort4_lanes_u16": [(1 | 2 << 16 | 3 << 32 | 4 << 48,),
                            (4 | 3 << 16 | 2 << 32 | 1 << 48,),
                            (0x2222_2222_2222_2222,),
                            (0xFFFF | 0x8000 << 32 | 1 << 48,), (0,)],
        "sort_small_by_key": [([],), ([5],), ([3, 1],), ([7] * 8,),
                              ([0, (1 << 56) - 1, (1 << 56) - 2, 1, 0, (1 << 56) - 1, 5, 0],),
                              ([4, 4, 1, 1, 4, 1],)],
    },
    "packing": {
        "field_deposit": [(0x1234_5678_9ABC_DEF0, v, k, o)
                          for v in [0, 1, M64, 0xDEAD_BEEF]
                          for k, o in [(1, 0), (32, 0), (8, 15), (0, 31), (33, 31), (100, 63)]],
        "field_extract": [(0x1234_5678_9ABC_DEF0, k, o)
                          for k, o in [(1, 0), (32, 0), (8, 15), (0, 31), (33, 31), (100, 63)]],
        "elias_fano_split": [(v, w) for v in [0, 1, M64, 0x0123_4567_89AB_CDEF]
                             for w in [0, 1, 7, 32, 63, 64]],
        "delta_encode_u32x2": [(v, p) for v in [0, 1, M64, 0xA | (0x14 << 32)]
                               for p in [0, 5 | (3 << 32), M64]],
        "delta_decode_u32x2": [(v, p) for v in [0, 1, M64, 0xA | (0x14 << 32)]
                               for p in [0, 5 | (3 << 32), M64]],
        "transpose8x8": [(w,) for w in [0, 1, M64, 0x0F0F_0F0F_F0F0_F0F0,
                                        0x8000_0000_0000_0001, 0xA5A5_A5A5_5A5A_5A5A]],
        "transpose64x64_combined": [(a, b) for a in [0, 0xA5A5_A5A5_5A5A_5A5A, M64]
                                    for b in [0, M64]],
        "delta_swap": [(w, m, s) for w in [0, 1, M64, 0xDEAD_BEEF]
                       for m, s in [(0, 0), (M64, 0), (M64, 1), (M64, 7),
                                    (0xAAAA_AAAA_AAAA_AAAA, 31), (1, 63)]],
        "bit_swap": [(w, sel) for w in [0, 1, M64, 0xF0F0_F0F0_F0F0_F0F0]
                     for sel in [0, 1, 63, 64, 0x3F | 0x3F << 6, 5]],
        "permute_step8": [(w, m) for w in [0, 1, M64, 0xDEAD_BEEF]
                          for m in [0, M64, 0x00FF_00FF_00FF_00FF]],
        "benes_network": [(w, sel) for w in [0, 1, M64, 0x0123_4567_89AB_CDEF]
                          for sel in [0, M64, 0xA5A5_5A5A_A5A5_5A5A]],
        "compress_bits": [(v, m) for v in [0, 1, M64, 0x0123_4567_89AB_CDEF]
                          for m in [0, 1, M64, 0b1011_0101_1100_0011]],
        "deposit_bits": [(v, m) for v in [0, 1, M64, 0x0123_4567_89AB_CDEF]
                         for m in [0, 1, M64, 0b1011_0101_1100_0011]],
    },
}


# byte-slice boundary entries are authored as bytes literals; normalize to
# lists of ints (the "u8s" encoding) before use.
for _fn, _entries in BOUNDARY["swar"].items():
    _tys = [ty for _n, ty in oracle.arg_types("swar", _fn)]
    BOUNDARY["swar"][_fn] = [
        tuple(list(v) if ty == "u8s" and isinstance(v, (bytes, bytearray)) else v
              for v, ty in zip(vals, _tys))
        for vals in _entries
    ]


# ---------------------------------------------------------------------------
# fuzz generators: fn-name suffix -> callable(rng) -> arg tuple
# keyed by (module, fn); falls back to type-driven defaults.
# ---------------------------------------------------------------------------

def _fz_u64(rng):
    return rng.next()


def _fz_word(rng):
    r = rng.next()
    kind = r % 4
    if kind == 0:
        return 0
    if kind == 1:
        return M64
    if kind == 2:
        return rng.next() & 0x00FF_00FF_00FF_00FF
    return rng.next()


def _fz_byte(rng):
    r = rng.next()
    if r % 8 == 0:
        return [0, 0x20, 0x7F, 0x80, 0xFF][r % 5]
    return (r >> 8) & 0xFF


def _fz_us(rng):
    r = rng.next()
    if r % 4 == 0:
        return [0, 1, 63, 64, 100][r % 5]
    return (r >> 3) % 130


def _fz_bytes(rng):
    n = rng.next() % 10
    if rng.next() % 8 == 0:
        n = [0, 7, 8, 9, 64, 65, 70][rng.next() % 7]
    pool = [0x20, 0x61, 0x62, 0x00, 0xFF, 0x7F]
    return [(pool[rng.next() % 6] if rng.next() % 3 else rng.next() & 0xFF)
            for _ in range(n)]


def _fz_u64s(rng, max_len=5, masked=False):
    n = rng.next() % (max_len + 1)
    out = []
    for _ in range(n):
        w = _fz_word(rng) if not masked else rng.next() & 0xFF
        out.append(w)
    return out


def _fz_sorted(rng, distinct):
    n = rng.next() % 15
    cur = rng.next() % 3
    out = []
    for _ in range(n):
        cur += rng.next() % 4 + (1 if distinct else 0)
        out.append(cur)
    return out


def _fz_eyt(rng):
    return oracle._layout(_fz_sorted(rng, True))


def _fz_sizes(rng):
    n = rng.next() % 15
    return oracle._subtree_sizes(n + 1)


FUZZERS: dict[tuple[str, str], list] = {
    # name -> list of arg-maker callables (aligned with the spec's arg order)
    ("bitset", "set_bit_u64"): [_fz_word, _fz_us],
    ("bitset", "clear_bit_u64"): [_fz_word, _fz_us],
    ("bitset", "rank_u64"): [_fz_word, _fz_us],
    ("bitset", "rank_exclusive_u64"): [_fz_word, _fz_us],
    ("bitset", "select_bit_u64"): [_fz_word, _fz_us],
    ("bitset", "xor_u64"): [_fz_word, _fz_word],
    ("bitset", "not_u64"): [_fz_word],
    ("bitset", "and_not_u64"): [_fz_word, _fz_word],
    ("bitset", "intersect_u64"): [_fz_word, _fz_word],
    ("bitset", "union_u64"): [_fz_word, _fz_word],
    ("bitset", "is_subset_u64"): [_fz_word, _fz_word],
    ("bitset", "popcount_u64"): [_fz_word],
    ("bitset", "reverse_bits_u64"): [_fz_word],
    ("bitset", "parity_u32"): [lambda rng: rng.next()],
    ("bitset", "isolate_lowest_u64"): [_fz_word],
    ("bitset", "clear_lowest_u64"): [_fz_word],
    ("bitset", "mask_to_lowest_u64"): [_fz_word],
    ("bitset", "funnel_shift_right_u64"): [_fz_word, _fz_word,
                                           lambda rng: rng.next()],
    ("bitset", "funnel_shift_left_u64"): [_fz_word, _fz_word,
                                          lambda rng: rng.next()],
    ("bitset", "parity_u64_slice"): [lambda rng: _fz_u64s(rng)],
    ("bitset", "hamming_u64_slices"): [lambda rng: _fz_u64s(rng),
                                       lambda rng: _fz_u64s(rng)],
    ("bitset", "any_bit_set_u64_slice"): [lambda rng: _fz_u64s(rng)],
    ("bitset", "intersect_u64_slices"): [lambda rng: _fz_u64s(rng, 6),
                                         lambda rng: _fz_u64s(rng, 4)],
    ("bitset", "union_u64_slices"): [lambda rng: _fz_u64s(rng, 6),
                                     lambda rng: _fz_u64s(rng, 4)],
    ("bitset", "difference_u64_slices"): [lambda rng: _fz_u64s(rng, 6),
                                          lambda rng: _fz_u64s(rng, 4)],
    ("bitset", "jaccard_u64_slices"): [lambda rng: _fz_u64s(rng, 6),
                                       lambda rng: _fz_u64s(rng, 4)],
    ("swar", "byte_lane_in_range"): [_fz_word, _fz_byte, _fz_byte],
    ("swar", "find_byte_in_word"): [_fz_word, _fz_byte],
    ("swar", "count_byte_in_word"): [_fz_word, _fz_byte],
    ("swar", "has_byte_in_word"): [_fz_word, _fz_byte],
    ("swar", "first_byte_position"): [_fz_word, _fz_byte],
    ("swar", "to_lower_ascii_word"): [_fz_word],
    ("swar", "to_upper_ascii_word"): [_fz_word],
    ("swar", "classify_bytes"): [_fz_word],
    ("swar", "parse_8_decimal_digits"): [lambda rng: int.from_bytes(
        bytes((rng.next() % 10 + 0x30 if rng.next() % 3 else rng.next() & 0xFF)
              for _ in range(8)), "little")],
    ("swar", "parse_4_hex_digits"): [lambda rng: int.from_bytes(
        bytes((b"0123456789abcdefABCDEFgh"[rng.next() % 22]
               if rng.next() % 2 else rng.next() & 0xFF) for _ in range(4)), "little")],
    ("swar", "find_byte_mask_word"): [_fz_word, _fz_byte],
    ("swar", "find_first_of_word"): [_fz_word, _fz_byte],
    ("swar", "horizontal_sum_u8x8"): [_fz_word],
    ("swar", "horizontal_max_u8x8"): [_fz_word],
    ("swar", "horizontal_min_u8x8"): [_fz_word],
    ("swar", "horizontal_or_u8x8"): [_fz_word],
    ("swar", "horizontal_and_u8x8"): [_fz_word],
    ("swar", "horizontal_xor_u8x8"): [_fz_word],
    ("swar", "horizontal_count_eq_u8"): [_fz_word, _fz_byte],
    ("swar", "swar_find_all_positions"): [_fz_bytes, _fz_byte],
    ("swar", "count_leading_eq_u8"): [_fz_bytes, _fz_byte],
    ("swar", "is_all_ascii"): [_fz_bytes],
    ("swar", "skip_spaces"): [_fz_bytes],
    ("swar", "count_byte_in_slice"): [_fz_bytes, _fz_byte],
    ("swar", "find_first_byte_in_slice"): [_fz_bytes, _fz_byte],
    ("eytzinger", "build_eytzinger_layout"): [lambda rng: _fz_sorted(rng, False)],
    ("eytzinger", "build_layout_permutation"): [lambda rng: _fz_sorted(rng, False)],
    ("eytzinger", "layout_permutation_inverse"):
        [lambda rng: oracle._perm(_fz_sorted(rng, False))],
    ("eytzinger", "build_subtree_sizes"): [lambda rng: rng.next() % 16 + 1],
    ("eytzinger", "search_eytzinger_u64"): [_fz_eyt, lambda rng: rng.next() % 64],
    ("eytzinger", "lower_bound_eytzinger"): [_fz_eyt, _fz_sizes,
                                             lambda rng: rng.next() % 64],
    ("sort_small", "sort2"): [lambda rng: [rng.next() for _ in range(2)]],
    ("sort_small", "sort4"): [lambda rng: [rng.next() for _ in range(4)]],
    ("sort_small", "sort5"): [lambda rng: [rng.next() for _ in range(5)]],
    ("sort_small", "sort6"): [lambda rng: [rng.next() for _ in range(6)]],
    ("sort_small", "sort7"): [lambda rng: [rng.next() for _ in range(7)]],
    ("sort_small", "sort8"): [lambda rng: [rng.next() for _ in range(8)]],
    ("sort_small", "sort_bytes_u64"): [_fz_word],
    ("sort_small", "sort4_lanes_u16"): [_fz_word],
    ("sort_small", "sort_small_by_key"): [lambda rng: [rng.next() & ((1 << 56) - 1)
                                                       for _ in range(rng.next() % 9)]],
    ("packing", "field_deposit"): [_fz_word, _fz_word,
                                   lambda rng: rng.next() % 40,
                                   lambda rng: rng.next() % 40],
    ("packing", "field_extract"): [_fz_word, lambda rng: rng.next() % 40,
                                   lambda rng: rng.next() % 40],
    ("packing", "elias_fano_split"): [_fz_word, lambda rng: rng.next() % 70],
    ("packing", "delta_encode_u32x2"): [_fz_word, _fz_word],
    ("packing", "delta_decode_u32x2"): [_fz_word, _fz_word],
    ("packing", "transpose8x8"): [_fz_word],
    ("packing", "transpose64x64_combined"): [_fz_word, _fz_word],
    ("packing", "delta_swap"): [_fz_word, _fz_word, lambda rng: rng.next()],
    ("packing", "bit_swap"): [_fz_word, _fz_word],
    ("packing", "permute_step8"): [_fz_word, _fz_word],
    ("packing", "benes_network"): [_fz_word, _fz_word],
    ("packing", "compress_bits"): [_fz_word, _fz_word],
    ("packing", "deposit_bits"): [_fz_word, _fz_word],
}


def fuzz_args(module, fn, rng):
    makers = FUZZERS.get((module, fn))
    types = oracle.arg_types(module, fn)
    if makers is None:
        makers = []
        for _name, ty in types:
            if ty == "u64":
                makers.append(_fz_word)
            elif ty == "u32":
                makers.append(lambda r: r.next())
            elif ty == "u8":
                makers.append(_fz_byte)
            elif ty == "us":
                makers.append(_fz_us)
            elif ty == "u64s":
                makers.append(lambda r: _fz_u64s(r))
            elif ty == "u32s":
                makers.append(lambda r: [r.next() & M32 for _ in range(r.next() % 5)])
            else:  # pragma: no cover
                raise ValueError(f"no fuzzer for {ty}")
    vals = [m(rng) for m in makers]
    assert len(vals) == len(types), (module, fn)
    return vals


def mask_vals(module, fn, vals):
    types = oracle.arg_types(module, fn)
    out = []
    for (name, ty), v in zip(types, vals):
        if ty == "u64":
            out.append(v & M64)
        elif ty == "u32":
            out.append(v & M32)
        elif ty == "u8":
            out.append(v & 0xFF)
        elif ty == "us":
            out.append(v)
        elif ty == "bool":
            out.append(bool(v & 1))
        else:
            out.append(v)
    return out


def generate():
    VECTORS.mkdir(parents=True, exist_ok=True)
    manifest_files = {}
    for module in ["bitset", "swar", "eytzinger", "sort_small", "packing"]:
        rng = Xorshift(SEEDS[module])
        cases = []
        counters: dict[tuple[str, str], int] = {}
        specs = oracle.SPECS[module]

        def emit(fn, vals, kind):
            vals = mask_vals(module, fn, vals)
            args = {name: enc_in(ty, v) for (name, ty), v in zip(
                oracle.arg_types(module, fn), vals)}
            got = oracle.call(module, fn, {name: v for (name, _), v in zip(
                oracle.arg_types(module, fn), vals)})
            key = (fn, kind)
            i = counters.get(key, 0)
            counters[key] = i + 1
            cases.append({
                "id": f"{module}.{fn}.{kind}.{i}",
                "fn": fn,
                "in": args,
                "expect": {"out": enc_out(oracle.out_type(module, fn), got)},
            })

        for fn in specs:
            for vals in BOUNDARY[module][fn]:
                tys = oracle.arg_types(module, fn)
                emit(fn, vals, "corner" if _corner(tys, vals) else "boundary")
        # fuzz tier: round-robin over the module's fns until the budget is spent
        fn_names = list(specs)
        made = 0
        while made < FUZZ_PER_MODULE:
            for fn in fn_names:
                if made >= FUZZ_PER_MODULE:
                    break
                vals = fuzz_args(module, fn, rng)
                emit(fn, vals, "fuzz")
                made += 1
        doc = {
            "schema": SCHEMA,
            "module": module,
            "generator": {
                "xorshift_seed": SEEDS[module],
                "boundary": True,
                "n_fuzz": FUZZ_PER_MODULE,
            },
            "cases": cases,
        }
        path = VECTORS / f"{module}.json"
        text = json.dumps(doc, sort_keys=True, indent=1) + "\n"
        path.write_text(text)
        import blake3
        digest = blake3.blake3(path.read_bytes()).hexdigest()
        manifest_files[module] = {"blake3": digest, "cases": len(cases)}
    manifest = {"schema": SCHEMA, "files": dict(sorted(manifest_files.items()))}
    (VECTORS / "manifest.json").write_text(
        json.dumps(manifest, sort_keys=True, indent=1) + "\n")


def main():
    generate()
    total = sum(m["cases"] for m in json.loads(
        (VECTORS / "manifest.json").read_text())["files"].values())
    size = sum(p.stat().st_size for p in VECTORS.glob("*.json"))
    print(f"modules=5 cases={total} bytes={size} cap={MAX_CORPUS_BYTES}")


if __name__ == "__main__":
    main()

"""Pure-Python reference oracles for the branchless kernel corpus.

Formulation A (generator side): every implementation here is an
independent, loop-based restatement of the corresponding Rust kernel in
``crates/branchless/src/{bitset,swar,eytzinger,sort_small,packing}.rs`` —
scalar per-bit / per-byte / std-builtin Python loops, never a transcription
of the SWAR or network forms. test_kernels.py carries a second formulation
for the boundary tier, so generation and re-derivation never share code.

Types used in the spec table:
  u64, u32, u8, us (usize), bool        scalar int args (hex-encoded)
  u64s, u32s                            slices / fixed arrays (hex lists)
  opt_us                                Option<usize>
  f32b                                  f32 carried as raw bits
"""

from __future__ import annotations

import functools
import itertools
import struct

M64 = (1 << 64) - 1
M32 = (1 << 32) - 1

TYPE_MAX = {"u64": M64, "u32": M32, "u8": 255}


# ---------------------------------------------------------------------------
# bitset
# ---------------------------------------------------------------------------

def _set_bit(x, pos):
    return x | (1 << (pos & 63))


def _clear_bit(x, pos):
    return x & (M64 ^ (1 << (pos & 63)))


def _rank(x, pos):
    p = min(pos, 63)
    return sum((x >> b) & 1 for b in range(p + 1))


def _rank_excl(x, pos):
    p = pos & 63
    return sum((x >> b) & 1 for b in range(p))


def _select(x, n):
    seen = 0
    for b in range(64):
        seen += (x >> b) & 1
        if seen == n + 1:
            return b
    return None


def _is_subset(x, y):
    return 1 if (x & (M64 ^ y)) == 0 else 0


def _funnel_right(hi, lo, s):
    s &= 63
    if s == 0:
        return lo
    return (((hi << 64) | lo) >> s) & M64


def _funnel_left(hi, lo, s):
    s &= 63
    cat = (hi << 64) | lo
    return ((cat << s) >> 64) & M64


def _isolate_lowest(x):
    for b in range(64):
        if (x >> b) & 1:
            return 1 << b
    return 0


def _clear_lowest(x):
    for b in range(64):
        if (x >> b) & 1:
            return x & (M64 ^ (1 << b))
    return 0


def _mask_to_lowest(x):
    for b in range(64):
        if (x >> b) & 1:
            return (1 << (b + 1)) - 1
    return M64


def _f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def _jaccard(a, b):
    # the Rust kernel divides in single precision: round operands and the
    # quotient through f32 (innocuous double rounding for 53 >= 2*24+2)
    n = min(len(a), len(b))
    inter = uni = 0
    for i in range(n):
        inter += bin(a[i] & b[i]).count("1")
        uni += bin(a[i] | b[i]).count("1")
    return _f32(_f32(inter) / _f32(uni + (1 if uni == 0 else 0)))


def _slices_intersect(a, b):
    return [a[i] & b[i] for i in range(min(len(a), len(b)))] + list(a[min(len(a), len(b)):])


def _slices_union(a, b):
    return [a[i] | b[i] for i in range(min(len(a), len(b)))] + list(a[min(len(a), len(b)):])


def _slices_difference(a, b):
    return [a[i] & (M64 ^ b[i]) for i in range(min(len(a), len(b)))] + list(a[min(len(a), len(b)):])


# ---------------------------------------------------------------------------
# swar
# ---------------------------------------------------------------------------

def _lanes(word):
    return list(word.to_bytes(8, "little"))


def _lane_in_range(word, lo, hi):
    mask = 0
    for i, b in enumerate(_lanes(word)):
        if lo <= b <= hi and b >> 7 == 0:
            mask |= 0x80 << (8 * i)
    return mask


def _find_byte_word(word, byte):
    mask = 0
    for i, b in enumerate(_lanes(word)):
        if b == byte:
            mask |= 0x80 << (8 * i)
    return mask


def _classify(word):
    out = 0
    for i, b in enumerate(_lanes(word)):
        c = 0
        if 0x30 <= b <= 0x39:
            c |= 0x01
        if 0x41 <= b <= 0x5A or 0x61 <= b <= 0x7A:
            c |= 0x02
        if 0x09 <= b <= 0x0D or b == 0x20:
            c |= 0x04
        if 0x41 <= b <= 0x5A:
            c |= 0x08
        if 0x61 <= b <= 0x7A:
            c |= 0x10
        out |= c << (8 * i)
    return out


def _parse8(word):
    digits = [_lanes(word)[i] - 0x30 for i in range(8)]
    if any(d > 9 or d < 0 for d in digits):
        return None
    v = 0
    for d in digits:
        v = v * 10 + d
    return v


def _parse4_hex(word):
    bs = [(word >> (8 * i)) & 0xFF for i in range(4)]
    vals = []
    for b in bs:
        if 0x30 <= b <= 0x39:
            vals.append(b - 0x30)
        elif 0x41 <= b <= 0x46:
            vals.append(b - 0x41 + 10)
        elif 0x61 <= b <= 0x66:
            vals.append(b - 0x61 + 10)
        else:
            return None
    return (vals[0] << 12) | (vals[1] << 8) | (vals[2] << 4) | vals[3]


def _find_all_positions(bs, target):
    mask = 0
    for i, b in enumerate(bs[:64]):
        if b == target:
            mask |= 1 << i
    return mask


def _count_leading(bs, target):
    n = 0
    for b in bs:
        if b != target:
            break
        n += 1
    return n


# ---------------------------------------------------------------------------
# eytzinger
# ---------------------------------------------------------------------------

def _layout(src):
    dest = [0] * (len(src) + 1)
    cursor = 0

    def rec(k):
        nonlocal cursor
        if k < len(dest):
            rec(2 * k)
            dest[k] = src[cursor]
            cursor += 1
            rec(2 * k + 1)

    rec(1)
    return dest


def _perm(src):
    perm = [0] * (len(src) + 1)
    cursor = 0

    def rec(k):
        nonlocal cursor
        if k < len(perm):
            rec(2 * k)
            perm[k] = cursor
            cursor += 1
            rec(2 * k + 1)

    rec(1)
    return perm


def _perm_inverse(perm):
    out = [0] * len(perm)
    for slot in range(1, len(perm)):
        out[perm[slot]] = slot
    return out


def _subtree_sizes(length):
    sizes = [0] * length
    n = length - 1
    for k in range(n, 0, -1):
        l = sizes[2 * k] if 2 * k < length else 0
        r = sizes[2 * k + 1] if 2 * k + 1 < length else 0
        sizes[k] = 1 + l + r
    return sizes


def _eyt_search(eyt, target):
    n = len(eyt)
    if n <= 1:
        return None
    for slot in range(1, n):
        if eyt[slot] == target:
            return slot
    return None


def _eyt_lower_bound(eyt, sizes, target):
    src = sorted(eyt[1:])
    count = 0
    for v in src:
        if v < target:
            count += 1
        else:
            break
    return count


# ---------------------------------------------------------------------------
# packing
# ---------------------------------------------------------------------------

def _field_deposit(word, value, k, offset):
    k = max(1, min(k, 32))
    offset &= 31
    out = word
    for i in range(k):
        bit = (value >> i) & 1
        out = (out & (M64 ^ (1 << (offset + i)))) | (bit << (offset + i))
    return out


def _field_extract(word, k, offset):
    k = max(1, min(k, 32))
    offset &= 31
    return (word >> offset) & ((1 << k) - 1)


def _elias_fano(value, w):
    w &= 63
    lo = value & ((1 << w) - 1)
    hi = min(value >> w, 63 - w)
    return lo | (1 << (w + hi))


def _delta_enc(value, prev):
    d0 = ((value & M32) - (prev & M32)) & M32
    d1 = ((value >> 32) - (prev >> 32)) & M32
    return d0 | (d1 << 32)


def _delta_dec(delta, prev):
    r0 = ((delta & M32) + (prev & M32)) & M32
    r1 = ((delta >> 32) + (prev >> 32)) & M32
    return r0 | (r1 << 32)


def _transpose8(word):
    res = 0
    for i in range(8):
        for j in range(8):
            if (word >> (i * 8 + j)) & 1:
                res |= 1 << (j * 8 + i)
    return res


def _delta_swap(word, mask, shift):
    shift &= 63
    lo = word & mask
    hi = (word >> shift) & mask
    diff = lo ^ hi
    return (word ^ diff) & M64 ^ ((diff << shift) & M64)


def _bit_swap(word, selector):
    i = selector & 63
    j = (selector >> 6) & 63
    bi = (word >> i) & 1
    bj = (word >> j) & 1
    return (word & (M64 ^ (1 << i)) & (M64 ^ (1 << j))) | (bi << j) | (bj << i)


def _benes(word, selector):
    s = word
    for sh, rot in ((1, 11), (2, 23), (4, 31), (8, 43), (16, 53), (32, 53)):
        m = (selector << rot | selector >> (64 - rot)) & M64 if rot else selector
        t = ((s >> sh) ^ s) & m
        s = (s ^ t ^ (t << sh)) & M64
    return s


def _compress(value, mask):
    res = 0
    r = 0
    for i in range(64):
        if (mask >> i) & 1:
            res |= ((value >> i) & 1) << r
            r += 1
    return res


def _deposit(value, mask):
    res = 0
    v = 0
    for i in range(64):
        if (mask >> i) & 1:
            res |= ((value >> v) & 1) << i
            v += 1
    return res


# ---------------------------------------------------------------------------
# f32 helper
# ---------------------------------------------------------------------------

def f32_bits(x: float) -> str:
    return hex(struct.unpack("<I", struct.pack("<f", x))[0])


# ---------------------------------------------------------------------------
# spec table: fn -> (arg types by name, out type, implementation)
# ---------------------------------------------------------------------------

def _t(name, ty):
    return (name, ty)


SPECS: dict[str, dict[str, tuple]] = {
    "bitset": {
        "set_bit_u64": ([_t("x", "u64"), _t("pos", "us")], "u64", _set_bit),
        "clear_bit_u64": ([_t("x", "u64"), _t("pos", "us")], "u64", _clear_bit),
        "rank_u64": ([_t("x", "u64"), _t("pos", "us")], "us", _rank),
        "rank_exclusive_u64": ([_t("x", "u64"), _t("pos", "us")], "us", _rank_excl),
        "select_bit_u64": ([_t("x", "u64"), _t("n", "us")], "opt_us", _select),
        "xor_u64": ([_t("x", "u64"), _t("y", "u64")], "u64", lambda x, y: x ^ y),
        "not_u64": ([_t("x", "u64")], "u64", lambda x: M64 ^ x),
        "and_not_u64": ([_t("x", "u64"), _t("y", "u64")], "u64", lambda x, y: x & (M64 ^ y)),
        "intersect_u64": ([_t("x", "u64"), _t("y", "u64")], "u64", lambda x, y: x & y),
        "union_u64": ([_t("x", "u64"), _t("y", "u64")], "u64", lambda x, y: x | y),
        "is_subset_u64": ([_t("x", "u64"), _t("y", "u64")], "u64", _is_subset),
        "popcount_u64": ([_t("x", "u64")], "u64", lambda x: bin(x).count("1")),
        "reverse_bits_u64": ([_t("x", "u64")], "u64", lambda x: int(f"{x:064b}"[::-1], 2)),
        "parity_u32": ([_t("x", "u32")], "u32", lambda x: bin(x).count("1") & 1),
        "isolate_lowest_u64": ([_t("x", "u64")], "u64", _isolate_lowest),
        "clear_lowest_u64": ([_t("x", "u64")], "u64", _clear_lowest),
        "mask_to_lowest_u64": ([_t("x", "u64")], "u64", _mask_to_lowest),
        "funnel_shift_right_u64": (
            [_t("hi", "u64"), _t("lo", "u64"), _t("s", "u32")], "u64", _funnel_right),
        "funnel_shift_left_u64": (
            [_t("hi", "u64"), _t("lo", "u64"), _t("s", "u32")], "u64", _funnel_left),
        "parity_u64_slice": ([_t("a", "u64s")], "u64",
                             lambda a: bin(0 if not a else functools.reduce(
                                 lambda p, q: p ^ q, a)).count("1") & 1),
        "hamming_u64_slices": ([_t("a", "u64s"), _t("b", "u64s")], "us",
                               lambda a, b: sum(bin(a[i] ^ b[i]).count("1")
                                                for i in range(min(len(a), len(b))))),
        "any_bit_set_u64_slice": ([_t("a", "u64s")], "bool",
                                  lambda a: any(v != 0 for v in a)),
        "intersect_u64_slices": ([_t("a", "u64s"), _t("b", "u64s")], "u64s", _slices_intersect),
        "union_u64_slices": ([_t("a", "u64s"), _t("b", "u64s")], "u64s", _slices_union),
        "difference_u64_slices": ([_t("a", "u64s"), _t("b", "u64s")], "u64s", _slices_difference),
        "jaccard_u64_slices": ([_t("a", "u64s"), _t("b", "u64s")], "f32b", _jaccard),
    },
    "swar": {
        "byte_lane_in_range": (
            [_t("word", "u64"), _t("lo", "u8"), _t("hi", "u8")], "u64", _lane_in_range),
        "find_byte_in_word": ([_t("word", "u64"), _t("byte", "u8")], "u64", _find_byte_word),
        "count_byte_in_word": ([_t("word", "u64"), _t("byte", "u8")], "u32",
                               lambda w, b: bin(_find_byte_word(w, b)).count("1")),
        "has_byte_in_word": ([_t("word", "u64"), _t("byte", "u8")], "bool",
                             lambda w, b: _find_byte_word(w, b) != 0),
        "first_byte_position": ([_t("word", "u64"), _t("byte", "u8")], "opt_us",
                                lambda w, b: next((i for i, x in enumerate(_lanes(w))
                                                   if x == b), None)),
        "to_lower_ascii_word": ([_t("word", "u64")], "u64",
                                lambda w: w | (_lane_in_range(w, 0x41, 0x5A) >> 2)),
        "to_upper_ascii_word": ([_t("word", "u64")], "u64",
                                lambda w: w & (M64 ^ (_lane_in_range(w, 0x61, 0x7A) >> 2))),
        "classify_bytes": ([_t("word", "u64")], "u64", _classify),
        "parse_8_decimal_digits": ([_t("word", "u64")], "opt_us", _parse8),
        "parse_4_hex_digits": ([_t("word", "u32")], "opt_us", _parse4_hex),
        "find_byte_mask_word": ([_t("word", "u64"), _t("byte", "u8")], "u64", _find_byte_word),
        "find_first_of_word": ([_t("word", "u64"), _t("byte", "u8")], "u64",
                               lambda w, b: next((i for i, x in enumerate(_lanes(w))
                                                  if x == b), 8)),
        "horizontal_sum_u8x8": ([_t("word", "u64")], "u32",
                                lambda w: sum(_lanes(w))),
        "horizontal_max_u8x8": ([_t("word", "u64")], "u8", lambda w: max(_lanes(w))),
        "horizontal_min_u8x8": ([_t("word", "u64")], "u8", lambda w: min(_lanes(w))),
        "horizontal_or_u8x8": ([_t("word", "u64")], "u8",
                               lambda w: functools.reduce(
                                   lambda p, q: p | q, _lanes(w))),
        "horizontal_and_u8x8": ([_t("word", "u64")], "u8",
                                lambda w: functools.reduce(
                                    lambda p, q: p & q, _lanes(w))),
        "horizontal_xor_u8x8": ([_t("word", "u64")], "u8",
                                lambda w: functools.reduce(
                                    lambda p, q: p ^ q, _lanes(w))),
        "horizontal_count_eq_u8": ([_t("word", "u64"), _t("target", "u8")], "u32",
                                   lambda w, t: _lanes(w).count(t)),
        "swar_find_all_positions": ([_t("bytes", "u8s"), _t("target", "u8")], "u64",
                                    _find_all_positions),
        "count_leading_eq_u8": ([_t("bytes", "u8s"), _t("target", "u8")], "us", _count_leading),
        "is_all_ascii": ([_t("bytes", "u8s")], "bool",
                         lambda bs: all(b < 0x80 for b in bs)),
        "skip_spaces": ([_t("bytes", "u8s")], "us",
                        lambda bs: len(bs) - len(list(
                            itertools.dropwhile(lambda b: b == 0x20, bs)))),
        "count_byte_in_slice": ([_t("bytes", "u8s"), _t("target", "u8")], "us",
                                lambda bs, t: bs.count(t)),
        "find_first_byte_in_slice": ([_t("bytes", "u8s"), _t("target", "u8")], "opt_us",
                                     lambda bs, t: next((i for i, b in enumerate(bs)
                                                         if b == t), None)),
    },
    "eytzinger": {
        "build_eytzinger_layout": ([_t("src", "u64s")], "u64s", _layout),
        "build_layout_permutation": ([_t("src", "u64s")], "u64s", _perm),
        "layout_permutation_inverse": ([_t("perm", "u64s")], "u64s", _perm_inverse),
        "build_subtree_sizes": ([_t("len", "us")], "u32s", _subtree_sizes),
        "search_eytzinger_u64": ([_t("eyt", "u64s"), _t("target", "u64")], "opt_us",
                                 _eyt_search),
        "lower_bound_eytzinger": ([_t("eyt", "u64s"), _t("sizes", "u32s"), _t("target", "u64")],
                                  "us", _eyt_lower_bound),
    },
    "sort_small": {
        "sort2": ([_t("a", "u32s")], "u32s", lambda a: sorted(a)),
        "sort4": ([_t("a", "u32s")], "u32s", lambda a: sorted(a)),
        "sort5": ([_t("a", "u32s")], "u32s", lambda a: sorted(a)),
        "sort6": ([_t("a", "u32s")], "u32s", lambda a: sorted(a)),
        "sort7": ([_t("a", "u32s")], "u32s", lambda a: sorted(a)),
        "sort8": ([_t("a", "u32s")], "u32s", lambda a: sorted(a)),
        "sort_bytes_u64": ([_t("val", "u64")], "u64",
                           lambda v: int.from_bytes(bytes(sorted(v.to_bytes(8, "little"))),
                                                    "little")),
        "sort4_lanes_u16": ([_t("v", "u64")], "u64",
                            lambda v: int.from_bytes(
                                b"".join(x.to_bytes(2, "little") for x in sorted(
                                    [int.from_bytes(v.to_bytes(8, "little")[i * 2:i * 2 + 2],
                                                    "little") for i in range(4)])),
                                "little")),
        "sort_small_by_key": ([_t("keys", "u64s")], "u64s",
                              lambda keys: [k for k, _ in sorted(
                                  ((k, i) for i, k in enumerate(keys)),
                                  key=lambda p: (p[0], p[1]))]),
    },
    "packing": {
        "field_deposit": ([_t("word", "u64"), _t("value", "u64"), _t("k", "u32"),
                           _t("offset", "u32")], "u64", _field_deposit),
        "field_extract": ([_t("word", "u64"), _t("k", "u32"), _t("offset", "u32")],
                          "u64", _field_extract),
        "elias_fano_split": ([_t("value", "u64"), _t("w", "u32")], "u64", _elias_fano),
        "delta_encode_u32x2": ([_t("value", "u64"), _t("prev", "u64")], "u64", _delta_enc),
        "delta_decode_u32x2": ([_t("delta", "u64"), _t("prev", "u64")], "u64", _delta_dec),
        "transpose8x8": ([_t("word", "u64")], "u64", _transpose8),
        "transpose64x64_combined": ([_t("a", "u64"), _t("b", "u64")], "u64",
                                    lambda a, b: _transpose8(a ^ b)),
        "delta_swap": ([_t("word", "u64"), _t("mask", "u64"), _t("shift", "u32")],
                       "u64", _delta_swap),
        "bit_swap": ([_t("word", "u64"), _t("selector", "u64")], "u64", _bit_swap),
        "permute_step8": ([_t("word", "u64"), _t("mask", "u64")], "u64",
                          lambda w, m: _delta_swap(w, m, 8)),
        "benes_network": ([_t("word", "u64"), _t("selector", "u64")], "u64", _benes),
        "compress_bits": ([_t("value", "u64"), _t("mask", "u64")], "u64", _compress),
        "deposit_bits": ([_t("value", "u64"), _t("mask", "u64")], "u64", _deposit),
    },
}


def call(module: str, fn: str, args: dict):
    """Runs one oracle case; args are decoded python values, result decoded."""
    types, out_ty, f = SPECS[module][fn]
    vals = [args[name] for name, _ in types]
    return f(*vals)


def arg_types(module: str, fn: str) -> list:
    return SPECS[module][fn][0]


def out_type(module: str, fn: str) -> str:
    return SPECS[module][fn][1]

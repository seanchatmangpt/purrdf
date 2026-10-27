"""Independent scalar/bit-loop reference oracles for src/bitset.rs.

Every body here is a plain Python bit loop, an ``int.bit_count()`` call, or a
``bin()`` string walk -- deliberately a DIFFERENT formulation from the SWAR /
mask-arithmetic bodies in the Rust module, so agreement is evidence. Never
port the Rust SWAR body shape into this file.

All word arguments are Python ints; results are masked to the declared width
so callers may pass unmasked values.
"""

from __future__ import annotations

import struct
from typing import List, Optional, Sequence

MASK64 = 0xFFFFFFFFFFFFFFFF
MASK32 = 0xFFFFFFFF


# ---------------------------------------------------------------------------
# Single-bit set/clear/rank
# ---------------------------------------------------------------------------

def set_bit_u64(x: int, pos: int) -> int:
    """`x` with bit `pos` set. Mirrors bitset.rs::set_bit_u64."""
    return (x | (1 << (pos & 63))) & MASK64


def clear_bit_u64(x: int, pos: int) -> int:
    """`x` with bit `pos` cleared. Mirrors bitset.rs::clear_bit_u64."""
    return (x & ~(1 << (pos & 63))) & MASK64


def rank_u64(x: int, pos: int) -> int:
    """Set bits among positions 0..=pos (inclusive). Mirrors rank_u64."""
    return ((x & ((1 << (pos + 1)) - 1)) & MASK64).bit_count()


def rank_exclusive_u64(x: int, pos: int) -> int:
    """Set bits below position `pos` (exclusive). Mirrors rank_exclusive_u64."""
    return (x & ((1 << (pos & 63)) - 1)).bit_count()


# ---------------------------------------------------------------------------
# Word logical set ops
# ---------------------------------------------------------------------------

def xor_u64(x: int, y: int) -> int:
    """Bitwise XOR. Mirrors xor_u64."""
    return (x ^ y) & MASK64


def not_u64(x: int) -> int:
    """Bitwise NOT within 64 bits. Mirrors not_u64."""
    return ~x & MASK64


def and_not_u64(x: int, y: int) -> int:
    """`x & !y`. Mirrors and_not_u64."""
    return (x & ~y) & MASK64


def intersect_u64(x: int, y: int) -> int:
    """`x & y`. Mirrors intersect_u64."""
    return (x & y) & MASK64


def union_u64(x: int, y: int) -> int:
    """`x | y`. Mirrors union_u64."""
    return (x | y) & MASK64


def is_subset_u64(x: int, y: int) -> int:
    """1 when every set bit of `x` is set in `y`, else 0. Mirrors is_subset_u64."""
    return 1 if (x & ~y) & MASK64 == 0 else 0


def select_bit_u64(x: int, n: int) -> Optional[int]:
    """Position of the (n+1)-th set bit, or None. Mirrors select_bit_u64."""
    seen = -1
    for pos in range(64):
        if (x >> pos) & 1:
            seen += 1
            if seen == n:
                return pos
    return None


# ---------------------------------------------------------------------------
# Counting / bit scans
# ---------------------------------------------------------------------------

def popcount_u64(x: int) -> int:
    """Number of set bits. Mirrors popcount_u64."""
    return (x & MASK64).bit_count()


def leading_zeros_u64(x: int) -> int:
    """Count of zero bits above the highest set bit; 64 when x == 0."""
    x &= MASK64
    if x == 0:
        return 64
    return 64 - x.bit_length()


def trailing_zeros_u64(x: int) -> int:
    """Index of the lowest set bit; 64 when x == 0."""
    x &= MASK64
    if x == 0:
        return 64
    n = 0
    while (x >> n) & 1 == 0:
        n += 1
    return n


def reverse_bits_u64(x: int) -> int:
    """Bit-reversal of a 64-bit word. Mirrors reverse_bits_u64."""
    r = 0
    x &= MASK64
    for i in range(64):
        r |= ((x >> i) & 1) << (63 - i)
    return r


def parity_u32(x: int) -> int:
    """XOR-parity of a 32-bit word (0/1). Mirrors parity_u32."""
    return (x & MASK32).bit_count() & 1


def parity_u64_slice(a: Sequence[int]) -> int:
    """XOR-parity across all words of the slice. Mirrors parity_u64_slice."""
    acc = 0
    for w in a:
        acc ^= w & MASK64
    return acc.bit_count() & 1


def hamming_u64_slices(a: Sequence[int], b: Sequence[int]) -> int:
    """Popcount of pairwise XOR over the min length. Mirrors hamming_u64_slices."""
    n = min(len(a), len(b))
    return sum(((a[i] ^ b[i]) & MASK64).bit_count() for i in range(n))


def jaccard_u64_slices(a: Sequence[int], b: Sequence[int]) -> float:
    """|A&B| / |A|B| over the min length; 0.0 on empty union. Mirrors jaccard_u64_slices.

    The division is rounded through an f32 (struct pack/unpack) to match the
    Rust `f32` return precision.
    """
    n = min(len(a), len(b))
    inter = 0
    union = 0
    for i in range(n):
        inter += ((a[i] & b[i]) & MASK64).bit_count()
        union += ((a[i] | b[i]) & MASK64).bit_count()
    denom = union + (1 if union == 0 else 0)
    raw = inter / denom
    return struct.unpack("<f", struct.pack("<f", raw))[0]


# ---------------------------------------------------------------------------
# Slice set ops (Rust mutates `a` in place; Python returns a fresh list --
# DIVERGENCE from bcinr: immutability is the Python convention)
# ---------------------------------------------------------------------------

def intersect_u64_slices(a: Sequence[int], b: Sequence[int]) -> List[int]:
    """Per-word `a[i] & b[i]` over the min length; a-length result. Mirrors intersect_u64_slices."""
    out = [w & MASK64 for w in a]
    for i in range(min(len(out), len(b))):
        out[i] = (out[i] & b[i]) & MASK64
    return out


def union_u64_slices(a: Sequence[int], b: Sequence[int]) -> List[int]:
    """Per-word `a[i] | b[i]` over the min length; a-length result. Mirrors union_u64_slices."""
    out = [w & MASK64 for w in a]
    for i in range(min(len(out), len(b))):
        out[i] = (out[i] | b[i]) & MASK64
    return out


def difference_u64_slices(a: Sequence[int], b: Sequence[int]) -> List[int]:
    """Per-word `a[i] & !b[i]` over the min length; a-length result. Mirrors difference_u64_slices."""
    out = [w & MASK64 for w in a]
    for i in range(min(len(out), len(b))):
        out[i] = (out[i] & ~b[i]) & MASK64
    return out


def any_bit_set_u64_slice(a: Sequence[int]) -> bool:
    """Whether any word has a set bit. Mirrors any_bit_set_u64_slice."""
    return any((w & MASK64) != 0 for w in a)


# ---------------------------------------------------------------------------
# OPT-54 kernels (loop-form oracles, never the mask-arithmetic bodies)
# ---------------------------------------------------------------------------

def isolate_lowest_u64(x: int) -> int:
    """Only the lowest set bit of `x` (blsi). Loop oracle for isolate_lowest_u64."""
    x &= MASK64
    for i in range(64):
        if (x >> i) & 1:
            return 1 << i
    return 0


def clear_lowest_u64(x: int) -> int:
    """`x` with its lowest set bit cleared (blsr). Loop oracle for clear_lowest_u64."""
    x &= MASK64
    for i in range(64):
        if (x >> i) & 1:
            return clear_bit_u64(x, i)
    return 0


def mask_to_lowest_u64(x: int) -> int:
    """Mask of bits 0..=lowest-set (blsmsk); MASK64 when x == 0 (Rust underflow contract)."""
    x &= MASK64
    if x == 0:
        return MASK64
    low = isolate_lowest_u64(x)
    return (low | (low - 1)) & MASK64


def funnel_shift_right_u64(hi: int, lo: int, s: int) -> int:
    """(hi:lo) >> s truncated to 64 bits, s masked &63; s==0 -> lo.

    Loop oracle via a Python 128-bit intermediate. Mirrors funnel_shift_right_u64.
    """
    s &= 63
    cat = ((hi & MASK64) << 64) | (lo & MASK64)
    return (cat >> s) & MASK64


def funnel_shift_left_u64(hi: int, lo: int, s: int) -> int:
    """(hi:lo) << s truncated to the high 64 bits, s masked &63; s==0 -> hi.

    Loop oracle via a Python 128-bit intermediate. Mirrors funnel_shift_left_u64.
    """
    s &= 63
    cat = ((hi & MASK64) << 64) | (lo & MASK64)
    return (cat >> (64 - s)) & MASK64 if s else (hi & MASK64)

"""Independent per-byte-lane reference oracles for src/swar.rs.

Every body is a plain Python byte loop over the little-endian lanes of the
packed word (``(word >> 8*i) & 0xFF``) or a list fold -- deliberately a
DIFFERENT formulation from the SWAR subtract/Mycroft bodies in the Rust
module. Never port the SWAR body shape into this file.
"""

from __future__ import annotations

from typing import List, Optional, Sequence

MASK64 = 0xFFFFFFFFFFFFFFFF
MASK32 = 0xFFFFFFFF
HIGHS = 0x8080808080808080


def _lanes(word: int, width: int = 8) -> List[int]:
    """Little-endian byte lanes of `word` (lane i = bits 8i..8i+7)."""
    w = word & MASK64
    return [(w >> (8 * i)) & 0xFF for i in range(width)]


def _from_lanes(lanes: Sequence[int]) -> int:
    out = 0
    for i, b in enumerate(lanes):
        out |= (b & 0xFF) << (8 * i)
    return out


# ---------------------------------------------------------------------------
# Byte-lane matching within one 64-bit word
# ---------------------------------------------------------------------------

def find_byte_in_word(word: int, byte: int) -> int:
    """0x80 in each lane whose byte equals `byte`. Mirrors swar.rs::find_byte_in_word."""
    out = 0
    for i, lane in enumerate(_lanes(word)):
        if lane == (byte & 0xFF):
            out |= 0x80 << (8 * i)
    return out


def count_byte_in_word(word: int, byte: int) -> int:
    """Number of lanes equal to `byte` (0-8). Mirrors count_byte_in_word."""
    return _lanes(word).count(byte & 0xFF)


def has_byte_in_word(word: int, byte: int) -> bool:
    """Whether `byte` appears in any lane. Mirrors has_byte_in_word."""
    return count_byte_in_word(word, byte) > 0


def first_byte_position(word: int, byte: int) -> Optional[int]:
    """Index of the first lane equal to `byte`, or None. Mirrors first_byte_position."""
    for i, lane in enumerate(_lanes(word)):
        if lane == (byte & 0xFF):
            return i
    return None


def byte_lane_in_range(word: int, lo: int, hi: int) -> int:
    """0x80 in each lane with lo <= byte <= hi. Mirrors byte_lane_in_range."""
    out = 0
    for i, lane in enumerate(_lanes(word)):
        if (lo & 0xFF) <= lane <= (hi & 0xFF) and lane >> 7 == 0:
            out |= 0x80 << (8 * i)
    return out


def to_lower_ascii_word(word: int) -> int:
    """A-Z -> a-z per lane. Mirrors to_lower_ascii_word."""
    return _from_lanes(
        [b | 0x20 if 0x41 <= b <= 0x5A else b for b in _lanes(word)]
    )


def to_upper_ascii_word(word: int) -> int:
    """a-z -> A-Z per lane. Mirrors to_upper_ascii_word."""
    return _from_lanes(
        [b & 0xDF if 0x61 <= b <= 0x7A else b for b in _lanes(word)]
    )


def classify_bytes(word: int) -> int:
    """Per-lane bitfield: 0x01 digit, 0x02 alpha, 0x04 space, 0x08 upper, 0x10 lower.

    Mirrors swar.rs::classify_bytes.
    """
    out = 0
    for i, b in enumerate(_lanes(word)):
        flags = 0
        if 0x30 <= b <= 0x39:
            flags |= 0x01
        if 0x41 <= b <= 0x5A or 0x61 <= b <= 0x7A:
            flags |= 0x02
        if 0x09 <= b <= 0x0D or b == 0x20:
            flags |= 0x04
        if 0x41 <= b <= 0x5A:
            flags |= 0x08
        if 0x61 <= b <= 0x7A:
            flags |= 0x10
        out |= flags << (8 * i)
    return out


# ---------------------------------------------------------------------------
# Parsers
# ---------------------------------------------------------------------------

def parse_8_decimal_digits(word: int) -> Optional[int]:
    """Value of 8 ASCII decimal digits, lane 0 most significant; None if any non-digit.

    Mirrors swar.rs::parse_8_decimal_digits.
    """
    value = 0
    for lane in _lanes(word):
        if not 0x30 <= lane <= 0x39:
            return None
        value = value * 10 + (lane - 0x30)
    return value & MASK32


def parse_4_hex_digits(word: int) -> Optional[int]:
    """Value of 4 ASCII hex digits (little-endian lanes, lane 0 most significant); None if invalid.

    Mirrors swar.rs::parse_4_hex_digits.
    """
    w = word & MASK32
    value = 0
    for i in range(4):
        lane = (w >> (8 * i)) & 0xFF
        if 0x30 <= lane <= 0x39:
            d = lane - 0x30
        elif 0x41 <= lane <= 0x46:
            d = lane - 0x41 + 10
        elif 0x61 <= lane <= 0x66:
            d = lane - 0x61 + 10
        else:
            return None
        value = value * 16 + d
    return value & MASK32


def find_byte_mask_word(word: int, byte: int) -> int:
    """0x80 in each lane equal to `byte` (cascade-safe). Mirrors find_byte_mask_word."""
    return find_byte_in_word(word, byte)


# ---------------------------------------------------------------------------
# Slice kernels
# ---------------------------------------------------------------------------

def find_byte_mask(bytes_: Sequence[int], target: int) -> int:
    """Bit i set iff bytes[i] == target (first 64 bytes). Mirrors find_byte_mask."""
    mask = 0
    for i, b in enumerate(bytes_[:64]):
        if b == target:
            mask |= 1 << i
    return mask


def is_all_ascii(bytes_: Sequence[int]) -> bool:
    """Whether every byte has bit 7 clear. Mirrors is_all_ascii."""
    return all(b & 0x80 == 0 for b in bytes_)


def skip_spaces(bytes_: Sequence[int]) -> int:
    """Length of the leading run of ASCII spaces. Mirrors skip_spaces."""
    n = 0
    for b in bytes_:
        if b != 0x20:
            break
        n += 1
    return n


def count_byte_in_slice(bytes_: Sequence[int], target: int) -> int:
    """Occurrences of `target` in `bytes`. Mirrors count_byte_in_slice."""
    return sum(1 for b in bytes_ if b == target)


def find_first_byte_in_slice(bytes_: Sequence[int], target: int) -> Optional[int]:
    """First index of `target`, or None. Mirrors find_first_byte_in_slice."""
    for i, b in enumerate(bytes_):
        if b == target:
            return i
    return None


# ---------------------------------------------------------------------------
# Horizontal reductions over 8 byte lanes (plain list folds)
# ---------------------------------------------------------------------------

def horizontal_sum_u8x8(word: int) -> int:
    """Sum of the 8 byte lanes (0..=2040). Mirrors horizontal_sum_u8x8."""
    return sum(_lanes(word)) & MASK32


def horizontal_max_u8x8(word: int) -> int:
    """Maximum byte lane. Mirrors horizontal_max_u8x8."""
    return max(_lanes(word))


def horizontal_min_u8x8(word: int) -> int:
    """Minimum byte lane. Mirrors horizontal_min_u8x8."""
    return min(_lanes(word))


def horizontal_or_u8x8(word: int) -> int:
    """Bitwise OR of all lanes. Mirrors horizontal_or_u8x8."""
    out = 0
    for b in _lanes(word):
        out |= b
    return out


def horizontal_and_u8x8(word: int) -> int:
    """Bitwise AND of all lanes. Mirrors horizontal_and_u8x8."""
    out = 0xFF
    for b in _lanes(word):
        out &= b
    return out


def horizontal_xor_u8x8(word: int) -> int:
    """Bitwise XOR of all lanes. Mirrors horizontal_xor_u8x8."""
    out = 0
    for b in _lanes(word):
        out ^= b
    return out


def horizontal_count_eq_u8(word: int, target: int) -> int:
    """Number of lanes equal to `target`. Mirrors horizontal_count_eq_u8."""
    return _lanes(word).count(target & 0xFF)


# ---------------------------------------------------------------------------
# Fixed-width parallel scans (list folds, masked to the lane width)
# ---------------------------------------------------------------------------

def prefix_sum_u32x16(arr: Sequence[int]) -> List[int]:
    """Inclusive prefix sum (wrapping u32). Mirrors prefix_sum_u32x16."""
    out: List[int] = []
    acc = 0
    for v in arr[:16]:
        acc = (acc + v) & MASK32
        out.append(acc)
    return out


def exclusive_scan_u32x16(arr: Sequence[int]) -> List[int]:
    """Exclusive prefix sum; out[0] == 0. Mirrors exclusive_scan_u32x16."""
    out: List[int] = []
    acc = 0
    for v in arr[:16]:
        out.append(acc)
        acc = (acc + v) & MASK32
    return out


def segmented_prefix_sum_u32x8(values: Sequence[int], flags: Sequence[bool]) -> List[int]:
    """Segmented inclusive prefix sum; a segment restarts at each True flag.

    Mirrors segmented_prefix_sum_u32x8.
    """
    out: List[int] = []
    acc = 0
    for i in range(8):
        if i < len(flags) and flags[i]:
            acc = 0
        acc = (acc + (values[i] if i < len(values) else 0)) & MASK32
        out.append(acc)
    return out


def prefix_max_u32x16(arr: Sequence[int]) -> List[int]:
    """Inclusive prefix maximum. Mirrors prefix_max_u32x16."""
    out: List[int] = []
    run = 0
    for i, v in enumerate(arr[:16]):
        v &= MASK32
        run = max(run, v) if i else v
        out.append(run)
    return out


def prefix_xor_u64x8(arr: Sequence[int]) -> List[int]:
    """Inclusive prefix XOR. Mirrors prefix_xor_u64x8."""
    out: List[int] = []
    acc = 0
    for v in arr[:8]:
        acc ^= v & MASK64
        out.append(acc)
    return out

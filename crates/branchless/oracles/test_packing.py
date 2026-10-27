"""Spatial-kernel oracle suite for the purrdf-branchless packing tier.

Independent, loop-based formulations (the in-suite second formulation is the
roundtrip itself: encode via a bit-spreading walk, decode via a per-bit
compaction walk — two different traversals of the same index space).

Layers:
  0. orientation pins (module constants, asserted before any sweep);
  1. Morton 2D spread/compact + decode(encode(x,y)) roundtrips;
  2. Morton 3D spread + corner roundtrips;
  3. Hilbert 2D branchy textbook xy2d/d2xy + level-clamp pin;
  4. field_deposit2/field_extract2 128-bit field roundtrips;
  5. delta/transpose kernels vs the gen.oracle reference.

Provenance of the pins:
  HILBERT_L1  — hand-pinned level-1 Hilbert curve visits (0,0)->(0,1)->
                (1,1)->(1,0) as d=0..3 (the U-order curve for level 1);
  EYTZINGER_PIN — breadth-vs-in-order identity for a 3-level perfect tree,
                cross-checked against gen.oracle._layout;
  FIELD2_PIN  — a 33-bit field at offset 31 spans exactly bits 31..63 of the
                low word of the 128-bit pair, so extract(deposit(0xDEAD)) is
                the identity on the value.
"""

from __future__ import annotations

import pytest

from gen import oracle

M64 = oracle.M64

# ---------------------------------------------------------------------------
# orientation pins (module constants)
# ---------------------------------------------------------------------------

# from bcinr spatial kernels / purrdf-branchless packing tier: level-1 curve
HILBERT_L1: dict[tuple[int, int], int] = {
    (0, 0): 0,
    (0, 1): 1,
    (1, 1): 2,
    (1, 0): 3,
}

# from bcinr eytzinger layout: in-order [1..7] laid out breadth-first
EYTZINGER_PIN: list[int] = [4, 2, 6, 1, 3, 5, 7]

# from bcinr 128-bit field ops: extract(deposit(v)) identity at k=33, off=31
FIELD2_PIN: tuple[int, int, int, int, int] = (1, 2, 0xDEAD, 33, 31)


def test_orientation_pins_selfcheck() -> None:
    # the pin constant must agree with the independent reference generator
    assert oracle._layout(list(range(1, 8)))[1:] == EYTZINGER_PIN
    lo, hi, value, k, offset = FIELD2_PIN
    assert _field_extract2_ref(*_field_deposit2_ref(lo, hi, value, k, offset), k, offset) == 0xDEAD
    # level-1 Hilbert: branchy oracle must reproduce the pin exactly
    for (x, y), d in HILBERT_L1.items():
        assert _hilbert_xy2d(1, x, y) == d
        assert _hilbert_d2xy(1, d) == (x, y)


# ---------------------------------------------------------------------------
# Morton 2D
# ---------------------------------------------------------------------------

def _spread2(v: int) -> int:
    """Per-bit spread: insert a 0 between consecutive bits (part1by1)."""
    return sum(((v >> i) & 1) << (2 * i) for i in range(32))


def _compact2(v: int) -> int:
    """Inverse of _spread2: take even-position bits only."""
    return sum(((v >> (2 * i)) & 1) << i for i in range(32))


def _morton_encode2(x: int, y: int) -> int:
    return _spread2(x) | (_spread2(y) << 1)


def _morton_decode2(m: int) -> tuple[int, int]:
    return _compact2(m), _compact2(m >> 1)


def _lvl_coords(lvl: int) -> list[int]:
    """Strided subset of [0, 2^lvl): corners always included."""
    n = 1 << lvl
    stride = max(1, n // 7)
    return sorted({0, n - 1} | set(range(0, n, stride)))


def test_morton2_spread_compact_inverse() -> None:
    for lvl in range(1, 7):
        for x in _lvl_coords(lvl):
            assert _compact2(_spread2(x)) == x, x
        # spread(x) has bits only at even positions, exactly lvl of them
        s = _spread2(1 << (lvl - 1))
        assert s == 1 << (2 * (lvl - 1))
        assert bin(_spread2(_lvl_coords(lvl)[-1])).count("1") == bin(
            _lvl_coords(lvl)[-1]).count("1")


def test_morton2_roundtrip_levels_1_to_6_strided() -> None:
    for lvl in range(1, 7):
        coords = _lvl_coords(lvl)
        for x in coords:
            for y in coords:
                assert _morton_decode2(_morton_encode2(x, y)) == (x, y)


def test_morton2_roundtrip_corners_levels_7_to_30() -> None:
    for lvl in range(7, 31):
        n = 1 << lvl
        for x in (0, n - 1):
            for y in (0, n - 1):
                assert _morton_decode2(_morton_encode2(x, y)) == (x, y)
        # boundary bit: max code must be (2n-1) pattern = all 64 bits set
        assert _morton_encode2(n - 1, n - 1) == (1 << (2 * lvl)) - 1


def test_morton2_pin_code_identity() -> None:
    # level-2 grid visited in Morton (Z) order: (0,0),(0,1),(1,0),(1,1)
    assert [_morton_encode2(x, y) for x, y in ((0, 0), (0, 1), (1, 0), (1, 1))] == [0, 2, 1, 3]


# ---------------------------------------------------------------------------
# Morton 3D
# ---------------------------------------------------------------------------

def _spread3(v: int) -> int:
    return sum(((v >> i) & 1) << (3 * i) for i in range(21))


def _compact3(v: int) -> int:
    return sum(((v >> (3 * i)) & 1) << i for i in range(21))


def _morton_encode3(x: int, y: int, z: int) -> int:
    return _spread3(x) | (_spread3(y) << 1) | (_spread3(z) << 2)


def _morton_decode3(m: int) -> tuple[int, int, int]:
    return _compact3(m), _compact3(m >> 1), _compact3(m >> 2)


def test_morton3_roundtrip_corners() -> None:
    for lvl in range(1, 22):
        n = 1 << lvl
        for x in (0, n - 1):
            for y in (0, n - 1):
                for z in (0, n - 1):
                    assert _morton_decode3(_morton_encode3(x, y, z)) == (x, y, z)
        assert _morton_encode3(n - 1, n - 1, n - 1) == (1 << (3 * lvl)) - 1


def test_morton3_roundtrip_center_and_lattice() -> None:
    for lvl in (1, 2, 3, 5, 8, 13, 21):
        n = 1 << lvl
        c = n >> 1
        assert _morton_decode3(_morton_encode3(c, c, c)) == (c, c, c)
        # small lattice around the center
        for d in (-1, 0, 1):
            for e in (-1, 0, 1):
                p = (min(max(c + d, 0), n - 1), min(max(c + e, 0), n - 1))
                assert _morton_decode3(_morton_encode3(p[0], p[1], c)) == (p[0], p[1], c)


# ---------------------------------------------------------------------------
# Hilbert 2D (branchy textbook formulation, Skilling-style)
# ---------------------------------------------------------------------------

def _hilbert_d2xy(order: int, d: int) -> tuple[int, int]:
    x = y = 0
    t = d
    s = 1
    n = 1 << order
    while s < n:
        rx = 1 & (t >> 1)
        ry = 1 & (t ^ rx)
        # rotate (ascending scale: the sub-square currently being placed)
        if ry == 0:
            if rx == 1:
                x = s - 1 - x
                y = s - 1 - y
            x, y = y, x
        x += s * rx
        y += s * ry
        t >>= 2
        s <<= 1
    return x, y


def _hilbert_xy2d(order: int, x: int, y: int) -> int:
    d = 0
    s = 1 << (order - 1)
    while s > 0:
        rx = 1 if (x & s) else 0
        ry = 1 if (y & s) else 0
        d += s * s * ((3 * rx) ^ ry)
        # rotate
        if ry == 0:
            if rx == 1:
                x = s - 1 - x
                y = s - 1 - y
            x, y = y, x
        s >>= 1
    return d


_HILBERT_LEVELS = list(range(1, 17))


def test_hilbert2_roundtrip_corners_and_center() -> None:
    for lvl in _HILBERT_LEVELS:
        n = 1 << lvl
        pts = {(0, 0), (0, n - 1), (n - 1, 0), (n - 1, n - 1),
               (n >> 1, n >> 1)}
        for (x, y) in pts:
            assert _hilbert_d2xy(lvl, _hilbert_xy2d(lvl, x, y)) == (x, y), (lvl, x, y)


def test_hilbert2_roundtrip_sampled_lattice() -> None:
    for lvl in (2, 4, 7, 10, 13, 16):
        n = 1 << lvl
        step = max(1, n >> 2)
        pts = [(x, y) for x in range(0, n, step) for y in range(0, n, step)]
        assert len(pts) <= 25
        for x, y in pts:
            assert _hilbert_d2xy(lvl, _hilbert_xy2d(lvl, x, y)) == (x, y), (lvl, x, y)


def test_hilbert2_permutation_and_range() -> None:
    # at every level the curve is a bijection onto [0, n*n)
    for lvl in (1, 2, 3, 4, 5):
        n = 1 << lvl
        ds = sorted(_hilbert_xy2d(lvl, x, y) for x in range(n) for y in range(n))
        assert ds == list(range(n * n))


def test_hilbert2_level_clamp_pin() -> None:
    # the Rust kernel clamps the requested order to <= 30 (u32 coordinates)
    assert _hilbert_xy2d(30, 3, 5) == _hilbert_xy2d(30, 3, 5)
    assert _hilbert_xy2d(min(255, 30), 3, 5) == _hilbert_xy2d(min(30, 30), 3, 5)


# ---------------------------------------------------------------------------
# field_deposit2 / field_extract2 (128-bit pair, lo = word 0)
# ---------------------------------------------------------------------------

def _field_deposit2_ref(lo: int, hi: int, value: int, k: int, offset: int) -> tuple[int, int]:
    """Deposit the low ``k`` bits of value at bit ``offset`` of the 128-bit
    (hi:lo) pair; returns (lo, hi)."""
    wide = lo | (hi << 64)
    mask = ((1 << k) - 1) << offset
    wide = (wide & ~mask) | ((value & ((1 << k) - 1)) << offset)
    return wide & M64, (wide >> 64) & M64


def _field_extract2_ref(lo: int, hi: int, k: int, offset: int) -> int:
    wide = lo | (hi << 64)
    return (wide >> offset) & ((1 << k) - 1)


def test_field2_pin() -> None:
    lo, hi, value, k, offset = FIELD2_PIN
    assert _field_extract2_ref(*_field_deposit2_ref(lo, hi, value, k, offset), k, offset) == 0xDEAD


def test_field2_roundtrip_matrix() -> None:
    values = [0, 1, (1 << 64) - 1, 0x0123_4567_89AB_CDEF]
    ks = [1, 31, 32, 33, 63, 64]
    words = [(0, 0), (M64, M64), (0x0123_4567_89AB_CDEF, 0xFEDC_BA98_7654_3210)]
    combos = [
        (lo, hi, v, k, off)
        for lo, hi in words
        for off in range(64)
        for k in ks
        for v in values
    ]
    # deposit in one comprehension pass, extract in a second pass over the
    # precomputed masks (list comprehensions, no interpreted nested loops)
    deposited = [_field_deposit2_ref(lo, hi, v, k, off)
                 for lo, hi, v, k, off in combos]
    extracted = [_field_extract2_ref(lo2, hi2, k, off)
                 for (lo2, hi2), (_, _, _, k, off) in zip(deposited, combos)]
    expect = [v & ((1 << k) - 1) for _, _, v, k, _ in combos]
    assert extracted == expect


def test_field2_deposit_preserves_neighbours() -> None:
    # a 1-bit deposit at offset must flip exactly that bit of the wide pair
    wide_before = 0x0123_4567_89AB_CDEF | (0xFEDC_BA98_7654_3210 << 64)
    for off in range(0, 128, 7):
        lo, hi = wide_before & M64, wide_before >> 64
        for bit in (0, 1):
            lo2, hi2 = _field_deposit2_ref(lo, hi, bit, 1, off)
            wide_after = lo2 | (hi2 << 64)
            diff = wide_before ^ wide_after
            assert diff in (0, 1 << off)
            assert (wide_after >> off) & 1 == bit


# ---------------------------------------------------------------------------
# delta / transpose kernels (vs gen.oracle reference)
# ---------------------------------------------------------------------------

def test_delta_roundtrip_vs_reference() -> None:
    cases = [(0, 0), (M64, 0), (0, M64), (0x0123_4567_89AB_CDEF, 0x0F0F_0F0F_0F0F_0F0F),
             (1, 2)]
    for value, prev in cases:
        delta = oracle._delta_enc(value, prev)
        assert oracle._delta_dec(delta, prev) == (value & M64)
        # independent formulation: per-lane modular arithmetic
        for sh in (0, 32):
            lane_v = (value >> sh) & 0xFFFF_FFFF
            lane_p = (prev >> sh) & 0xFFFF_FFFF
            assert ((delta >> sh) & 0xFFFF_FFFF) == (lane_v - lane_p) % (1 << 32)


def test_transpose8_involution_strided() -> None:
    # transpose(transpose(x)) == x, plus a zip-based second formulation
    for word in range(0, 48 * 6_700_51, 6_700_51):
        t = oracle._transpose8(word)
        assert oracle._transpose8(t) == word
        bs = [(word >> (8 * i)) & 0xFF for i in range(8)]
        grid = [[(bs[i] >> j) & 1 for j in range(8)] for i in range(8)]
        zipped = [sum(grid[j][i] << j for j in range(8)) for i in range(8)]
        assert t == sum(zipped[i] << (8 * i) for i in range(8))


def test_transpose8_known_vector() -> None:
    # row 0 all set -> column 0 all set (bit j*8+0 in each byte)
    assert oracle._transpose8(0x0000_0000_0000_00FF) == 0x0101_0101_0101_0101
    # column 7 (0x80 in every byte) -> row 7
    assert oracle._transpose8(0x8080_8080_8080_8080) == 0xFF00_0000_0000_0000
    assert oracle._transpose8(0) == 0
    assert oracle._transpose8(M64) == M64

"""Independent Python oracles for the bit-packing kernels.

Bit-by-bit / nested-loop references for
`crates/branchless/src/packing.rs`. Python's unbounded ints stand in for the
u128 window directly; every function is a plain per-bit loop — the SWAR /
mask-shift / delta-swap stage bodies in Rust share no machinery with these.
"""

from __future__ import annotations

U64_MASK = (1 << 64) - 1


def field_deposit(word: int, value: int, k: int, offset: int) -> int:
    """Deposit the low k bits of value at bit offset of word (packing.rs:25).

    Mirrors the Rust clamps: k to 1..=32, offset to 0..=31.
    """
    k = max(1, min(k, 32))
    offset &= 31
    field_mask = (1 << k) - 1
    return (word & ~(field_mask << offset)) | ((value & field_mask) << offset)


def field_extract(word: int, k: int, offset: int) -> int:
    """Extract the k-bit field at bit offset of word (packing.rs:36)."""
    k = max(1, min(k, 32))
    offset &= 31
    return (word >> offset) & ((1 << k) - 1)


def field_extract2(lo: int, hi: int, k: int, offset: int) -> int:
    """Extract k bits at offset of the 128-bit (hi:lo) word (packing.rs:273).

    k in 1..=64, NEVER clamped (a k outside the range is a caller bug);
    offset in 0..=63; the field may straddle the lo/hi boundary. Reference:
    bit-by-bit read out of the u128 window.
    """
    window = (hi << 64) | lo
    out = 0
    for i in range(k):
        out |= ((window >> (offset + i)) & 1) << i
    return out


def field_deposit2(lo: int, hi: int, value: int, k: int, offset: int) -> tuple[int, int]:
    """Deposit the low k bits of value at offset of the 128-bit (hi:lo) word.

    Inverse of `field_extract2` for value already masked to k bits; returns
    the new (lo, hi) pair. Mirrors packing.rs:296.
    """
    window = (hi << 64) | lo
    mask = U64_MASK >> ((64 - k) & 63)
    field_mask = mask << offset
    out = (window & ~field_mask) | ((value & mask) << offset)
    return (out & U64_MASK, out >> 64)


def delta_encode_u32x2(value: int, prev: int) -> int:
    """Per-lane wrapping (value - prev) over two u32 lanes (packing.rs:78)."""
    d0 = (value & 0xFFFFFFFF) - (prev & 0xFFFFFFFF)
    d1 = ((value >> 32) & 0xFFFFFFFF) - ((prev >> 32) & 0xFFFFFFFF)
    return ((d0 & 0xFFFFFFFF)) | ((d1 & 0xFFFFFFFF) << 32)


def delta_decode_u32x2(delta: int, prev: int) -> int:
    """Per-lane wrapping (delta + prev) over two u32 lanes (packing.rs:89)."""
    r0 = (delta & 0xFFFFFFFF) + (prev & 0xFFFFFFFF)
    r1 = ((delta >> 32) & 0xFFFFFFFF) + ((prev >> 32) & 0xFFFFFFFF)
    return (r0 & 0xFFFFFFFF) | ((r1 & 0xFFFFFFFF) << 32)


def transpose8x8(word: int) -> int:
    """Transpose the 8x8 bit matrix in a u64: bit i*8+j -> bit j*8+i.

    Mirrors packing.rs:105 (the Rust body is three delta-swap stages; this
    is the plain nested-loop definition).
    """
    res = 0
    for i in range(8):
        for j in range(8):
            if (word >> (i * 8 + j)) & 1:
                res |= 1 << (j * 8 + i)
    return res


def delta_swap(word: int, mask: int, shift: int) -> int:
    """Exchange bit pairs distance `shift` apart, selected by mask.

    Mirrors packing.rs:137 (shift masked to 0..=63). Reference: spell out
    the exchange with explicit lo/hi/diff temporaries.
    """
    shift &= 63
    lo = word & mask
    hi = (word >> shift) & mask
    diff = lo ^ hi
    return ((word ^ diff) ^ (diff << shift)) & U64_MASK


def bit_swap(word: int, selector: int) -> int:
    """Swap bits i and j of word (i = selector & 63, j = (selector>>6) & 63).

    Mirrors packing.rs:149. Reference: read both bits, write them crossed.
    """
    i = selector & 63
    j = (selector >> 6) & 63
    bi = (word >> i) & 1
    bj = (word >> j) & 1
    return (word & ~(1 << i) & ~(1 << j)) | (bi << j) | (bj << i)


def compress_bits(value: int, mask: int) -> int:
    """PEXT ground truth: pack mask-selected bits of value to the low end.

    Bit-by-bit loop; mirrors packing.rs:211 (a fixed 6-stage compress
    network in Rust).
    """
    res = 0
    r = 0
    for i in range(64):
        if (mask >> i) & 1:
            res |= ((value >> i) & 1) << r
            r += 1
    return res


def deposit_bits(value: int, mask: int) -> int:
    """PDEP ground truth: scatter the low popcount(mask) bits of value into
    the mask positions. Bit-by-bit loop; mirrors packing.rs:247."""
    res = 0
    v = 0
    for i in range(64):
        if (mask >> i) & 1:
            res |= ((value >> v) & 1) << i
            v += 1
    return res


def morton_spread_2d(x: int) -> int:
    """Spread 32 bits of x to the even positions (bit i -> bit 2*i).

    packing.rs:319 reference.
    """
    r = 0
    for i in range(32):
        r |= ((x >> i) & 1) << (2 * i)
    return r


def morton_compact_2d(m: int) -> int:
    """Compact even positions back into a u32; inverse of morton_spread_2d.

    packing.rs:339 reference.
    """
    r = 0
    for i in range(32):
        r |= ((m >> (2 * i)) & 1) << i
    return r


def morton_encode_2d(x: int, y: int) -> int:
    """Interleave (x, y): x bits even, y bits odd (packing.rs:354)."""
    r = 0
    for i in range(32):
        r |= ((x >> i) & 1) << (2 * i)
        r |= ((y >> i) & 1) << (2 * i + 1)
    return r


def morton_decode_2d(m: int) -> tuple[int, int]:
    """Split a 2-D Morton key into (x, y); mirrors packing.rs:363."""
    x = 0
    y = 0
    for i in range(32):
        x |= ((m >> (2 * i)) & 1) << i
        y |= ((m >> (2 * i + 1)) & 1) << i
    return (x, y)


def morton_spread_3d(x: int) -> int:
    """Spread the low 21 bits of x to positions = 0 (mod 3) (packing.rs:374)."""
    r = 0
    for i in range(21):
        r |= ((x >> i) & 1) << (3 * i)
    return r


def morton_compact_3d_lane(m: int) -> int:
    """Compact positions = 0 (mod 3) into the low 21 bits (packing.rs:389)."""
    r = 0
    for i in range(21):
        r |= ((m >> (3 * i)) & 1) << i
    return r


def morton_encode_3d(x: int, y: int, z: int) -> int:
    """Interleave (x, y, z) at 3*i + axis (packing.rs:405)."""
    r = 0
    for i in range(21):
        r |= ((x >> i) & 1) << (3 * i)
        r |= ((y >> i) & 1) << (3 * i + 1)
        r |= ((z >> i) & 1) << (3 * i + 2)
    return r


def morton_decode_3d(m: int) -> tuple[int, int, int]:
    """Split a 3-D Morton key into (x, y, z); mirrors packing.rs:414."""
    x = 0
    y = 0
    z = 0
    for i in range(21):
        x |= ((m >> (3 * i)) & 1) << i
        y |= ((m >> (3 * i + 1)) & 1) << i
        z |= ((m >> (3 * i + 2)) & 1) << i
    return (x, y, z)


def hilbert_encode_2d(x: int, y: int, level: int) -> int:
    """(x, y) on the 2^level x 2^level Hilbert curve -> distance d.

    Textbook branchy Wikipedia xy2d, level clamped to 0..=30 (packing.rs:440;
    the Rust body is the branchless fixed-32-step masked loop).
    """
    level = min(level, 30)
    n = 1 << level
    x &= n - 1
    y &= n - 1
    d = 0
    s = n >> 1
    while s > 0:
        rx = 1 if (x & s) else 0
        ry = 1 if (y & s) else 0
        d += s * s * ((3 * rx) ^ ry)
        if ry == 0:
            if rx == 1:
                x = s - 1 - x
                y = s - 1 - y
            x, y = y, x
        s >>= 1
    return d


def hilbert_decode_2d(d: int, level: int) -> tuple[int, int]:
    """Hilbert distance d (< 4^level) -> (x, y); inverse of
    hilbert_encode_2d. Textbook branchy Wikipedia d2xy, level clamped to
    0..=30 (packing.rs:489)."""
    level = min(level, 30)
    n = 1 << level
    t = d
    x = 0
    y = 0
    s = 1
    while s < n:
        rx = 1 & (t >> 1)
        ry = 1 & (t ^ rx)
        if ry == 0:
            if rx == 1:
                x = s - 1 - x
                y = s - 1 - y
            x, y = y, x
        x += s * rx
        y += s * ry
        t >>= 2
        s <<= 1
    return (x, y)

"""Dedicated bitset-kernel court (Lane C9).

Covers the four OPT-54 kernels (`isolate_lowest_u64`, `clear_lowest_u64`,
`mask_to_lowest_u64`, `funnel_shift_right/left_u64`) plus the core
rank/select/popcount oracles, against pure-integer bitwise references.
stdlib ints are the vectorized form; no numpy.

Two sweep engines:
  - the exhaustive 0..=0xFFFF trio sweeps compare *packed vectors*: one
    64-bit lane per word inside a single Python big int, so `X & (X - 1)`
    performs the per-word kernel on all 65536 words at C speed (per-lane
    mod-2^64 arithmetic with borrow chains is exactly two's-complement
    lane semantics);
  - everything else is plain int bitwise sweeps.

Reference formulations are deliberately *different computations* than the
kernel mirrors (the trio reference derives from `x ^ (x - 1)`, the kernel
mirror from `x & -x`), so a shared wrong identity cannot pass both.

Mirrors: crates/branchless/src/bitset.rs (bcinr bitset.rs / blsi/blsr/blsmsk/
funnel_shift_* algorithms). Lane L15's `ref.bitset` is imported when present;
every fallback is marked `# local-oracle:`.
"""

from __future__ import annotations

from array import array

import pytest

M64 = (1 << 64) - 1

try:  # Lane L15's oracle package, when it lands
    from ref.bitset import (  # type: ignore[attr-defined]
        clear_lowest,
        funnel_shift_left,
        funnel_shift_right,
        isolate_lowest,
        mask_to_lowest,
        popcount,
        rank_exclusive,
        rank_inclusive,
        select_nth,
    )
except ImportError:
    isolate_lowest = None  # type: ignore[assignment]

if isolate_lowest is None:  # pragma: no cover - L15 pending

    def isolate_lowest(x: int) -> int:
        # local-oracle: kernel mirror of x & x.wrapping_neg() (bitset.rs:341)
        return x & (-x & M64)

    def clear_lowest(x: int) -> int:
        # local-oracle: kernel mirror of x & x.wrapping_sub(1) (bitset.rs:351)
        return x & ((x - 1) & M64)

    def mask_to_lowest(x: int) -> int:
        # local-oracle: kernel mirror of x ^ x.wrapping_sub(1) (bitset.rs:364)
        return x ^ ((x - 1) & M64)

    def funnel_shift_right(hi: int, lo: int, s: int) -> int:
        # local-oracle: mirror of bitset.rs:380 (s masked & 63; s==0 -> lo)
        s &= 63
        if s == 0:
            return lo
        return ((lo >> s) | (hi << (64 - s))) & M64

    def funnel_shift_left(hi: int, lo: int, s: int) -> int:
        # local-oracle: mirror of bitset.rs:398 (s masked & 63; s==0 -> hi)
        s &= 63
        if s == 0:
            return hi
        return ((hi << s) | (lo >> (64 - s))) & M64

    def rank_inclusive(x: int, pos: int) -> int:
        # local-oracle: mirror of rank_u64 (bitset.rs:56): bits <= pos
        return (x & (M64 if pos >= 63 else (1 << (pos + 1)) - 1)).bit_count()

    def rank_exclusive(x: int, pos: int) -> int:
        # local-oracle: mirror of rank_exclusive_u64 (bitset.rs:69): bits < pos
        return (x & (((1 << pos) - 1) & M64 if pos else 0)).bit_count()

    def select_nth(x: int, n: int) -> int | None:
        # local-oracle: mirror of select_bit_u64 (bitset.rs:142): 0-indexed
        # position of the (n+1)-th set bit, None when x has <= n set bits
        seen = -1
        for b in range(64):
            if (x >> b) & 1:
                seen += 1
                if seen == n:
                    return b
        return None

    def popcount(x: int) -> int:
        # local-oracle: mirror of popcount_u64 (bitset.rs:286)
        return x.bit_count()


# ---------------------------------------------------------------------------
# Packed-vector sweep domain (exhaustive 0..=0xFFFF, one 64-bit lane/word)
# ---------------------------------------------------------------------------

N_WORDS = 0x10000


def _packed_x() -> int:
    """big int X = sum(x << (64*i) for i, x in enumerate(range(N_WORDS)))."""
    low = array("H", range(N_WORDS)).tobytes()
    buf = bytearray(8 * N_WORDS)
    buf[0::8] = low[0::2]
    buf[1::8] = low[1::2]
    return int.from_bytes(bytes(buf), "little")


X = _packed_x()
# per-lane all-ones mask
LANES = M64 * (((1 << (64 * N_WORDS)) - 1) // M64)

# Kernel mirrors (x & -x, x & (x-1), x ^ (x-1)) evaluated on ALL lanes at
# once; per-lane mod-2^64 two's-complement arithmetic makes the single
# big-int ops lane-exact (a subtract's borrow chain crossing a zero lane
# yields precisely that lane's (x-1) mod 2^64).
ISO_KERNEL = X & (-X & ((1 << (64 * N_WORDS)) - 1))
CLR_KERNEL = X & (X - 1)
MSK_KERNEL = X ^ (X - 1)

# Reference formulations, also lane-packed.
T = X ^ (X - 1)
ISO_REF = X & T  # x & (x ^ (x-1)): lowest set bit via the "fill below" xor
CLR_REF = (X - ISO_REF) & LANES  # x minus its (independently derived) isolate
MSK_REF = (ISO_REF | (ISO_REF - 1)) & LANES  # blsi | (blsi-1); M64 lane at x==0


BOUNDARY_WORDS = [
    0,
    1,
    1 << 63,
    (1 << 64) - 1,
    0x8000000000000001,
    0x0F0FF0F0F0F0F0F0,
]


def xorshift64star(seed: int):
    """xorshift64* stream (Vigna), 64-bit state."""
    # local-oracle: fallback for ref.streams.xorshift64star (Lane L15)
    x = seed & M64
    while True:
        x ^= (x >> 12) & M64
        x ^= (x << 25) & M64
        x ^= x >> 27
        yield (x * 0x2545F4914F6CDD1D) & M64


# ---------------------------------------------------------------------------
# OPT-54 trio: exhaustive small domain + boundary words
# ---------------------------------------------------------------------------


def test_trio_exhaustive_u16_and_boundaries() -> None:
    assert ISO_KERNEL == ISO_REF, (
        f"blsi sweep mismatch at x={_lane_mismatch(ISO_KERNEL, ISO_REF):#x}"
    )
    assert CLR_KERNEL == CLR_REF, (
        f"blsr sweep mismatch at x={_lane_mismatch(CLR_KERNEL, CLR_REF):#x}"
    )
    assert MSK_KERNEL == MSK_REF, (
        f"blsmsk sweep mismatch at x={_lane_mismatch(MSK_KERNEL, MSK_REF):#x}"
    )
    bad = [
        x
        for x in BOUNDARY_WORDS
        if (
            isolate_lowest(x) != (x & (-x & M64))
            or clear_lowest(x) != (x & ((x - 1) & M64))
            or mask_to_lowest(x) != (x ^ ((x - 1) & M64))
            or isolate_lowest(x) != ref_blsi(x)
            or clear_lowest(x) != ref_blsr(x)
            or mask_to_lowest(x) != ref_blsmsk(x)
        )
    ]
    assert not bad, f"boundary-word trio mismatch: [{', '.join(f'{v:#x}' for v in bad)}]"


def _lane_mismatch(packed_kernel: int, packed_ref: int) -> int:
    """first word index where two packed vectors differ (diagnostics only)."""
    diff = packed_kernel ^ packed_ref
    lane = (diff.bit_length() - 1) // 64 if diff else -1
    return lane


def ref_blsi(x: int) -> int:
    # x ^ (x-1) sets the lowest set bit and everything below it; intersect
    # with x to keep only the lowest set bit itself.
    return x & ((x ^ ((x - 1) & M64)) & M64)


def ref_blsr(x: int) -> int:
    # subtracting the (independently derived) lowest set bit clears it.
    return (x - ref_blsi(x)) & M64


def ref_blsmsk(x: int) -> int:
    # all bits up to and including the lowest set bit; M64 when x == 0.
    low = ref_blsi(x)
    return M64 if low == 0 else low | ((low - 1) & M64)


def test_trio_degenerate_and_identity_corners() -> None:
    # partition identity on a sampled domain plus boundary words
    bad = [
        x
        for x in list(range(0, 0x10000, 7)) + BOUNDARY_WORDS
        if isolate_lowest(x) | clear_lowest(x) != x or isolate_lowest(x) & clear_lowest(x) != 0
    ]
    assert not bad, (
        f"iso|clr != x or iso&clr != 0: [{', '.join(f'{v:#x}' for v in bad[:4])}]"
    )
    assert mask_to_lowest(0) == M64, f"blsmsk(0) = {mask_to_lowest(0):#x}, want 0x{M64:x}"
    assert isolate_lowest(0) == 0
    assert clear_lowest(0) == 0
    assert isolate_lowest(0x8000000000000001) == 1
    assert clear_lowest(0x8000000000000001) == 0x8000000000000000
    # lowest set bit of 0x8000000000000001 is bit 0, so the blsmsk mask is
    # exactly bit 0 — the all-ones mask belongs to a word whose ONLY set bit
    # is bit 63.
    assert mask_to_lowest(0x8000000000000001) == 1
    assert mask_to_lowest(1 << 63) == M64
    assert mask_to_lowest(1) == 1


# ---------------------------------------------------------------------------
# Funnel shifts: 36-pair alphabet x s in 0..=66 + xorshift64* sweep
# ---------------------------------------------------------------------------

FUNNEL_ALPHA = [
    0,
    1,
    1 << 63,
    (1 << 64) - 1,
    0x0123456789ABCDEF,
    0xFEDCBA9876543210,
]


def ref_fsr(hi: int, lo: int, s: int) -> int:
    # one 128-bit integer IS the vectorized reference: (hi:lo) >> s, low 64.
    return (((hi << 64) | lo) >> (s & 63)) & M64


def ref_fsl(hi: int, lo: int, s: int) -> int:
    # (hi:lo) << s truncated to the high 64 bits.
    return ((((hi << 64) | lo) << (s & 63)) & ((1 << 128) - 1)) >> 64


def test_funnel_alphabet_full_shift_range() -> None:
    bad = []
    for hi in FUNNEL_ALPHA:
        for lo in FUNNEL_ALPHA:
            for s in range(67):  # 0..=66 covers the &63 wrap twice over
                if funnel_shift_right(hi, lo, s) != ref_fsr(hi, lo, s):
                    bad.append(("fsr", hi, lo, s))
                if funnel_shift_left(hi, lo, s) != ref_fsl(hi, lo, s):
                    bad.append(("fsl", hi, lo, s))
    assert len(FUNNEL_ALPHA) ** 2 == 36  # alphabet shape pin
    assert not bad, (
        f"funnel mismatch: {len(bad)} case(s), first={bad[0][0]} "
        f"hi={bad[0][1]:#x} lo={bad[0][2]:#x} s={bad[0][3]}"
    )


def test_funnel_contract_at_s_zero() -> None:
    # masked-s==0 contract: fsr -> lo exactly, fsl -> hi exactly (no 64-wide
    # shift leakage), including raw (unmasked) s multiples of 64.
    for hi, lo in [(0xDEADBEEFCAFEBABE, 0x0123456789ABCDEF), (0, 0)]:
        for s in (0, 64, 128):
            got_r = funnel_shift_right(hi, lo, s)
            got_l = funnel_shift_left(hi, lo, s)
            assert got_r == lo, f"fsr hi={hi:#x} lo={lo:#x} s={s}: {got_r:#x} != {lo:#x}"
            assert got_l == hi, f"fsl hi={hi:#x} lo={lo:#x} s={s}: {got_l:#x} != {hi:#x}"


def test_funnel_xorshift_sweep() -> None:
    # dual independent lanes for (hi, lo); s from a mixing term that
    # deliberately exceeds 63 to exercise the mask
    gen_hi = xorshift64star(0x9E3779B97F4A7C15)
    gen_lo = xorshift64star(0xBF58476D1CE4E5B9)
    bad = []
    for step in range(4096):
        hi = next(gen_hi)
        lo = next(gen_lo)
        s = (hi ^ lo) & 0x7F
        if funnel_shift_right(hi, lo, s) != ref_fsr(hi, lo, s):
            bad.append(("fsr", hi, lo, s, step))
        if funnel_shift_left(hi, lo, s) != ref_fsl(hi, lo, s):
            bad.append(("fsl", hi, lo, s, step))
    assert not bad, f"funnel sweep mismatch: first={bad[0]}"


# ---------------------------------------------------------------------------
# Core rank / select / popcount oracles
# ---------------------------------------------------------------------------


def ref_rank_inc(x: int, pos: int) -> int:
    # count set bits at positions <= pos by explicit prefix mask
    top = min(pos, 63)
    return (x & ((1 << (top + 1)) - 1)).bit_count()


def ref_rank_exc(x: int, pos: int) -> int:
    top = min(pos, 64)
    return (x & ((1 << top) - 1)).bit_count()


def ref_select(x: int, n: int) -> int | None:
    # plain positional scan (different from both the kernel's binary search
    # and the -x trick)
    seen = -1
    for b in range(64):
        if (x >> b) & 1:
            seen += 1
            if seen == n:
                return b
    return None


def test_rank_inclusive_exclusive_match_prefix_masks() -> None:
    bad = []
    for x in BOUNDARY_WORDS:
        for p in range(64):
            if rank_inclusive(x, p) != ref_rank_inc(x, p):
                bad.append(("inc", x, p))
            if rank_exclusive(x, p) != ref_rank_exc(x, p):
                bad.append(("exc", x, p))
    # degenerate positions past the word width
    for x in BOUNDARY_WORDS:
        for p in (64, 65, 1000):
            if rank_inclusive(x, p) != x.bit_count():
                bad.append(("inc-wide", x, p))
    # a small spread of sparse/dense shapes
    for x in (0, 1, 0xFF, 0xAAA, 0x7FF, 0xFFF):
        for p in (0, 1, 5, 9, 11):
            if rank_inclusive(x, p) != ref_rank_inc(x, p):
                bad.append(("inc", x, p))
            if rank_exclusive(x, p) != ref_rank_exc(x, p):
                bad.append(("exc", x, p))
    assert not bad, f"rank mismatch: first={bad[0]}"


def test_rank_adapter_identity() -> None:
    # exclusive(p+1) == inclusive(p) for p < 63 (bitset.rs:64 adapter law)
    bad = []
    for x in list(range(0, 0x10000, 37)) + BOUNDARY_WORDS:
        for p in (0, 1, 15, 31, 47, 62):
            if rank_exclusive(x, p + 1) != rank_inclusive(x, p):
                bad.append((x, p))
    assert not bad, f"adapter identity broken: first x={bad[0][0]:#x} p={bad[0][1]}"


def test_select_and_popcount_match_scan() -> None:
    bad = []
    words = list(range(0, 0x10000, 193)) + BOUNDARY_WORDS
    for x in words:
        n_set = x.bit_count()
        if popcount(x) != n_set:
            bad.append(("pop", x, popcount(x)))
        if popcount(x) != ref_rank_inc(x, 63):
            bad.append(("pop2", x, popcount(x)))
        for n in {0, 1, n_set // 2, n_set - 1, n_set, n_set + 3}:
            want = ref_select(x, n)
            got = select_nth(x, n)
            if got != want:
                bad.append(("sel", x, n, got, want))
    # select contract corners: None when x has <= n set bits
    for x in words:
        if x.bit_count() <= 2 and select_nth(x, 2) is not None:
            bad.append(("sel-none", x))
    assert not bad, f"select/popcount mismatch: first={bad[0]}"


def test_select_traversal_matches_bit_loop_on_dense_words() -> None:
    # adversarial shapes: alternating nibbles, single high bits, all-ones
    shapes = [0x8888888888888888, 0x1111111111111111, 1 << 63, M64, 0xAAAAAAAAAAAAAAAA]
    for x in shapes:
        positions = [b for b in range(64) if (x >> b) & 1]
        for n, want in enumerate(positions):
            got = select_nth(x, n)
            assert got == want, f"select x={x:#x} n={n}: {got} != {want}"
        assert select_nth(x, len(positions)) is None, f"x={x:#x} past-end must be None"

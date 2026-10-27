"""Test-stream primitives: xorshift64* and boundary-corner corpora.

Pure Python mirrors of the sweep drivers used by the Rust tests in
crates/branchless/src/bitset.rs and src/swar.rs. All arithmetic is masked to
the lane width so Python's big ints never leak past 64 bits.
"""

from __future__ import annotations

from typing import Iterator, Sequence

U64 = 0xFFFFFFFFFFFFFFFF
U32 = 0xFFFFFFFF

GOLDEN = 0x9E3779B97F4A7C15
MULT: int = 0x2545F4914F6CDD1D


def xorshift64star(seed: int = GOLDEN) -> Iterator[int]:
    """Infinite xorshift64* stream; x ^= x<<13; x ^= x>>7; x ^= x<<17; x *= M.

    Mirrors the `xorshift64star` helper in the branchless Rust test modules.
    """
    x = seed & U64
    while True:
        x ^= (x << 13) & U64
        x ^= x >> 7
        x ^= (x << 17) & U64
        x = (x * MULT) & U64
        yield x


def boundary_u64() -> list[int]:
    """Fixed 64-bit corner words: 0, 1, sign boundaries, MAX, and walk masks."""
    return [
        0x0,
        0x1,
        0x7FFFFFFFFFFFFFFF,
        0x8000000000000000,
        0xFFFFFFFFFFFFFFFF,
        1 << 63,
        0x8000000000000001,
        0x0F0FF0F0F0F0F0F0,
    ]


def boundary_u32() -> list[int]:
    """Fixed 32-bit corner words mirroring boundary_u64 at half width."""
    return [
        0x0,
        0x1,
        0x7FFFFFFF,
        0x80000000,
        0xFFFFFFFF,
        1 << 31,
        0x80000001,
        0x0F0FF0F0,
    ]


def fuzz(
    corners: Sequence[int],
    stream: Iterator[int],
    n: int,
) -> list[int]:
    """Interleave the fixed `corners` with `n` draws from `stream`.

    Corner-first ordering keeps deterministic boundary coverage at the head of
    the corpus; the stream draws provide the randomized sweep behind it.
    """
    out: list[int] = list(corners)
    for _ in range(n):
        out.append(next(stream) & U64)
    return out

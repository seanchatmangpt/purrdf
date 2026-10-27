"""Independent Python oracles for the small-N sorting kernels.

Plain `sorted()` / branchy compare-swap references for
`crates/branchless/src/sort_small.rs` — deliberately NOT the XOR-mask
comparator networks; the oracle's value is that it shares no machinery
with the kernel bodies.
"""

from __future__ import annotations

from collections.abc import MutableSequence


def sort2(a: list[int]) -> list[int]:
    """Sorted form of a 2-element slice (mirrors `sort2`, sort_small.rs:37)."""
    return sorted(a)


def sort4(a: list[int]) -> list[int]:
    """Sorted form of a 4-element slice (mirrors `sort4`, sort_small.rs:45)."""
    return sorted(a)


def sort5(a: list[int]) -> list[int]:
    """Sorted form of a 5-element slice (mirrors `sort5`, sort_small.rs:56)."""
    return sorted(a)


def sort6(a: list[int]) -> list[int]:
    """Sorted form of a 6-element slice (mirrors `sort6`, sort_small.rs:71)."""
    return sorted(a)


def sort7(a: list[int]) -> list[int]:
    """Sorted form of a 7-element slice (mirrors `sort7`, sort_small.rs:89)."""
    return sorted(a)


def sort8(a: list[int]) -> list[int]:
    """Sorted form of an 8-element slice (mirrors `sort8`, sort_small.rs:111)."""
    return sorted(a)


def cswap(lanes: MutableSequence[int], i: int, j: int) -> None:
    """Branchy in-place compare-exchange: orders lanes[i] <= lanes[j].

    Mirrors `cswap` (sort_small.rs:28) with a Python `if` + temporary —
    the oracle IS the branchy form the Rust kernel's XOR-mask predication
    must agree with. Mutates `lanes` in place like the Rust `&mut [u32]`.
    """
    if lanes[i] > lanes[j]:
        tmp = lanes[i]
        lanes[i] = lanes[j]
        lanes[j] = tmp


def cswap_u64(lanes: MutableSequence[int], i: int, j: int) -> None:
    """64-bit-lane variant of `cswap` (mirrors `cswap_u64`, sort_small.rs:152).

    In Python the integer width is unbounded, so this is the same branchy
    exchange; it exists so differential tests can name the u64 kernel.
    """
    cswap(lanes, i, j)


def sort_bytes_u64(val: int) -> int:
    """Sorts the 8 bytes of a u64 ascending, LSB first (sort_small.rs:244).

    Reference: split to bytes, `sorted()`, repack — the Rust kernel computes
    stable ranks by fully unrolled comparison-as-arithmetic.
    """
    assert 0 <= val < (1 << 64), "val must fit a u64"
    byte_list = [(val >> (8 * i)) & 0xFF for i in range(8)]
    out = 0
    for i, b in enumerate(sorted(byte_list)):
        out |= b << (8 * i)
    return out

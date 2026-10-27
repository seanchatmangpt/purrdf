"""Independent Python oracles for the Eytzinger-layout kernels.

Every function here is a plain-loop / linear-scan / sorted()-based reference
formulation, deliberately different from the branchless Rust bodies in
`crates/branchless/src/eytzinger.rs` (which walk the implicit BFS tree with
predicated compares). These compute the same answers structurally: linear
scans and index arithmetic, no tree descent.

Mirrors the public API of `crates/branchless/src/eytzinger.rs`.
"""

from __future__ import annotations

from collections.abc import Callable


def _inorder_slots(n: int) -> list[int]:
    """In-order visit order of the implicit-tree slots 1..=n (iterative).

    The Eytzinger placement IS the in-order sequence of the BFS-numbered
    tree: the p-th visited slot receives the p-th smallest key.
    """
    slots: list[int] = []
    stack: list[int] = []
    k = 1
    while stack or k <= n:
        while k <= n:
            stack.append(k)
            k = 2 * k
        k = stack.pop()
        slots.append(k)
        k = 2 * k + 1
    return slots


def build_eytzinger_layout(src: list[int]) -> list[int]:
    """Sorted keys -> 1-based Eytzinger array with a 0 sentinel at index 0.

    Contract of `build_eytzinger_layout` (eytzinger.rs:24): result length is
    `len(src) + 1`, `result[0] == 0`, and the classic BFS-index placement
    holds — the p-th smallest key lands at the p-th in-order slot of the
    implicit heap-numbered tree. Implemented here with an explicit stack
    walk (index math only), not the Rust recursion.
    """
    n = len(src)
    out = [0] * (n + 1)
    for p, slot in enumerate(_inorder_slots(n)):
        out[slot] = src[p]
    return out


def build_layout_permutation(src: list[int]) -> list[int]:
    """Slot -> sorted-index permutation (perm[0] is the unused sentinel 0).

    Contract of `build_layout_permutation` (eytzinger.rs:43): `perm[k]` is
    the sorted index of the key at layout slot `k`. The Eytzinger placement
    IS the in-order sequence of the implicit tree, so the permutation is
    positional: the p-th visited slot receives sorted index p -- the same
    assignment `build_eytzinger_layout` uses, so the two cannot drift and
    duplicate keys stay positional (`src.index(key)` would collapse every
    duplicate to its first occurrence).
    """
    perm = [0] * (len(src) + 1)
    for p, slot in enumerate(_inorder_slots(len(src))):
        perm[slot] = p
    return perm


def layout_permutation_inverse(perm: list[int]) -> list[int]:
    """Inverts a slot->sorted permutation into sorted->slot (`out[0]` is 0).

    Contract of `layout_permutation_inverse` (eytzinger.rs:62):
    `out[perm[k]] == k` for every usable slot k; index 0 stays 0.
    """
    out = [0] * len(perm)
    for k in range(1, len(perm)):
        out[perm[k]] = k
    return out


def build_subtree_sizes(length: int) -> list[int]:
    """Subtree-size table for a layout of `length` slots (len == n + 1).

    Contract of `build_subtree_sizes` (eytzinger.rs:97): `length` is the
    LAYOUT length (usable slots + 1), sizes[1] == n, and for every k in
    1..=n the recurrence `sizes[k] = 1 + left + right` holds (children past
    the array count 0). Plain top-down memo-free loop over all k, bottom-up.

    The oracle is TOTAL: length 0 falls out of the natural body as []
    (``range(-1, 0, -1)`` is empty) -- masked index arithmetic, return 0,
    no raise. The Rust kernel asserts len >= 1; that divergence is
    documented in test_eytzinger.py's module docstring.
    """
    sizes = [0] * length
    n = length - 1
    for k in range(n, 0, -1):
        left = sizes[2 * k] if 2 * k < length else 0
        right = sizes[2 * k + 1] if 2 * k + 1 < length else 0
        sizes[k] = 1 + left + right
    return sizes


def search_eytzinger_u64(eytzinger: list[int], target: int) -> int | None:
    """Exact search over a 1-based layout: the slot or None.

    Contract of `search_eytzinger_u64` (eytzinger.rs:123): returns the
    1-based layout slot holding `target`, NOT the sorted index; None on
    miss. Reference formulation: a plain linear scan — structurally
    independent of the tree-descent loop in the Rust kernel.
    """
    for k in range(1, len(eytzinger)):
        if eytzinger[k] == target:
            return k
    return None


def search_by(eytzinger: list, target: int, key: Callable) -> int | None:
    """Exact search with a key accessor: the 1-based slot or None.

    Contract of `search_by` (eytzinger.rs:147): same semantics as
    `search_eytzinger_u64` over `key(eytzinger[k])`.
    """
    for k in range(1, len(eytzinger)):
        if key(eytzinger[k]) == target:
            return k
    return None


def lower_bound_eytzinger(eytzinger: list[int], sizes: list[int], target: int) -> int:
    """Lower bound as the count of elements < target (bisect_left equivalent).

    Contract of `lower_bound_eytzinger` (eytzinger.rs:179): returns the
    sorted index of the first element >= target, i.e. exactly the number of
    usable slots whose key is < target. Reference formulation: that count,
    computed by a plain linear scan — the `sizes` table is accepted for API
    parity but never consulted (the Rust kernel needs it; the oracle does
    not, which is precisely the independence we are buying).
    """
    return sum(1 for k in range(1, len(eytzinger)) if eytzinger[k] < target)

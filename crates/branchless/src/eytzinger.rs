// SPDX-FileCopyrightText: 2026 Blackcat Informatics(R) Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Branchless 1-based Eytzinger layout, exact search, and lower bound.
//!
//! Ported from `purrdf-core` (`crates/rdf-core/src/eytzinger.rs`, now a
//! re-export of this module — the switchover has landed). This module is the
//! single home for the Eytzinger kernels.
//!
//! Layout semantics: `dest.len() == src.len() + 1`, `dest[0]` is a `0`
//! sentinel. Search returns the 1-based layout slot, NOT the sorted index;
//! use the permutation helpers to map between them.

// The search kernels are single-call hot-path leaves; inlining at every call
// site is the point (module precedent: cost_math.rs).
#![allow(clippy::inline_always)]

/// Interned 64-bit term identifier.
pub type TermId = u64;

/// Constructs a 1-based Eytzinger array from an existing sorted slice.
///
/// `dest` length must be `src.len() + 1`; `dest[0]` remains `0` as sentinel.
pub fn build_eytzinger_layout(src: &[TermId], dest: &mut [TermId]) {
    assert_eq!(dest.len(), src.len() + 1);
    let mut cursor = 0usize;
    layout_recursive(src, dest, 1, &mut cursor);
}

fn layout_recursive(src: &[TermId], dest: &mut [TermId], k: usize, cursor: &mut usize) {
    if k < dest.len() {
        layout_recursive(src, dest, 2 * k, cursor);
        dest[k] = src[*cursor];
        *cursor += 1;
        layout_recursive(src, dest, 2 * k + 1, cursor);
    }
}

/// Builds the layout-slot to sorted-index permutation for an Eytzinger layout.
///
/// `perm[k]` is the sorted index of the key at layout slot `k` (`perm[0]`
/// unused). `perm.len()` must equal `src.len() + 1`.
pub fn build_layout_permutation(src: &[TermId], perm: &mut [usize]) {
    assert_eq!(perm.len(), src.len() + 1);
    let mut cursor = 0usize;
    perm_recursive(perm, 1, &mut cursor);
}

fn perm_recursive(perm: &mut [usize], k: usize, cursor: &mut usize) {
    if k < perm.len() {
        perm_recursive(perm, 2 * k, cursor);
        perm[k] = *cursor;
        *cursor += 1;
        perm_recursive(perm, 2 * k + 1, cursor);
    }
}

/// Inverts a layout-to-sorted permutation (`out[sorted_index] = slot`).
///
/// Index 0 is a sentinel in `perm` and left untouched in `out`; both slices
/// must have equal length and the caller initializes `out[0]`.
pub fn layout_permutation_inverse(perm: &[usize], out: &mut [usize]) {
    assert_eq!(out.len(), perm.len());
    for (slot, &sorted) in perm[1..].iter().enumerate() {
        out[sorted] = slot + 1;
    }
}

/// Builds the parallel subtree-size array for an Eytzinger layout.
///
/// `sizes[k]` is the number of layout slots in the subtree rooted at slot
/// `k` (slots past the array count 0). Bottom-up, O(n).
///
/// `len` is the **layout length** (usable slots + 1, i.e. the same length as
/// the Eytzinger array itself), NOT the slot count; every caller passes
/// `n + 1`. `sizes` may be a padded buffer (`sizes.len() >= len`; padding is
/// left zeroed).
///
/// Invariant: `sizes[1] == n` (the root covers all usable slots), and for
/// every `k` in `1..=n` the exact recurrence
///
/// ```text
/// sizes[k] = 1 + (sizes[2k] if 2k <= n else 0)
///              + (sizes[2k+1] if 2k+1 <= n else 0)
/// ```
///
/// with leaf slots (`2k > n`) equal to 1.
///
/// Masking note: [`lower_bound_eytzinger`] consumes `sizes[j]` only at even
/// `j` (the left child of whichever node the search path visits). Passing the
/// slot count `m = n` instead of `n + 1` drops slot `n` to 0, and that deficit
/// propagates up the ancestor chain. The lower-bound symptom manifests IFF
/// `n` is NOT of the form `2^d - 1`: for a perfect tree the even indices the
/// search reads all remain correct, so the wrong table is entirely masked
/// (see the test `subtree_sizes_perfect_tree_masking` — this is why green
/// suites at n ∈ {1, 7} hid the convention conflict).
pub fn build_subtree_sizes(len: usize, sizes: &mut [u32]) {
    assert!(
        len >= 1,
        "layout length must be at least 1 (n+1 >= 1; got len=0 — pass the layout length n+1, not the slot count n)"
    );
    assert!(sizes.len() >= len);
    sizes[..].fill(0);
    let n = len - 1; // usable slots 1..=n
    for k in (1..=n).rev() {
        let l = if 2 * k < sizes.len() {
            sizes[2 * k] as usize
        } else {
            0
        };
        let r = if 2 * k + 1 < sizes.len() {
            sizes[2 * k + 1] as usize
        } else {
            0
        };
        sizes[k] = (1 + l + r) as u32;
    }
}

/// Branchless Eytzinger exact search over a 1-based contiguous array.
///
/// Returns `Some(layout_slot)` on exact match: the 1-based position inside
/// `eytzinger`, NOT the sorted index. `None` on miss. One predicated compare
/// per level; the trip count is the tree depth.
#[inline(always)]
pub fn search_eytzinger_u64(eytzinger: &[TermId], target: TermId) -> Option<usize> {
    let n = eytzinger.len();
    if n <= 1 {
        return None;
    }
    let mut k: usize = 1;
    while k < n {
        let cmp = usize::from(eytzinger[k] < target);
        k = 2 * k + cmp;
    }
    // The candidate is the last left-turn node: strip trailing 1s from k.
    let shift = (!k).trailing_zeros() + 1;
    let idx = k >> shift;
    if idx > 0 && idx < n && eytzinger[idx] == target {
        Some(idx)
    } else {
        None
    }
}

/// Branchless exact search with a generic key accessor.
///
/// Same semantics as [`search_eytzinger_u64`] over `key(&eytzinger[k])`.
#[inline(always)]
pub fn search_by<T>(eytzinger: &[T], target: TermId, key: impl Fn(&T) -> TermId) -> Option<usize> {
    let n = eytzinger.len();
    if n <= 1 {
        return None;
    }
    let mut k: usize = 1;
    while k < n {
        let cmp = usize::from(key(&eytzinger[k]) < target);
        k = 2 * k + cmp;
    }
    let shift = (!k).trailing_zeros() + 1;
    let idx = k >> shift;
    if idx > 0 && idx < n && key(&eytzinger[idx]) == target {
        Some(idx)
    } else {
        None
    }
}

/// Branchless Eytzinger lower bound: the sorted index of the first element
/// `>= target` (the count of elements `< target`).
///
/// Needs the parallel subtree-size array from [`build_subtree_sizes`]. On each
/// right turn (`a[k] < target`) the whole left subtree of `k` plus `k` itself
/// are `< target`, contributing `S[2k] + 1`; O(log n), one cmov per level.
///
/// Length contract: `eyt[s] == 0` for the sentinel slot 0, i.e. `eyt.len() ==
/// slot_count + 1`, and `sizes` must be the table [`build_subtree_sizes`]
/// produced with that same **layout length** (`sizes[..=slot_count]` must
/// satisfy `sizes[1] == slot_count`). Only even indices `2k` of `sizes` are
/// read.
#[inline(always)]
pub fn lower_bound_eytzinger(eytzinger: &[TermId], sizes: &[u32], target: TermId) -> usize {
    let n = eytzinger.len() - 1; // usable slots 1..=n
    if n == 0 {
        return 0;
    }
    let mut k: usize = 1;
    let mut res: usize = 0;
    while k <= n {
        let cmp = usize::from(eytzinger[k] < target);
        let left = if 2 * k < sizes.len() {
            sizes[2 * k] as usize
        } else {
            0
        };
        res += cmp * (left + 1);
        k = 2 * k + cmp;
    }
    res
}

/// Advisory cache prefetch: SAFE no-op stub.
///
/// `core::arch` prefetch intrinsics need `unsafe`, forbidden here; call sites
/// keep this marker so a future cfg-gated lift has one audited home.
#[inline(always)]
pub fn prefetch_read<T>(_ptr: &T) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent oracle: linear lower bound over a sorted slice.
    fn oracle_lower_bound(sorted: &[u64], target: u64) -> usize {
        let mut i = 0;
        while i < sorted.len() && sorted[i] < target {
            i += 1;
        }
        i
    }

    /// Builds layout + subtree sizes for a sorted slice.
    /// Max test size: the exhaustive sweep uses n <= 33 (34 slots).
    const MAXN: usize = 34;

    /// Fixed-buffer setup (crate is no_std; no alloc in tests).
    ///
    /// Returns the active slot count so callers slice `&eyt[..=n]` — the
    /// kernels take the exact-length layout, not the padded buffer.
    fn setup(sorted: &[u64]) -> ([u64; MAXN], [u32; MAXN], usize) {
        let n = sorted.len();
        let mut eyt = [0u64; MAXN];
        let mut full = [0u64; MAXN];
        full[..n].copy_from_slice(sorted);
        build_eytzinger_layout(&full[..n], &mut eyt[..=n]);
        let mut sizes = [0u32; MAXN];
        build_subtree_sizes(n + 1, &mut sizes);
        (eyt, sizes, n)
    }

    #[test]
    fn empty_layout() {
        let src: [u64; 0] = [];
        let mut eyt = [0u64; 1];
        build_eytzinger_layout(&src, &mut eyt);
        let sizes = [0u32; 1];
        assert_eq!(search_eytzinger_u64(&eyt, 0), None);
        assert_eq!(lower_bound_eytzinger(&eyt, &sizes, 0), 0);
    }

    #[test]
    fn single_element() {
        let src = [42u64];
        let mut eyt = [0u64; 2];
        build_eytzinger_layout(&src, &mut eyt);
        let mut sizes = [0u32; 2];
        build_subtree_sizes(eyt.len(), &mut sizes);
        assert_eq!(search_eytzinger_u64(&eyt, 42), Some(1));
        assert_eq!(search_eytzinger_u64(&eyt, 41), None);
        assert_eq!(lower_bound_eytzinger(&eyt, &sizes, 42), 0);
        assert_eq!(lower_bound_eytzinger(&eyt, &sizes, 43), 1);
    }

    #[test]
    fn ported_semantics_from_rdf_core() {
        let sorted = [100u64, 250, 400, 550, 700, 850, 1000];
        let (eyt, sizes, n) = setup(&sorted);
        let eyt = &eyt[..=n];
        let sizes = &sizes[..=n];
        assert_eq!(search_eytzinger_u64(eyt, 550), Some(1)); // root
        assert_eq!(search_eytzinger_u64(eyt, 100), Some(4)); // leftmost leaf
        assert_eq!(search_eytzinger_u64(eyt, 999), None);
        for &t in sorted.iter().chain([0u64, 999, 1001].iter()) {
            assert_eq!(search_by(eyt, t, |k| *k), search_eytzinger_u64(eyt, t));
        }
        for &t in sorted.iter().chain([0u64, 101, 999, 1001].iter()) {
            assert_eq!(
                lower_bound_eytzinger(eyt, sizes, t),
                oracle_lower_bound(&sorted, t)
            );
        }
    }

    #[test]
    fn permutation_round_trip() {
        let sorted = [10u64, 20, 30, 40];
        let mut perm = [0usize; 5];
        build_layout_permutation(&sorted, &mut perm);
        assert_eq!(perm[1..], [2, 1, 3, 0]);
        let mut inv = [9usize; 5];
        layout_permutation_inverse(&perm, &mut inv);
        for (slot, &sorted) in perm[1..].iter().enumerate() {
            assert_eq!(inv[sorted], slot + 1);
        }
    }

    #[test]
    fn non_power_of_two_and_duplicates() {
        let sorted = [5u64, 5, 5, 9, 9, 12, 30, 30, 41, 41, 41, 77];
        let (eyt, sizes, n) = setup(&sorted);
        let eyt = &eyt[..=n];
        let sizes = &sizes[..=n];
        for &t in &sorted {
            assert_eq!(
                lower_bound_eytzinger(eyt, sizes, t),
                oracle_lower_bound(&sorted, t)
            );
        }
        for t in [0u64, 4, 6, 8, 10, 13, 31, 42, 76, 78, u64::MAX] {
            assert_eq!(
                lower_bound_eytzinger(eyt, sizes, t),
                oracle_lower_bound(&sorted, t)
            );
        }
        if let Some(slot) = search_eytzinger_u64(eyt, 41) {
            assert_eq!(eyt[slot], 41);
        }
    }

    #[test]
    fn lower_bound_edges() {
        let sorted = [10u64, 20, 30, 40, 50];
        let (eyt, sizes, n) = setup(&sorted);
        let eyt = &eyt[..=n];
        let sizes = &sizes[..=n];
        assert_eq!(lower_bound_eytzinger(eyt, sizes, 1), 0);
        assert_eq!(lower_bound_eytzinger(eyt, sizes, 10), 0);
        assert_eq!(lower_bound_eytzinger(eyt, sizes, 11), 1);
        assert_eq!(lower_bound_eytzinger(eyt, sizes, 50), 4);
        assert_eq!(lower_bound_eytzinger(eyt, sizes, 51), 5);
        assert_eq!(lower_bound_eytzinger(eyt, sizes, u64::MAX), 5);
    }

    #[test]
    fn subtree_sizes_convention_boundary() {
        // n = 6 slots: slot 3's right child is slot 6 (a leaf). The two
        // candidate conventions differ exactly on right-descendant leaves:
        // layout-length (7) gives sizes = [_,6,3,2,1,1,1]; slot-count (6)
        // would give sizes[6] = 0, sizes[3] = 1, sizes[1] = 5.
        let sorted = [1u64, 2, 3, 4, 5, 6];
        let mut eyt = [0u64; 7];
        build_eytzinger_layout(&sorted, &mut eyt);
        let mut sizes = [0u32; 7];
        build_subtree_sizes(eyt.len(), &mut sizes);
        assert_eq!(sizes, [0, 6, 3, 2, 1, 1, 1]);
        // lower_bound consumes the sizes array; it must still agree with the
        // scalar oracle on this degenerate (missing-right-leaf-at-depth-2)
        // shape, where the undercounted variant would report 5 for targets
        // above the root.
        for t in 0..=7u64 {
            assert_eq!(
                lower_bound_eytzinger(&eyt, &sizes, t),
                oracle_lower_bound(&sorted, t),
                "t={t}"
            );
        }
        // Contrast witness: passing the slot count instead of the layout
        // length produces the undercounted table (sizes[6] = 0).
        let mut wrong = [0u32; 7];
        build_subtree_sizes(6, &mut wrong);
        assert_eq!(wrong, [0, 5, 3, 1, 1, 1, 0]);
    }

    #[test]
    fn subtree_sizes_hand_computed_tables() {
        // Hand-derived from sizes[k] = 1 + [2k<=n]sizes[2k] + [2k+1<=n]sizes[2k+1].
        let cases: [(usize, [u32; 8]); 5] = [
            (1, [0, 1, 0, 0, 0, 0, 0, 0]),
            (2, [0, 2, 1, 0, 0, 0, 0, 0]),
            (3, [0, 3, 1, 1, 0, 0, 0, 0]),
            (6, [0, 6, 3, 2, 1, 1, 1, 0]),
            (7, [0, 7, 3, 3, 1, 1, 1, 1]),
        ];
        for &(n, want) in &cases {
            let mut sizes = [0u32; 8];
            build_subtree_sizes(n + 1, &mut sizes);
            assert_eq!(sizes, want, "n={n}");
        }
    }

    #[test]
    fn subtree_sizes_perfect_tree_masking() {
        // WHY the stale OPT-42 ticket saw green suites: for a PERFECT tree
        // (n = 2^d - 1), lower_bound_eytzinger reads sizes only at even
        // indices 2k, and those all agree between the correct table and the
        // slot-count table — the dropped slot n (odd here) never propagates.
        // n = 7 with the WRONG table still matches the scalar oracle for
        // every target.
        let sorted = [1u64, 2, 3, 4, 5, 6, 7];
        let mut eyt = [0u64; 8];
        build_eytzinger_layout(&sorted, &mut eyt);
        let mut wrong = [0u32; 8];
        build_subtree_sizes(7, &mut wrong); // slot count, not layout length
        assert_eq!(wrong, [0, 6, 3, 2, 1, 1, 1, 0]); // sizes[7] dropped
        for t in 0..=8u64 {
            assert_eq!(
                lower_bound_eytzinger(&eyt, &wrong, t),
                oracle_lower_bound(&sorted, t),
                "perfect-tree masking violated at t={t}"
            );
        }
        // At n = 6 (NOT of the form 2^d - 1) the same mistake is NOT masked:
        // target 7 walks k=1 (reads sizes[2]=3, unchanged) then k=3 (reads
        // sizes[6]=0 instead of 1), reporting 5 where the answer is 6.
        let sorted6 = [1u64, 2, 3, 4, 5, 6];
        let mut eyt6 = [0u64; 7];
        build_eytzinger_layout(&sorted6, &mut eyt6);
        let mut wrong6 = [0u32; 7];
        build_subtree_sizes(6, &mut wrong6);
        assert_eq!(wrong6, [0, 5, 3, 1, 1, 1, 0]);
        assert_eq!(lower_bound_eytzinger(&eyt6, &wrong6, 7), 5);
        assert_eq!(oracle_lower_bound(&sorted6, 7), 6);
    }

    #[test]
    fn exhaustive_small_vs_oracle() {
        for n in [2usize, 3, 5, 6, 7, 9, 13, 15, 16, 17, 31, 33] {
            let mut sorted = [0u64; MAXN];
            for (i, v) in sorted.iter_mut().enumerate().take(n) {
                *v = (i as u64) * 3 + (i as u64 % 4);
            }
            let (eyt, sizes, nslots) = setup(&sorted[..n]);
            let eyt = &eyt[..=nslots];
            let sizes = &sizes[..=nslots];
            for t in 0..=(n as u64 * 4 + 2) {
                assert_eq!(
                    lower_bound_eytzinger(eyt, sizes, t),
                    oracle_lower_bound(&sorted[..n], t),
                    "n={n} target={t}"
                );
                if t % 3 == 0 {
                    let hit = sorted[..n].contains(&t);
                    assert_eq!(search_eytzinger_u64(eyt, t).is_some(), hit, "n={n} t={t}");
                }
            }
        }
    }
}

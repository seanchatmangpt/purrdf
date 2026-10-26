// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Cache-friendly, branchless 1-based Eytzinger search layout and traversal routines.

/// Interned 64-bit term identifier.
pub type TermId = u64;

/// Constructs a 1-based Eytzinger array from an existing sorted slice.
/// `dest` length must be `src.len() + 1`. `dest[0]` remains 0 as sentinel.
pub fn build_eytzinger_layout(src: &[TermId], dest: &mut [TermId]) {
    assert_eq!(dest.len(), src.len() + 1);
    let mut cursor = 0;
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

/// Branchless Eytzinger search over a 1-based contiguous array.
/// Returns Some(index) on exact match, or None. Emits `cmov` with zero branch misses.
#[inline(always)]
pub fn search_eytzinger_u64(eytzinger: &[TermId], target: TermId) -> Option<usize> {
    let n = eytzinger.len();
    if n <= 1 {
        return None;
    }

    let mut k: usize = 1;
    while k < n {
        let cmp = (target > eytzinger[k]) as usize;
        k = (k << 1) | cmp;
    }

    // Recover candidate leaf index
    k >>= k.trailing_zeros().min(1);
    k >>= 1;

    while k >= n {
        k >>= 1;
    }

    if k > 0 && eytzinger[k] == target {
        Some(k)
    } else {
        None
    }
}

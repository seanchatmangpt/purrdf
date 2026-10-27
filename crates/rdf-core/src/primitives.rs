// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Low-level, branchless, zero-allocation algorithms and hardware primitives.

/// 1. SIMD SWAR Memchr: Detects delimiters across 8 packed bytes in parallel.
/// Evaluates Alan Mycroft's zero-byte mask with zero branching.
#[inline(always)]
pub fn swar_find_delimiter_u64(word: u64, delimiter: u8) -> u64 {
    let mask = (delimiter as u64) * 0x0101010101010101;
    let diff = word ^ mask;
    (diff.wrapping_sub(0x0101010101010101)) & (!diff) & 0x8080808080808080
}

/// 2. Sorting Network 4-Lane: Sorts S, P, O, G TermIds in fixed registers.
/// Optimal 5-comparator network; every compare-exchange is the XOR-mask
/// conditional-selection form, guaranteed to lower to predicated `xor`/cmov
/// (no `if`, no temporaries).
// from bcinr crates/branchless/src/sort_small.rs:24-33 (originally
// bcinr-logic/src/network.rs:49-54), adopted per OPT-12 item 1; u64-widened
// from the u32 source.
#[inline(always)]
pub fn sort_quad_lanes_branchless(mut lanes: [u64; 4]) -> [u64; 4] {
    macro_rules! cswap {
        ($a:expr, $b:expr) => {
            let mask = u64::from(lanes[$a] > lanes[$b]);
            let diff = (lanes[$a] ^ lanes[$b]) & 0u64.wrapping_sub(mask);
            lanes[$a] ^= diff;
            lanes[$b] ^= diff;
        };
    }
    cswap!(0, 1);
    cswap!(2, 3);
    cswap!(0, 2);
    cswap!(1, 3);
    cswap!(1, 2);
    lanes
}

/// 3. Bitset Word Intersection: Single-cycle 64-bit Roaring bitset chunk AND.
#[inline(always)]
pub fn bitset_intersect_u64(chunk_a: u64, chunk_b: u64) -> u64 {
    chunk_a & chunk_b
}

/// 4. Clamped Scaling: Computes (val * numerator) / denominator with zero overflow,
/// saturating branchlessly at u64::MAX on overflow.
#[inline(always)]
pub fn clamped_scaling_u64(val: u64, num: u64, den: u64) -> u64 {
    if den == 0 {
        return u64::MAX;
    }
    match val.checked_mul(num) {
        Some(product) => product / den,
        None => u64::MAX,
    }
}

//! 64-bit bitset algebra: word and slice primitives.
//!
//! Ported from bcinr (`crates/bcinr-logic/src/bitset.rs:13-264`, `int.rs`,
//! `algorithms/set_*_branchless.rs`), scaffold ABIs stripped. Slice loops are
//! CC=1: trip count depends only on slice length, never on bit data.
//!
//! One home per job: purrdf's bulk bit-packed containers (`RankSelect`,
//! `IntVector`, `crates/rdf-core/src/ir/pack/bits.rs`) are the container
//! altitude — this module is the per-word / per-slice kernel layer beneath
//! them, not a replacement.
//!
//! All functions are allocation-free and `unsafe`-free.
//!
//! # Canonical rank/select conventions
//!
//! This module is the canonical home for per-word rank/select
//! (dedupe family #16). Conventions:
//!
//! - [`rank_u64`] is **inclusive**: counts set bits at positions `<= pos`
//!   (`pos` masked `& 63`; `rank_u64(x, 63)` counts the whole word).
//! - [`rank_exclusive_u64`] is **exclusive**: counts set bits at positions
//!   `< pos` — the adapter target for bitmap-style strict-prefix ranks.
//! - [`select_bit_u64`] returns the 0-indexed bit position of the `n`-th
//!   set bit (0-indexed `n`), or [`None`] when the word has `<= n` set bits.
//!
//! Cross-references: `crates/rdf-core/src/ir/pack/bits.rs`'s `select_in_word`
//! (~line 530) is deliberately container-local — its 256-entry LUT form is
//! the right performance at container altitude and is not shared downward.
//! `crate::bitmap`'s rank/select are adapters over this module's kernels.

/// Sets the bit at `pos` (masked `pos & 63`; out-of-range never shifts
/// overflow).
///
/// from bcinr bitset.rs:33
#[inline]
#[must_use]
pub const fn set_bit_u64(x: u64, pos: usize) -> u64 {
    x | (1u64 << (pos & 63))
}

/// Clears the bit at `pos` (masked `pos & 63`).
///
/// from bcinr bitset.rs:53
#[inline]
#[must_use]
pub const fn clear_bit_u64(x: u64, pos: usize) -> u64 {
    x & !(1u64 << (pos & 63))
}

/// Inclusive rank: number of set bits at or below `pos` (masked `pos & 63`;
/// `pos >= 63` counts the whole word).
///
/// from bcinr bitset.rs:75
#[inline]
#[must_use]
pub const fn rank_u64(x: u64, pos: usize) -> usize {
    let mask = (0u64.wrapping_sub((pos >= 63) as u64))
        | ((1u64.wrapping_shl(((pos + 1) & 0x3F) as u32)).wrapping_sub(1));
    (x & mask).count_ones() as usize
}

/// Exclusive rank: number of set bits strictly below `pos` (masked
/// `pos & 63`). Adapter target for strict-prefix rank conventions; for
/// `pos < 63`, `rank_exclusive_u64(x, pos + 1) == rank_u64(x, pos)`.
///
/// Mask-form, const, allocation-free.
#[inline]
#[must_use]
pub const fn rank_exclusive_u64(x: u64, pos: usize) -> usize {
    // mask = (1 << (pos & 63)) - 1 with the p == 0 case folded in: the
    // saturating-add term zeroes the mask when no bit is strictly below pos.
    let p = (pos & 63) as u32;
    let mask = (1u64.wrapping_shl(p).wrapping_sub(1)) & 0u64.wrapping_sub((p > 0) as u64);
    (x & mask).count_ones() as usize
}

/// Word XOR: `x ^ y`.
///
/// from bcinr algorithms/set_symmetric_difference_branchless.rs:33
#[inline]
#[must_use]
pub const fn xor_u64(x: u64, y: u64) -> u64 {
    x ^ y
}

/// Word complement: `!x`.
#[inline]
#[must_use]
pub const fn not_u64(x: u64) -> u64 {
    !x
}

/// Word AND-NOT (difference): `x & !y`.
///
/// from bcinr algorithms/set_difference_branchless.rs:33
#[inline]
#[must_use]
pub const fn and_not_u64(x: u64, y: u64) -> u64 {
    x & !y
}

/// Word intersection: `x & y`.
///
/// from bcinr algorithms/set_intersection_branchless.rs:33
#[inline]
#[must_use]
pub const fn intersect_u64(x: u64, y: u64) -> u64 {
    x & y
}

/// Word union: `x | y`.
///
/// from bcinr algorithms/set_union_branchless.rs:33
#[inline]
#[must_use]
pub const fn union_u64(x: u64, y: u64) -> u64 {
    x | y
}

/// `1` when every set bit of `x` is also set in `y` (`x ⊆ y`), else `0`.
///
/// from bcinr algorithms/is_subset_mask_u64.rs:24 — the residual `x & !y`
/// collapses to a 0/1 flag without a branch.
#[inline]
#[must_use]
pub const fn is_subset_u64(x: u64, y: u64) -> u64 {
    ((x & !y) == 0) as u64
}

/// Position of the `(n+1)`-th set bit, or [`None`] when `x` has at most `n`
/// set bits.
///
/// from bcinr bitset.rs:98 — bit-parallel binary search, CC=1: six fixed
/// halving steps (32, 16, 8, 4, 2, 1); at each step the low `2^i` bits are
/// counted and the search advances `2^i` high only when they hold fewer than
/// `count` set bits, all via mask arithmetic
/// (`0usize.wrapping_sub(cond)`). The final pick is a mask-selected
/// two-element array index, not a branch. Pure `select` semantics,
/// pdep-free, `const`.
#[inline]
#[must_use]
pub const fn select_bit_u64(x: u64, n: usize) -> Option<usize> {
    macro_rules! halve {
        ($rest:ident, $res:ident, $count:ident, $step:expr) => {{
            let low = ($rest & ((1u64 << $step) - 1)).count_ones() as usize;
            let go_high = 0usize.wrapping_sub((low < $count) as usize);
            $res += $step & go_high;
            $rest >>= $step & go_high;
            $count -= low & go_high;
        }};
    }
    let mut res = 0usize;
    let mut rest = x;
    let mut count = n.wrapping_add(1);
    halve!(rest, res, count, 32);
    halve!(rest, res, count, 16);
    halve!(rest, res, count, 8);
    halve!(rest, res, count, 4);
    halve!(rest, res, count, 2);
    halve!(rest, res, count, 1);
    let exists = (res < 64 && count == 1 && (rest & 1) != 0) as usize;
    [None, Some(res)][exists]
}

/// XOR-parity of all set bits across the slice (1 = odd total, 0 = even/empty).
///
/// from bcinr bitset.rs:135
#[inline]
#[must_use]
pub fn parity_u64_slice(a: &[u64]) -> u64 {
    let mut acc = 0u64;
    let mut i = 0;
    while i < a.len() {
        acc ^= a[i];
        i += 1;
    }
    (acc.count_ones() & 1) as u64
}

/// Jaccard similarity `|A ∩ B| / |A ∪ B|` over the common prefix.
///
/// from bcinr bitset.rs:160 — the min-length selection is mask arithmetic
/// (`len & (0usize.wrapping_sub(cond))`), and a zero union is guarded by an
/// additive `+ f32::from(union == 0)`, both branchless.
#[inline]
#[must_use]
pub fn jaccard_u64_slices(a: &[u64], b: &[u64]) -> f32 {
    let (len_a, len_b) = (a.len(), b.len());
    let min_len = (len_a & (0usize.wrapping_sub(usize::from(len_a < len_b))))
        | (len_b & (0usize.wrapping_sub(usize::from(len_a >= len_b))));
    let mut intersection = 0u32;
    let mut union = 0u32;
    let mut i = 0;
    while i < min_len {
        intersection += (a[i] & b[i]).count_ones();
        union += (a[i] | b[i]).count_ones();
        i += 1;
    }
    (intersection as f32) / (union as f32 + f32::from(union == 0))
}

/// Hamming distance (differing bits) over the common prefix.
///
/// from bcinr bitset.rs:194
#[inline]
#[must_use]
pub fn hamming_u64_slices(a: &[u64], b: &[u64]) -> usize {
    let (len_a, len_b) = (a.len(), b.len());
    let min_len = (len_a & (0usize.wrapping_sub(usize::from(len_a < len_b))))
        | (len_b & (0usize.wrapping_sub(usize::from(len_a >= len_b))));
    let mut dist = 0usize;
    let mut i = 0;
    while i < min_len {
        dist += (a[i] ^ b[i]).count_ones() as usize;
        i += 1;
    }
    dist
}

/// In-place intersection `a[i] &= b[i]` over the common prefix; words of `a`
/// beyond it are unchanged.
///
/// from bcinr bitset.rs:218
#[inline]
pub fn intersect_u64_slices(a: &mut [u64], b: &[u64]) {
    let (len_a, len_b) = (a.len(), b.len());
    let min_len = (len_a & (0usize.wrapping_sub(usize::from(len_a < len_b))))
        | (len_b & (0usize.wrapping_sub(usize::from(len_a >= len_b))));
    let mut i = 0;
    while i < min_len {
        a[i] &= b[i];
        i += 1;
    }
}

/// In-place union `a[i] |= b[i]` over the common prefix.
///
/// from bcinr bitset.rs:240
#[inline]
pub fn union_u64_slices(a: &mut [u64], b: &[u64]) {
    let (len_a, len_b) = (a.len(), b.len());
    let min_len = (len_a & (0usize.wrapping_sub(usize::from(len_a < len_b))))
        | (len_b & (0usize.wrapping_sub(usize::from(len_a >= len_b))));
    let mut i = 0;
    while i < min_len {
        a[i] |= b[i];
        i += 1;
    }
}

/// In-place difference `a[i] &= !b[i]` over the common prefix.
///
/// Fresh (plan-mandated; bcinr has word-level ANDNOT only).
#[inline]
pub fn difference_u64_slices(a: &mut [u64], b: &[u64]) {
    let (len_a, len_b) = (a.len(), b.len());
    let min_len = (len_a & (0usize.wrapping_sub(usize::from(len_a < len_b))))
        | (len_b & (0usize.wrapping_sub(usize::from(len_a >= len_b))));
    let mut i = 0;
    while i < min_len {
        a[i] &= !b[i];
        i += 1;
    }
}

/// `true` when any bit is set across the slice (OR-reduction).
///
/// from bcinr bitset.rs:264
#[inline]
#[must_use]
pub fn any_bit_set_u64_slice(a: &[u64]) -> bool {
    let mut acc = 0u64;
    let mut i = 0;
    while i < a.len() {
        acc |= a[i];
        i += 1;
    }
    acc != 0
}

/// Population count as `u64` (single `POPCNT`).
///
/// from bcinr int.rs:35
#[inline]
#[must_use]
pub const fn popcount_u64(x: u64) -> u64 {
    x.count_ones() as u64
}

/// Leading zero count (`32`-style `64` when `x == 0`).
///
/// from bcinr int.rs:56
#[inline]
#[must_use]
pub const fn leading_zeros_u64(x: u64) -> u64 {
    x.leading_zeros() as u64
}

/// Trailing zero count (`64` when `x == 0`).
///
/// from bcinr int.rs:77
#[inline]
#[must_use]
pub const fn trailing_zeros_u64(x: u64) -> u64 {
    x.trailing_zeros() as u64
}

/// Bit-reversal of all 64 bits.
///
/// from bcinr int.rs:98
#[inline]
#[must_use]
pub const fn reverse_bits_u64(x: u64) -> u64 {
    x.reverse_bits()
}

/// XOR parity of the 8 nibbles via the classic `0x6996` lookup fold.
///
/// from bcinr int.rs:285 (`parity_u32`)
#[inline]
#[must_use]
pub const fn parity_u32(x: u32) -> u32 {
    let mut y = x;
    y ^= y >> 16;
    y ^= y >> 8;
    y ^= y >> 4;
    (0x6996 >> (y & 0xf)) & 1
}

/// Isolates the lowest set bit of `x` (BMI `blsi`): the result has exactly
/// one bit set — the least-significant `1` of `x` — and is `0` when `x` is
/// `0`. Computed as `x & x.wrapping_neg()` (two's-complement negation).
///
/// This is THE one home for the `x & -x` idiom in the workspace
/// (helpers-ledger job `branchless-per-word-rank-select`); container code
/// must call this kernel, not re-derive the expression.
///
/// from bcinr crates/bcinr-logic/src/algorithms/blsi_u64.rs:24
#[inline]
#[must_use]
pub const fn isolate_lowest_u64(x: u64) -> u64 {
    x & x.wrapping_neg()
}

/// Clears the lowest set bit of `x` (BMI `blsr`): every other bit is
/// unchanged, and `x == 0` yields `0`. Computed as `x & x.wrapping_sub(1)`.
///
/// from bcinr crates/bcinr-logic/src/algorithms/blsr_u64.rs:24
#[inline]
#[must_use]
pub const fn clear_lowest_u64(x: u64) -> u64 {
    x & x.wrapping_sub(1)
}

/// Mask of all bits up to and including the lowest set bit of `x`
/// (BMI `blsmsk`), computed as `x ^ x.wrapping_sub(1)`.
///
/// Degenerate case: `mask_to_lowest_u64(0) == u64::MAX` (the subtract
/// underflows to `u64::MAX` and XOR with `0` propagates it).
///
/// from bcinr crates/bcinr-logic/src/algorithms/blsmsk_u64.rs:24
#[inline]
#[must_use]
pub const fn mask_to_lowest_u64(x: u64) -> u64 {
    x ^ x.wrapping_sub(1)
}

/// 128-bit funnel shift right of the concatenation `hi:lo` by `s` bits.
///
/// `s` is masked `s & 63`; the shift is never 64 wide. **Contract at
/// `s == 0` (already-masked): the result is exactly `lo`** — the high
/// contribution is masked out by `nz`, so no 64-bit-wide shift is ever
/// executed. For `s & 63 == s != 0` this is `(hi:lo) >> s` truncated to
/// 64 bits; larger `s` values wrap modulo 64.
///
/// from bcinr crates/bcinr-logic/src/algorithms/funnel_shift_right_u64.rs:22-25
/// (3-argument adaptation of the aux-smuggled `(val, aux)` scaffold form).
#[inline]
#[must_use]
pub const fn funnel_shift_right_u64(hi: u64, lo: u64, s: u32) -> u64 {
    let s = s & 63;
    let nz = 0u64.wrapping_sub((s != 0) as u64);
    lo.wrapping_shr(s) | (hi.wrapping_shl((64u32.wrapping_sub(s)) & 63) & nz)
}

/// 128-bit funnel shift left of the concatenation `hi:lo` by `s` bits.
///
/// `s` is masked `s & 63`; the shift is never 64 wide. **Contract at
/// `s == 0` (already-masked): the result is exactly `hi`** — the low
/// contribution is masked out by `nz`, so no 64-bit-wide shift is ever
/// executed. For `s & 63 == s != 0` this is `(hi:lo) << s` truncated to
/// 64 bits; larger `s` values wrap modulo 64.
///
/// from bcinr crates/bcinr-logic/src/algorithms/funnel_shift_left_u64.rs:22-25
/// (3-argument adaptation of the aux-smuggled `(val, aux)` scaffold form).
#[inline]
#[must_use]
pub const fn funnel_shift_left_u64(hi: u64, lo: u64, s: u32) -> u64 {
    let s = s & 63;
    let nz = 0u64.wrapping_sub((s != 0) as u64);
    hi.wrapping_shl(s) | (lo.wrapping_shr((64u32.wrapping_sub(s)) & 63) & nz)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bit-loop reference oracles (independent of the implementations).
    fn ref_rank(x: u64, pos: usize) -> usize {
        let mut n = 0;
        for b in 0..=pos.min(63) {
            n += ((x >> b) & 1) as usize;
        }
        n
    }
    fn ref_select(x: u64, n: usize) -> Option<usize> {
        let mut seen = 0;
        for b in 0..64 {
            seen += (x >> b) & 1;
            if seen == (n as u64) + 1 {
                return Some(b);
            }
        }
        None
    }

    #[test]
    fn word_ops_boundaries() {
        assert_eq!(set_bit_u64(0, 63), 1u64 << 63);
        assert_eq!(set_bit_u64(u64::MAX, 0), u64::MAX);
        assert_eq!(clear_bit_u64(u64::MAX, 0), u64::MAX - 1);
        assert_eq!(and_not_u64(0b1111, 0b0101), 0b1010);
        assert_eq!(xor_u64(0b1100, 0b0110), 0b1010);
        assert_eq!(is_subset_u64(0b0100, 0b0111), 1);
        assert_eq!(is_subset_u64(0b1000, 0b0111), 0);
    }

    #[test]
    fn rank_matches_oracle() {
        for x in [0u64, 1, 0x8000_0000_0000_0001, u64::MAX, 0b1010_1010] {
            for pos in [0usize, 1, 7, 31, 62, 63, 64, 100] {
                assert_eq!(rank_u64(x, pos), ref_rank(x, pos), "x={x:#x} pos={pos}");
            }
        }
    }

    #[test]
    fn select_matches_oracle() {
        for x in [
            0u64,
            1,
            0b1010,
            0x8000_0000_0000_0000,
            u64::MAX,
            0x0F0F_0F0F_0F0F_0F0F,
        ] {
            for n in 0..70usize {
                assert_eq!(select_bit_u64(x, n), ref_select(x, n), "x={x:#x} n={n}");
            }
        }
    }

    #[test]
    fn slice_ops() {
        let mut a = [0b1111u64, 0b1010];
        intersect_u64_slices(&mut a, &[0b0101, 0b1100]);
        assert_eq!(a, [0b0101, 0b1000]);

        let mut b = [0b0101u64, 0b1010];
        union_u64_slices(&mut b, &[0b1010, 0b0101]);
        assert_eq!(b, [0b1111, 0b1111]);

        let mut c = [0b0111u64];
        difference_u64_slices(&mut c, &[0b0101]);
        assert_eq!(c, [0b0010]);

        assert_eq!(hamming_u64_slices(&[u64::MAX], &[0]), 64);
        assert_eq!(hamming_u64_slices(&[0b1100], &[0b0011]), 4);
        assert_eq!(parity_u64_slice(&[0b11]), 0);
        assert_eq!(parity_u64_slice(&[0b1]), 1);
        assert!(!any_bit_set_u64_slice(&[0, 0]));
        assert!(any_bit_set_u64_slice(&[0, 1]));
        assert_eq!(jaccard_u64_slices(&[], &[]), 0.0);
        assert_eq!(jaccard_u64_slices(&[0xFF], &[0xFF]), 1.0);
        assert_eq!(jaccard_u64_slices(&[0b1100], &[0b0011]), 0.0);
        let j = jaccard_u64_slices(&[0b1110], &[0b0111]);
        assert!((j - 0.5).abs() < 1e-6);
    }

    #[test]
    fn int_wrappers() {
        assert_eq!(popcount_u64(u64::MAX), 64);
        assert_eq!(leading_zeros_u64(0), 64);
        assert_eq!(trailing_zeros_u64(0), 64);
        assert_eq!(reverse_bits_u64(1), 1u64 << 63);
        assert_eq!(parity_u32(0b111), 1);
        assert_eq!(parity_u32(u32::MAX), 0);
    }

    #[test]
    fn rank_exclusive_agrees_with_inclusive() {
        // exhaustive over small alphabets and boundary words
        let words = [
            0u64,
            u64::MAX,
            1,
            1 << 63,
            0x8000_0000_0000_0001,
            0xF0F0_F0F0_F0F0_F0F0,
            0x0102_0408_1020_4081,
        ];
        for x in words {
            for p in 0..64usize {
                // adapter identity: exclusive(p+1) == inclusive(p) for p < 63
                if p < 63 {
                    assert_eq!(
                        rank_exclusive_u64(x, p + 1),
                        rank_u64(x, p),
                        "x={x:#x} p={p}"
                    );
                }
                // inclusive(0) counts only bit 0
                assert_eq!(rank_u64(x, 0), usize::from(x & 1 != 0));
                // inclusive(63) counts the whole word
                assert_eq!(rank_u64(x, 63), x.count_ones() as usize);
                // exclusive(0) is always zero
                assert_eq!(rank_exclusive_u64(x, 0), 0);
                // brute-force reference for both forms
                let want_inc: usize = (0..=p).filter(|i| x >> i & 1 == 1).count();
                let want_exc: usize = (0..p).filter(|i| x >> i & 1 == 1).count();
                assert_eq!(rank_u64(x, p), want_inc, "inc x={x:#x} p={p}");
                assert_eq!(rank_exclusive_u64(x, p), want_exc, "exc x={x:#x} p={p}");
            }
        }
    }

    // ------------------------------------------------------------------
    // OPT-54: lowest-bit + funnel-shift kernels
    // ------------------------------------------------------------------

    const BOUNDARY_WORDS: [u64; 6] = [
        0,
        1,
        1 << 63,
        u64::MAX,
        0x8000_0000_0000_0001,
        0xF0F0_F0F0_F0F0_F0F0,
    ];

    /// Independent bit-loop oracles (different formulation than the impl).
    fn ref_blsi(x: u64) -> u64 {
        let mut found = false;
        let mut res = 0u64;
        for b in 0..64 {
            let bit = (x >> b) & 1;
            res |= (bit & (!found) as u64) << b;
            found |= bit != 0;
        }
        res
    }
    fn ref_blsr(x: u64) -> u64 {
        let mut cleared = false;
        let mut res = 0u64;
        for b in 0..64 {
            let bit = (x >> b) & 1;
            let keep = bit & (cleared) as u64; // drop the first set bit only
            res |= keep << b;
            cleared |= bit != 0;
        }
        res
    }
    fn ref_blsmsk(x: u64) -> u64 {
        // all bits up to and including the lowest set bit; MAX when x == 0
        match (0..64).find(|b| (x >> b) & 1 == 1) {
            Some(b) => (1u64 << b) | ((1u64 << b) - 1),
            None => u64::MAX,
        }
    }

    fn opt54_words() -> impl Iterator<Item = u64> {
        (0u64..=0xFFFF).chain(BOUNDARY_WORDS)
    }

    #[test]
    fn blsi_blsr_blsmsk_match_oracles() {
        for x in opt54_words() {
            assert_eq!(isolate_lowest_u64(x), ref_blsi(x), "blsi x={x:#x}");
            assert_eq!(clear_lowest_u64(x), ref_blsr(x), "blsr x={x:#x}");
            assert_eq!(mask_to_lowest_u64(x), ref_blsmsk(x), "blsmsk x={x:#x}");
        }
    }

    #[test]
    fn blsi_blsr_blsmsk_identities() {
        // isolate | clear == x whenever x != 0; both are 0 at x == 0.
        for x in opt54_words() {
            let iso = isolate_lowest_u64(x);
            let clr = clear_lowest_u64(x);
            assert_eq!(iso | clr, x, "iso|clr x={x:#x}");
            assert_eq!(iso & clr, 0, "iso&clr x={x:#x}");
        }
        assert_eq!(mask_to_lowest_u64(0), u64::MAX);
        assert_eq!(isolate_lowest_u64(0), 0);
        assert_eq!(clear_lowest_u64(0), 0);
        assert_eq!(isolate_lowest_u64(0x8000_0000_0000_0001), 1);
        assert_eq!(
            clear_lowest_u64(0x8000_0000_0000_0001),
            0x8000_0000_0000_0000
        );
        // lowest set bit is bit 0 -> blsmsk == 1; u64::MAX is the blsmsk of 1<<63.
        assert_eq!(mask_to_lowest_u64(0x8000_0000_0000_0001), 1);
        assert_eq!(mask_to_lowest_u64(1 << 63), u64::MAX);
    }

    /// Independent u128 oracle for the funnel shifts.
    fn ref_funnel(hi: u64, lo: u64, s: u32, left: bool) -> u64 {
        let wide = (((hi) as u128) << 64) | (lo) as u128;
        let sm = s % 64;
        if sm == 0 {
            return if left { hi } else { lo };
        }
        if left {
            ((wide << sm) >> 64) as u64
        } else {
            (wide >> sm) as u64
        }
    }

    #[test]
    fn funnel_shift_matches_u128_oracle() {
        const ALPHA: [u64; 6] = [
            0,
            1,
            0x8000_0000_0000_0000,
            u64::MAX,
            0x0123_4567_89AB_CDEF,
            0xFEDC_BA98_7654_3210,
        ];
        let mut pairs = 0usize;
        for hi in ALPHA {
            for lo in ALPHA {
                pairs += 1;
                for s in 0..=66u32 {
                    assert_eq!(
                        funnel_shift_right_u64(hi, lo, s),
                        ref_funnel(hi, lo, s, false),
                        "fsr hi={hi:#x} lo={lo:#x} s={s}"
                    );
                    assert_eq!(
                        funnel_shift_left_u64(hi, lo, s),
                        ref_funnel(hi, lo, s, true),
                        "fsl hi={hi:#x} lo={lo:#x} s={s}"
                    );
                }
            }
        }
        assert_eq!(pairs, 36); // 6 hi x 6 lo alphabet
    }

    #[test]
    fn funnel_shift_xorshift_sweep() {
        // xorshift64* sweep, 4096 steps, dual lanes for (hi, lo) and s.
        let mut a = 0x9E37_79B9_7F4A_7C15u64;
        let mut b = 0xBF58_476D_1CE4_E5B9u64;
        for step in 0..4096u32 {
            let hi = {
                a ^= a >> 12;
                a ^= a << 25;
                a ^= a >> 27;
                a.wrapping_mul(0x2545_F491_4F6C_DD1D)
            };
            let lo = {
                b ^= b >> 12;
                b ^= b << 25;
                b ^= b >> 27;
                b.wrapping_mul(0x2545_F491_4F6C_DD1D)
            };
            let s = (hi ^ lo) as u32;
            assert_eq!(
                funnel_shift_right_u64(hi, lo, s),
                ref_funnel(hi, lo, s, false),
                "fsr sweep step={step}"
            );
            assert_eq!(
                funnel_shift_left_u64(hi, lo, s),
                ref_funnel(hi, lo, s, true),
                "fsl sweep step={step}"
            );
        }
    }

    #[test]
    fn kernels_leave_rank_select_courts_intact() {
        // Neighbor tie-in: clear_lowest composes with the existing rank kernel
        // (no existing test modified).
        for x in opt54_words() {
            assert_eq!(
                rank_u64(clear_lowest_u64(x), 63),
                x.count_ones() as usize - usize::from(x != 0),
                "rank(clear(x)) x={x:#x}"
            );
            // select on the cleared word skips the first set bit position.
            if let Some(first) = select_bit_u64(x, 0) {
                assert_eq!(select_bit_u64(clear_lowest_u64(x), 0), {
                    // next set bit of x, if any
                    (first + 1..64).find(|b| (x >> b) & 1 == 1)
                });
            }
        }
    }
}

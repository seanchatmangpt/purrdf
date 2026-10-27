//! Bitmap cardinality and per-word succinct primitives.
//!
//! Ported from BranchlessCInRust (`bcinr`) `crates/bcinr-logic/src/`:
//! `algorithms/cardinality_linear_counting.rs:27-64` and
//! `algorithms/succinct_bit_vector_{rank,select}.rs` (scaffold ABIs stripped).
//!
//! This is the *primitive* layer only. Chunked bitmap containers — rank/select
//! structures, run-length encoding, leapfrog intersection — live in
//! `purrdf-core`'s pack layer (`crates/rdf-core/src/ir/pack/bits.rs`,
//! `RankSelect`). Roaring containers and run-length primitives do not exist in
//! bcinr and are deliberately not built here.

/// Add one element to a linear-counting bitmap.
///
/// The bitmap holds `m = bitmap.len() * 64` bits; the element's hash selects
/// one bit to set. The estimator ([`linear_counting_estimate`]) later counts
/// zero bits to estimate distinct cardinality.
///
/// from bcinr `algorithms/cardinality_linear_counting.rs:27`.
///
/// # Examples
/// ```
/// use purrdf_branchless::bitmap::{linear_counting_add, linear_counting_estimate};
/// let mut bitmap = [0u64; 4];
/// linear_counting_add(&mut bitmap, 0xAAAA_BBBB);
/// assert!(linear_counting_estimate(&bitmap) >= 1);
/// ```
#[inline]
pub fn linear_counting_add(bitmap: &mut [u64], hash: u64) {
    // The single guard branch: an empty bitmap has no bit to set.
    if bitmap.is_empty() {
        return;
    }
    let m = bitmap.len() * 64;
    // Widen before the modulo: `hash as usize` truncates on wasm32 (usize=32),
    // making the same input hash to different bits across platforms.
    let bit_pos = (hash % m as u64) as usize;
    bitmap[bit_pos / 64] |= 1u64 << (bit_pos % 64);
}

/// Estimate distinct cardinality from a linear-counting bitmap.
///
/// The textbook estimator is `-m · ln(V/m)` where `V` is the zero-bit count
/// and `m` the total bit count. This integer form is deliberately simpler:
/// it returns the number of occupied hash slots (`m - V`), which is
///
/// - **exact** for small cardinalities (collisions rare),
/// - a **lower bound** of the true distinct count as fill grows,
/// - `0` on an empty bitmap and `m` on a fully saturated one.
///
/// The log correction that matters at high fill ratios is intentionally not
/// approximated here; use the estimate for gating and relative comparison,
/// not as an exact count near saturation.
///
/// from bcinr `algorithms/cardinality_linear_counting.rs:64`.
///
/// # Examples
/// ```
/// use purrdf_branchless::bitmap::linear_counting_estimate;
/// let bitmap = [0u64; 64];
/// assert_eq!(linear_counting_estimate(&bitmap), 0);
/// assert_eq!(linear_counting_estimate(&[u64::MAX; 4]), 256);
/// ```
#[inline]
pub fn linear_counting_estimate(bitmap: &[u64]) -> u64 {
    let m = bitmap.len() as u64 * 64;
    if m == 0 {
        return 0;
    }
    let set_bits = bitmap
        .iter()
        .fold(0u64, |acc, w| acc.wrapping_add(u64::from(w.count_ones())));
    let zeros = m.saturating_sub(set_bits);
    // Exactly one of the two flags is set (or neither, when the bitmap is
    // empty of elements): saturated → m, normal → set_bits, empty → 0.
    let is_saturated = u64::from(zeros == 0);
    let is_normal = u64::from(zeros > 0 && zeros < m);
    m.wrapping_mul(is_saturated)
        .wrapping_add(set_bits.wrapping_mul(is_normal))
}

/// Per-word rank: number of set bits strictly below position `pos & 63`.
///
/// Convention adapter over the canonical [`crate::bitset::rank_u64`] (which is
/// **inclusive**: bits at positions ≤ pos): the exclusive rank at `p` is the
/// inclusive rank at `p − 1`, and the exclusive rank at position 0 is 0.
///
/// from bcinr `algorithms/succinct_bit_vector_rank.rs:24` (scaffold ABI
/// stripped); body deduplicated onto the bitset kernel.
///
/// # Examples
/// ```
/// use purrdf_branchless::bitmap::rank_word;
/// assert_eq!(rank_word(0b1011, 2), 2);
/// assert_eq!(rank_word(0b1011, 0), 0);
/// assert_eq!(rank_word(0b1011, 64), 0); // positions wrap: 64 & 63 == 0 → p=0 → 0
/// assert_eq!(rank_word(0b1011, 63), 3); // full-word exclusive rank
/// ```
#[inline]
pub const fn rank_word(word: u64, pos: usize) -> u64 {
    let p = pos & 63;
    let inclusive_below = crate::bitset::rank_u64(word, p.wrapping_sub(1) & 63);
    // p == 0 → exclusive rank is 0 (rank_u64(_, 63) is the full-word rank).
    let is_zero = (p == 0) as usize;
    (inclusive_below as u64) * (1 - is_zero) as u64
}

/// Per-word select: 0-based index of the `(n + 1)`-th set bit.
///
/// Sentinel-`64` adapter over the canonical [`crate::bitset::select_bit_u64`]
/// (which returns `Option<usize>`). Returns `64` when the word has fewer than
/// `n + 1` set bits.
///
/// from bcinr `algorithms/succinct_bit_vector_select.rs:24` (scaffold ABI
/// stripped); body deduplicated onto the bitset kernel.
///
/// # Examples
/// ```
/// use purrdf_branchless::bitmap::select_word;
/// assert_eq!(select_word(0b1011, 0), 0);
/// assert_eq!(select_word(0b1011, 2), 3);
/// assert_eq!(select_word(0b1011, 3), 64); // only 3 bits set
/// ```
#[inline]
pub const fn select_word(word: u64, n: u64) -> u64 {
    match crate::bitset::select_bit_u64(word, n as usize) {
        Some(pos) => pos as u64,
        None => 64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Loop-based oracle, structurally distinct from the masked forms above.
    fn rank_reference(word: u64, pos: usize) -> u64 {
        let mut count = 0u64;
        let mut i = 0usize;
        while i < (pos & 63) {
            count += (word >> i) & 1;
            i += 1;
        }
        count
    }

    fn select_reference(word: u64, n: u64) -> u64 {
        let mut seen = 0u64;
        let mut i = 0u32;
        while i < 64 {
            seen += (word >> i) & 1;
            if seen == n + 1 {
                return u64::from(i);
            }
            i += 1;
        }
        64
    }

    fn set_bits(bitmap: &[u64]) -> u64 {
        bitmap
            .iter()
            .fold(0u64, |a, w| a + u64::from(w.count_ones()))
    }

    #[test]
    fn add_sets_exactly_one_bit() {
        let mut bitmap = [0u64; 4];
        linear_counting_add(&mut bitmap, 0xABCD);
        assert_eq!(set_bits(&bitmap), 1);
    }

    #[test]
    fn duplicate_add_is_idempotent() {
        let mut bitmap = [0u64; 8];
        linear_counting_add(&mut bitmap, 42);
        let first = bitmap;
        linear_counting_add(&mut bitmap, 42);
        assert_eq!(bitmap, first);
    }

    #[test]
    fn add_lands_in_the_selected_word() {
        // hash 64 maps to bit 64 → word 1, bit 0.
        let mut bitmap = [0u64; 4];
        linear_counting_add(&mut bitmap, 64);
        assert_eq!(bitmap[1], 1);
        assert_eq!(bitmap[0], 0);
    }

    #[test]
    fn empty_bitmap_estimate_is_zero() {
        assert_eq!(linear_counting_estimate(&[0u64; 16]), 0);
    }

    #[test]
    fn zero_length_bitmap_does_not_panic() {
        assert_eq!(linear_counting_estimate(&[]), 0);
        let mut empty: [u64; 0] = [];
        linear_counting_add(&mut empty, 42);
    }

    #[test]
    fn saturated_bitmap_estimates_m() {
        assert_eq!(linear_counting_estimate(&[u64::MAX; 4]), 256);
    }

    #[test]
    fn estimate_is_monotone_under_distinct_adds() {
        let mut bitmap = [0u64; 16];
        let mut prev = 0u64;
        for i in 0u64..50 {
            linear_counting_add(&mut bitmap, i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let est = linear_counting_estimate(&bitmap);
            assert!(est >= prev);
            prev = est;
        }
        assert!(prev >= 5);
    }

    #[test]
    fn rank_matches_loop_oracle() {
        for word in [0u64, 1, 0b1011, u64::MAX, 0xDEAD_BEEF_CAFE_F00D] {
            for pos in [0usize, 1, 2, 31, 63, 64, 127] {
                assert_eq!(rank_word(word, pos), rank_reference(word, pos));
            }
        }
    }

    #[test]
    fn select_matches_loop_oracle() {
        for word in [0u64, 1, 0b1011, u64::MAX, 0x8000_0000_0000_0001] {
            for n in [0u64, 1, 2, 3, 63] {
                assert_eq!(select_word(word, n), select_reference(word, n));
            }
        }
    }

    #[test]
    fn boundaries() {
        assert_eq!(rank_word(0, 63), 0);
        assert_eq!(rank_word(u64::MAX, 63), 63);
        assert_eq!(select_word(u64::MAX, 63), 63);
        assert_eq!(select_word(u64::MAX, 64), 64);
        assert_eq!(select_word(0, 0), 64);
        let mut one = [0u64; 1];
        linear_counting_add(&mut one, 0);
        linear_counting_add(&mut one, 64); // m = 64: 64 % 64 == 0, collapses onto bit 0
        assert_eq!(set_bits(&one), 1);
        linear_counting_add(&mut one, u64::MAX); // u64::MAX % 64 == 63: sets the top bit
        assert_eq!(set_bits(&one), 2);
    }
}

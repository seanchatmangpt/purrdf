//! Branchless search primitives.
//!
//! Ported from bcinr `crates/bcinr-logic/src/algorithms/` (commit `1e92b213`,
//! branch `bench/rdtsc-tick-tables`). Scaffold ABIs (`#[no_mangle]`,
//! `(val, aux)` u64 pairs) are stripped into named parameters; the fixed-trip
//! loop shapes and mask-select arithmetic are preserved verbatim.
//!
//! On the bcinr oracle bug: the directive flagged `linear_search_simd_u8`'s
//! reference oracle as MSB→LSB while the implementation is LSB→MSB. In this
//! tree the published reference happens to agree, but the implementation's
//! correctness rests on a subtle overwrite property (later, smaller indices
//! win), so [`first_match_byte_u64`] ships with an explicit multi-match
//! regression test that fails for any MSB-first or last-match regression.

/// Branchless lower bound of `key` in the sorted identity array
/// `[0, 1, ..., len - 1]`.
///
/// Returns the insertion index: the number of elements strictly less than
/// `key`, i.e. `min(key, len)`. The 32-bit result index is built one bit at a
/// time from the most-significant bit; each bit is kept only when the prefix
/// stays within both `len` and `key` (mask-select, no data-dependent branch).
///
/// Ported from bcinr `algorithms/unrolled_binary_search_u32.rs:19`, renamed
/// for honesty: the algorithm is a lower bound over the *identity* array, not
/// a general binary search.
#[inline]
#[must_use]
pub fn lower_bound_identity_u32(len: u32, key: u32) -> u32 {
    let len = (len) as u64;
    let k = (key) as u64;
    // Build the largest `pos` in [0, len] with pos <= key, bit by bit,
    // high to low. `ok` selects whether bit `b` stays set.
    let mut pos = 0u64;
    let mut b = 31i32;
    while b >= 0 {
        let bit = 1u64 << b;
        let cand = pos | bit;
        let ok = (cand <= len) as u64 & (cand <= k) as u64;
        pos |= bit & 0u64.wrapping_sub(ok);
        b -= 1;
    }
    pos as u32
}

/// Branchless 4-lane lower-bound count over packed `u16` lanes.
///
/// `word` packs four `u16` lanes (least-significant lane first); returns the
/// number of lanes strictly less than `target` — the lower-bound position of
/// `target` in a 4-element sorted array stored lane-packed.
///
/// Ported from bcinr `algorithms/lower_bound_branchless_u32.rs:33`.
#[inline]
#[must_use]
pub fn lower_bound_lanes_u16x4(word: u64, target: u16) -> u32 {
    let t = i32::from(target);
    let lane = |shift: u32| -> bool { i32::from(((word >> shift) & 0xFFFF) as u16) < t };
    (lane(0)) as u32 + (lane(16)) as u32 + (lane(32)) as u32 + (lane(48)) as u32
}

/// Branchless `equal_range` over the singleton sorted array `{x}`.
///
/// Returns `(lower, upper)` where `lower = (x < k) as u32` and
/// `upper = (x <= k) as u32` — the degenerate singleton-array semantics are
/// the documented contract (port of bcinr
/// `algorithms/equal_range_branchless_u32.rs:33`, which packs the pair into
/// one u64; the tuple is the honest shape).
#[inline]
#[must_use]
pub fn equal_range_singleton_u32(x: u32, k: u32) -> (u32, u32) {
    let lower = (x < k) as u32;
    let upper = (x <= k) as u32;
    (lower, upper)
}

/// Branchless first-match byte index in the 8 byte lanes of `word`.
///
/// Returns the index (0..8) of the *first* (lowest) byte equal to `target`,
/// or [`None`] when no lane matches.
///
/// Delegates to [`crate::swar::first_byte_position`] — the canonical O(1)
/// Mycroft-mask + `trailing_zeros` kernel (one mask build, one `tz`, no
/// 8-iteration walk). The original bcinr multiplexer accumulation
/// (`algorithms/linear_search_simd_u8.rs:33`, min-index-wins over lanes
/// walked 7→0) computes the identical lowest-index answer; the O(1) form
/// replaces it as the shared home for "first lane equal to byte".
// from bcinr algorithms/linear_search_simd_u8.rs:33 (semantics preserved;
// body delegated to purrdf_branchless::swar::first_byte_position)
#[inline]
#[must_use]
pub fn first_match_byte_u64(word: u64, target: u8) -> Option<usize> {
    crate::swar::first_byte_position(word, target).map(|p| p as usize)
}

/// Branchless first-match lane index in an 8-lane `u64` slice.
///
/// Returns the index (0..8) of the *first* (lowest) lane equal to `target`,
/// or [`None`] when no lane matches. This is the u64-word-lane widening of
/// the in-tree byte form [`crate::swar::first_byte_position`]
/// (`first_match_byte_u64`).
///
/// # Why OR-accumulate is required, not stylistic
///
/// The per-lane equality bit is accumulated into `hits` with bitwise **OR**
/// (`((eq) as u64).wrapping_neg()` is an all-ones mask, so OR is
/// set-bit union). XOR accumulation would *cancel* repeated matches: for the
/// multi-match class (an even number of equal lanes, e.g. the dedup /
/// idempotence workload) XOR yields `hits == 0` and wrongly reports a miss.
/// OR is order-free and idempotent, which is exactly what makes the
/// `trailing_zeros` extraction the lowest matching lane.
///
/// # Padding is the caller's contract
///
/// The slice is padded to 8 lanes by the *caller* (zero-staging). Zero
/// padding answers `Some(2)` for `target == 0` when the logical length is 2 —
/// the padding lanes match. Callers must guard, as
/// `crates/sparql-eval/src/substitute.rs::probe_index` does (`index >= len` →
/// miss), that a hit index is re-checked against the logical length. For
/// convenience this kernel itself treats a slice shorter than 8 lanes as
/// zero-padded (identical hazard) and reads only the first 8 lanes of a
/// longer slice.
///
/// # Oracle note
///
/// The bcinr reference oracle for `linear_search_simd_u8` is MSB→LSB while
/// its implementation is LSB→MSB (lowest index wins); see the module header.
/// The in-file tests use an independent LSB-first `position` oracle and
/// hardcoded multi-match regressions that fail under any MSB-first or
/// last-match reading.
// from bcinr algorithms/linear_search_simd_u8.rs:33-46 (lowest-index semantics;
// OR-accumulate+tz body per swar::first_byte_position, widened to u64 lanes)
#[inline]
#[must_use]
pub fn first_equal_lane_u64x8(lanes: &[u64], target: u64) -> Option<usize> {
    // Unrolled 8-lane equality: straight-line mask arithmetic, no control
    // flow. Missing lanes (len < 8) read as zero padding — see the padding
    // contract above; lanes past index 7 are ignored.
    let mut hits = 0u64;
    hits |= ((lanes.get(0).copied() == Some(target)) as u64).wrapping_neg();
    hits |= ((lanes.get(1).copied() == Some(target)) as u64).wrapping_neg();
    hits |= ((lanes.get(2).copied() == Some(target)) as u64).wrapping_neg();
    hits |= ((lanes.get(3).copied() == Some(target)) as u64).wrapping_neg();
    hits |= ((lanes.get(4).copied() == Some(target)) as u64).wrapping_neg();
    hits |= ((lanes.get(5).copied() == Some(target)) as u64).wrapping_neg();
    hits |= ((lanes.get(6).copied() == Some(target)) as u64).wrapping_neg();
    hits |= ((lanes.get(7).copied() == Some(target)) as u64).wrapping_neg();
    // Miss (hits == 0) => trailing_zeros() == 64, which `usize::try_from`
    // would happily accept as Some(64) even on 32-bit targets — bound the
    // result to the 8-lane domain so a miss is `None` on every width.
    usize::try_from(hits.trailing_zeros())
        .ok()
        .filter(|&i| i < 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Independent oracles (deliberately different formulation) ----------

    fn oracle_lower_bound_identity(len: u32, key: u32) -> u32 {
        // Loop-based binary search over the identity array.
        let mut lo = 0u64;
        let mut hi = (len) as u64;
        let k = (key) as u64;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if mid < k {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo as u32
    }

    fn oracle_lower_bound_lanes(word: u64, target: u16) -> u32 {
        let lanes = [
            (word & 0xFFFF) as u16,
            ((word >> 16) & 0xFFFF) as u16,
            ((word >> 32) & 0xFFFF) as u16,
            ((word >> 48) & 0xFFFF) as u16,
        ];
        lanes.iter().filter(|&&l| l < target).count() as u32
    }

    fn oracle_first_match(word: u64, target: u8) -> Option<usize> {
        // Correct oracle: explicit LSB-first first match. (Contrast the
        // fragile overwrite property of the kernel under test.)
        (0..8).find(|&i| ((word >> (i * 8)) & 0xFF) == (target) as u64)
    }

    // --- lower_bound_identity_u32 ------------------------------------------

    #[test]
    fn lower_bound_identity_matches_oracle_and_boundaries() {
        for &(len, key) in &[
            (42, 13),
            (0, 0),
            (0, u32::MAX),
            (u32::MAX, 0),
            (u32::MAX, u32::MAX),
            (u32::MAX, u32::MAX - 1),
            (7, 7),
            (7, 8),
            (1 << 31, 1 << 31),
            (u32::MAX, 1 << 16),
        ] {
            assert_eq!(
                lower_bound_identity_u32(len, key),
                oracle_lower_bound_identity(len, key),
                "len={len} key={key}"
            );
        }
        // Hardcoded boundary: min(key, len) semantics.
        assert_eq!(lower_bound_identity_u32(10, 3), 3);
        assert_eq!(lower_bound_identity_u32(10, 10), 10);
        assert_eq!(lower_bound_identity_u32(10, 99), 10);
        assert_eq!(lower_bound_identity_u32(0, 99), 0);
    }

    // --- lower_bound_lanes_u16x4 --------------------------------------------

    #[test]
    fn lower_bound_lanes_matches_oracle_and_boundaries() {
        for &(word, target) in &[
            (0x0004_0003_0002_0001, 3),
            (0, 0),
            (u64::MAX, u16::MAX),
            (u64::MAX, 0),
            (0x0005_0003_0003_0001, 3),
            (0x8000_0006_0002_0000, 7),
        ] {
            assert_eq!(
                lower_bound_lanes_u16x4(word, target),
                oracle_lower_bound_lanes(word, target),
                "word={word:#018x} target={target}"
            );
        }
        // Exact-count boundaries: all below, none below, ties excluded.
        assert_eq!(lower_bound_lanes_u16x4(0x0002_0002_0002_0002, 3), 4);
        assert_eq!(lower_bound_lanes_u16x4(0x0002_0002_0002_0002, 2), 0);
    }

    // --- equal_range_singleton_u32 -------------------------------------------

    #[test]
    fn equal_range_singleton_boundaries() {
        // x < k: both bounds 1. x == k: lower 0, upper 1. x > k: both 0.
        assert_eq!(equal_range_singleton_u32(1, 2), (1, 1));
        assert_eq!(equal_range_singleton_u32(2, 2), (0, 1));
        assert_eq!(equal_range_singleton_u32(3, 2), (0, 0));
        assert_eq!(equal_range_singleton_u32(0, u32::MAX), (1, 1));
        assert_eq!(equal_range_singleton_u32(u32::MAX, 0), (0, 0));
    }

    // --- first_match_byte_u64 (multi-match regression) -----------------------

    #[test]
    fn first_match_byte_is_lowest_matching_index() {
        for &(word, target) in &[
            (0x1122_3344_5566_7788, 0x77),
            (0, 0),
            (u64::MAX, 0xFF),
            (u64::MAX, 0),
            (0x0102_0304_0506_0708, 0x42),
        ] {
            assert_eq!(
                first_match_byte_u64(word, target),
                oracle_first_match(word, target),
                "word={word:#018x} target={target:#04x}"
            );
        }

        // MULTI-MATCH regression: target present in lanes 6, 2, and 0.
        // Lowest index (0) must win. An MSB-first scan or a last-match
        // regression would return 6 or 2 instead.
        let word = 0x4277_0042_5544_0042_u64; // byte0=0x42, byte2=0x42, byte6=0x42
        assert_eq!(first_match_byte_u64(word, 0x42), Some(0));
        // Multi-match with only high lanes: lowest of {4, 7} = 4.
        let word_hi = 0x4200_0042_0000_0000_u64; // byte4=0x42, byte7=0x42
        assert_eq!(first_match_byte_u64(word_hi, 0x42), Some(4));
        // Miss.
        assert_eq!(first_match_byte_u64(word, 0x99), None);
    }

    // --- first_equal_lane_u64x8 ----------------------------------------------

    /// Independent LSB-first oracle: a plain scalar `position` scan.
    /// Deliberately a different formulation from the OR-accumulate + tz
    /// kernel, and deliberately NOT the bcinr MSB→LSB reference form.
    fn oracle_first_equal_lane(lanes: &[u64], target: u64) -> Option<usize> {
        lanes.iter().position(|&l| l == target)
    }

    #[test]
    fn first_equal_lane_matches_lsb_first_oracle() {
        // Table carried from the sparql-eval differential
        // (crates/sparql-eval/src/substitute.rs probe-index cases).
        let lanes = [7u64, 0, 7, 0, 9, 7, 0, 1];
        for &target in &[7u64, 0, 9, 42] {
            assert_eq!(
                first_equal_lane_u64x8(&lanes, target),
                oracle_first_equal_lane(&lanes, target),
                "target={target}"
            );
        }
        // Hardcoded expectations for the same table.
        assert_eq!(first_equal_lane_u64x8(&lanes, 7), Some(0));
        assert_eq!(first_equal_lane_u64x8(&lanes, 0), Some(1));
        assert_eq!(first_equal_lane_u64x8(&lanes, 9), Some(4));
        assert_eq!(first_equal_lane_u64x8(&lanes, 42), None);
    }

    #[test]
    fn first_equal_lane_boundary_lanes() {
        // All-zero lanes, target 0: first lane wins.
        assert_eq!(first_equal_lane_u64x8(&[0u64; 8], 0), Some(0));
        // All-MAX lanes, MAX target.
        assert_eq!(first_equal_lane_u64x8(&[u64::MAX; 8], u64::MAX), Some(0));
        // Hit only in the high lanes {6, ...}: lowest of the two wins.
        let hi = [1u64, 2, 3, 4, 5, 6, 42, 42];
        assert_eq!(first_equal_lane_u64x8(&hi, 42), Some(6));
        let hi7 = [1u64, 2, 3, 4, 5, 6, 7, 42];
        assert_eq!(first_equal_lane_u64x8(&hi7, 42), Some(7));
    }

    #[test]
    fn first_equal_lane_multi_match_lowest_wins() {
        // MULTI-MATCH regression (mirrors first_match_byte_u64's): target
        // present at lanes {6, 2, 0} — lowest (0) must win. An MSB-first or
        // last-match regression would answer 6 or 2.
        let a = [42u64, 1, 42, 3, 4, 5, 42, 7];
        assert_eq!(first_equal_lane_u64x8(&a, 42), Some(0));
        // Target at {7, 4}: lowest (4) wins.
        let b = [0u64, 1, 2, 3, 42, 5, 6, 42];
        assert_eq!(first_equal_lane_u64x8(&b, 42), Some(4));
    }

    #[test]
    fn first_equal_lane_padding_hazard_is_documented() {
        // Logical array [1, 2] staged (zero-padded) into 8 lanes. With
        // target 0 the padding lanes MATCH, so the answer is Some(2) — an
        // index past the logical length. The caller owns the guard:
        // crates/sparql-eval/src/substitute.rs::probe_index rejects
        // `index >= len` before using the hit.
        let logical = [1u64, 2];
        let staged = [1u64, 2, 0, 0, 0, 0, 0, 0];
        assert_eq!(first_equal_lane_u64x8(&staged, 0), Some(2));
        // The kernel itself applies the same zero-padding convention to
        // short slices, so the unstaged slice answers identically — the
        // hazard is the caller's to guard either way.
        assert_eq!(first_equal_lane_u64x8(&logical, 0), Some(2));
        assert_eq!(first_equal_lane_u64x8(&staged, 1), Some(0));
        assert_eq!(first_equal_lane_u64x8(&staged, 2), Some(1));
    }

    #[test]
    fn first_equal_lane_miss_is_none() {
        assert_eq!(
            first_equal_lane_u64x8(&[7u64, 0, 7, 0, 9, 7, 0, 1], 42),
            None
        );
        assert_eq!(first_equal_lane_u64x8(&[0u64; 8], 1), None);
        assert_eq!(first_equal_lane_u64x8(&[u64::MAX; 8], 0), None);
        // Empty slice: every lane reads as zero padding, so only target 0
        // "hits" (at padding lane 0); anything else is a clean miss.
        assert_eq!(first_equal_lane_u64x8(&[], 5), None);
    }
}

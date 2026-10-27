//! Branchless selection: medians, order statistics, merges, stable argsort.
//!
//! Ported from bcinr `crates/bcinr-logic/src/algorithms/` (commit `1e92b213`),
//! with the generated `(val, aux)` scaffold ABI widened to explicit parameters
//! and `#[no_mangle]` stripped. Every function is constant-shape: fixed
//! comparator networks or rank arithmetic, no data-dependent branches, no heap.

/// Median of three `u32` values: `max(min(a,b), min(max(a,b), c))`.
///
/// Pure min/max — lowers to `cmov`/`smax` with zero branches.
// from bcinr crates/bcinr-logic/src/algorithms/median3_u32.rs:16-26
#[must_use]
#[inline]
pub const fn median3_u32(a: u32, b: u32, c: u32) -> u32 {
    max_u32(min_u32(a, b), min_u32(max_u32(a, b), c))
}

/// `const`-stable mask-form `min` (`Ord::min` is not const-callable yet).
#[must_use]
#[inline]
pub const fn min_u32(a: u32, b: u32) -> u32 {
    b ^ ((a ^ b) & (((a < b) as u32).wrapping_neg()))
}

/// `const`-stable mask-form `max`.
#[must_use]
#[inline]
pub const fn max_u32(a: u32, b: u32) -> u32 {
    a ^ ((a ^ b) & (((a < b) as u32).wrapping_neg()))
}

/// `const`-stable mask-form `min` over `u64`.
#[must_use]
#[inline]
pub const fn min_u64(a: u64, b: u64) -> u64 {
    b ^ ((a ^ b) & (((a < b) as u64).wrapping_neg()))
}

/// `const`-stable mask-form `max` over `u64`.
#[must_use]
#[inline]
pub const fn max_u64(a: u64, b: u64) -> u64 {
    a ^ ((a ^ b) & (((a < b) as u64).wrapping_neg()))
}

/// Median of five `u32` values via the 9-comparator Knuth sorting network.
///
/// Every compare-exchange is a `min`/`max` split (guaranteed `cmov` form);
/// the network is fully unrolled and returns the middle element.
// from bcinr crates/bcinr-logic/src/algorithms/median5_u32.rs:17-49
#[must_use]
#[inline]
pub const fn median5_u32(v0: u32, v1: u32, v2: u32, v3: u32, v4: u32) -> u32 {
    let (mut v0, mut v1, mut v2, mut v3, mut v4) = (v0, v1, v2, v3, v4);

    macro_rules! ce {
        ($a:ident, $b:ident) => {{
            let lo = min_u32($a, $b);
            let hi = max_u32($a, $b);
            $a = lo;
            $b = hi;
        }};
    }
    ce!(v0, v1);
    ce!(v3, v4);
    ce!(v2, v4);
    ce!(v2, v3);
    ce!(v0, v3);
    ce!(v0, v2);
    ce!(v1, v4);
    ce!(v1, v3);
    ce!(v1, v2);
    // Final comparator side-outputs (v0, v1, v3, v4) are unread by design —
    // only the middle lane v2 is the median.
    let _ = (v0, v1, v3, v4);
    v2
}

/// Median of nine `u8` values: the element of stable rank 4 (5th smallest).
///
/// Stable rank per element = count of strictly-smaller elements plus
/// equal-but-earlier elements (`j < i` tiebreak); the value whose rank is
/// exactly 4 is selected by an arithmetic mask. Fixed 9×9 predicate sums.
// from bcinr crates/bcinr-logic/src/algorithms/median9_u32.rs:22-63
#[must_use]
#[inline]
pub fn median9_u8(b: [u8; 9]) -> u8 {
    let wide = b.map(u64::from);
    let lt = |x: u64, y: u64| -> u64 { u64::from(x < y) };
    let eqe = |j: usize, i: usize, x: u64, y: u64| -> u64 { u64::from(x == y) & u64::from(j < i) };
    let rank = |i: usize| -> u64 {
        let mut r = 0;
        let mut j = 0;
        while j < 9 {
            r += lt(wide[j], wide[i]) + eqe(j, i, wide[j], wide[i]);
            j += 1;
        }
        r
    };
    let mut out = 0u64;
    let mut i = 0;
    while i < 9 {
        out += wide[i] * u64::from(rank(i) == 4);
        i += 1;
    }
    out as u8
}

/// `k`-th largest of four `u16` lanes (rank order statistic).
///
/// `k` is masked to `k & 3` (documented domain: `0..=3`); the ascending rank
/// of the k-th largest is `3 - (k & 3)`. Ranks and selection are branchless
/// predicate arithmetic.
// from bcinr crates/bcinr-logic/src/algorithms/top_k_u32x16.rs:17-44
// (scaffold `k = aux & 3` widened to an explicit masked parameter)
#[must_use]
#[inline]
pub fn top_k4_u16(lanes: u64, k: u8) -> u16 {
    let k = u64::from(k) & 3;
    let target = 3 - k; // ascending rank of the k-th largest
    let v = [
        lanes & 0xFFFF,
        (lanes >> 16) & 0xFFFF,
        (lanes >> 32) & 0xFFFF,
        (lanes >> 48) & 0xFFFF,
    ];
    let lt = |x: u64, y: u64| -> u64 { u64::from(x < y) };
    let eqe = |j: usize, i: usize, x: u64, y: u64| -> u64 { u64::from(x == y) & u64::from(j < i) };
    let rank = |i: usize| -> u64 {
        let mut r = 0;
        let mut j = 0;
        while j < 4 {
            r += lt(v[j], v[i]) + eqe(j, i, v[j], v[i]);
            j += 1;
        }
        r
    };
    let mut out = 0u64;
    let mut i = 0;
    while i < 4 {
        out += v[i] * u64::from(rank(i) == target);
        i += 1;
    }
    out as u16
}

/// Mask-select between two `u128` operands: `a` when `sel` is `false`, `b`
/// when `sel` is `true`. Canonical all-ones/zero mask select, zero branches.
// from bcinr crates/bcinr-logic/src/algorithms/select_u128.rs:21-27
// (scaffold `sel = bit 63 of val` widened to an explicit bool parameter)
#[must_use]
#[inline]
pub const fn select_u128(a: u128, b: u128, sel: bool) -> u128 {
    let mask = (sel as u128).wrapping_neg();
    (a & !mask) | (b & mask)
}

/// Merge two sorted `u32` arrays into one sorted array via the 11-comparator
/// Batcher odd-even merge network.
///
/// Compare-and-swaps are XOR-mask selects (`(c[i] > c[j]) as u32` →
/// `0u32.wrapping_sub(diff)` mask → masked XOR), data-independent.
// from bcinr crates/bcinr-logic/src/algorithms/merge_sorted_u32x8.rs:30-70
#[must_use]
#[inline]
pub fn merge_sorted_u32x8(a: [u32; 4], b: [u32; 4]) -> [u32; 8] {
    let mut c = [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]];

    macro_rules! cas {
        ($i:expr, $j:expr) => {
            let diff = u32::from(c[$i] > c[$j]);
            let mask = 0u32.wrapping_sub(diff);
            let mn = c[$i] ^ c[$j];
            c[$i] ^= mn & mask;
            c[$j] ^= mn & mask;
        };
    }

    // Step 1: cross-boundary comparisons (merge "big step")
    cas!(0, 4);
    cas!(1, 5);
    cas!(2, 6);
    cas!(3, 7);
    // Step 2: odd-even merge (interleaved half-step)
    cas!(0, 2);
    cas!(1, 3);
    cas!(4, 6);
    cas!(5, 7);
    // Step 3: adjacent merge within left and right halves
    cas!(2, 4);
    cas!(3, 5);
    // Step 4: final adjacent cleanup
    cas!(1, 2);
    cas!(3, 4);
    cas!(5, 6);

    c
}

/// Stable argsort of eight `(key, value)` pairs by key.
///
/// Stable rank per key = count of strictly-smaller keys plus equal keys at
/// earlier indices; pairs are then scattered into their rank slots. Fixed
/// 8×8 comparison budget; `(bool) as u32` folds only.
// from bcinr crates/bcinr-logic/src/algorithms/sort_stable_key_value_u32x8.rs:35-63
#[must_use]
#[inline]
pub fn sort_stable_key_value_u32x8(pairs: [(u32, u32); 8]) -> [(u32, u32); 8] {
    let mut ranks = [0u32; 8];
    let mut i = 0;
    while i < 8 {
        let mut r = 0u32;
        let mut j = 0;
        while j < 8 {
            r += u32::from(pairs[j].0 < pairs[i].0);
            r += u32::from(pairs[j].0 == pairs[i].0 && j < i);
            j += 1;
        }
        ranks[i] = r;
        i += 1;
    }

    let mut output = [(0u32, 0u32); 8];
    let mut i = 0;
    while i < 8 {
        output[ranks[i] as usize] = pairs[i];
        i += 1;
    }
    output
}

/// Branchless non-decreasing sortedness predicate over a `u32` slice.
///
/// AND-fold of adjacent `<=` comparisons; the loop trip count is
/// length-data-dependent (structural, not value-data-dependent).
// from bcinr crates/bcinr-logic/src/algorithms/sorting_network_verify_u32.rs:30-44
#[must_use]
#[inline]
pub fn is_sorted_branchless_u32(slice: &[u32]) -> bool {
    let mut result = 1u32;
    let mut i = 0;
    while i + 1 < slice.len() {
        result &= u32::from(slice[i] <= slice[i + 1]);
        i += 1;
    }
    result != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent oracles: std sort / sort_by (stable), deliberately
    // different structure from the implementations.

    fn median3_ref(a: u32, b: u32, c: u32) -> u32 {
        let mut v = [a, b, c];
        v.sort_unstable();
        v[1]
    }

    fn median5_ref(v: [u32; 5]) -> u32 {
        let mut v = v;
        v.sort_unstable();
        v[2]
    }

    fn median9_ref(v: [u8; 9]) -> u8 {
        let mut v = v;
        v.sort_unstable();
        v[4]
    }

    fn top_k4_ref(lanes: u64, k: u8) -> u16 {
        let mut v = [
            (lanes & 0xFFFF) as u16,
            (lanes >> 16) as u16,
            (lanes >> 32) as u16,
            (lanes >> 48) as u16,
        ];
        v.sort_unstable(); // ascending
        let k = k & 3;
        v[3 - usize::from(k)]
    }

    #[test]
    fn median3_boundaries_and_oracle() {
        assert_eq!(median3_u32(0, 0, 0), 0);
        assert_eq!(median3_u32(u32::MAX, u32::MAX, u32::MAX), u32::MAX);
        assert_eq!(median3_u32(u32::MAX, 0, 1), 1);
        assert_eq!(median3_u32(0, u32::MAX, 1), 1);
        // adversarial: all permutations of a handful of adversarial triples
        for a in [0u32, 1, 7, u32::MAX - 1, u32::MAX] {
            for b in [0u32, 2, 7, u32::MAX - 1, u32::MAX] {
                for c in [0u32, 3, 7, u32::MAX - 1, u32::MAX] {
                    assert_eq!(median3_u32(a, b, c), median3_ref(a, b, c));
                    assert_eq!(median3_u32(a, c, b), median3_ref(a, c, b));
                    assert_eq!(median3_u32(b, a, c), median3_ref(b, a, c));
                }
            }
        }
    }

    #[test]
    fn median3_mutant_divergence() {
        // Non-vacuous mutants: each must diverge from the implementation on
        // at least one concrete adversarial input.
        let wrong_median = |a: u32, _b: u32, _c: u32| a; // "return first" mutant
        let min_not_median = |a: u32, b: u32, c: u32| a.min(b).min(c);
        let off_by_one = |a: u32, b: u32, c: u32| median3_ref(a, b, c).wrapping_add(1);
        assert_ne!(wrong_median(3, 1, 2), median3_u32(3, 1, 2));
        assert_ne!(min_not_median(3, 1, 2), median3_u32(3, 1, 2));
        assert_ne!(off_by_one(1, 2, 3), median3_u32(1, 2, 3));
    }

    #[test]
    fn median5_boundaries_and_oracle() {
        assert_eq!(median5_u32(0, 0, 0, 0, 0), 0);
        assert_eq!(
            median5_u32(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            u32::MAX
        );
        assert_eq!(median5_u32(5, 1, 4, 2, 3), 3);
        assert_eq!(median5_u32(u32::MAX, 0, u32::MAX, 0, 7), 7);
        // adversarial: permutations of an adversarial 5-set
        let base = [0u32, 1, u32::MAX / 2, u32::MAX - 1, u32::MAX];
        for i in 0..5 {
            for j in 0..5 {
                let mut v = base;
                v.swap(i, j);
                assert_eq!(median5_u32(v[0], v[1], v[2], v[3], v[4]), median5_ref(v));
                assert_eq!(median5_u32(v[4], v[3], v[2], v[1], v[0]), median5_ref(v));
            }
        }
    }

    #[test]
    fn median5_mutant_divergence() {
        // mutants: drop one comparator's worth of correctness by returning
        // min / max / off-by-one instead of the median (variables, not
        // literals, so the constant-comparison lint stays quiet)
        let floor = 0u32;
        let ceil = 5u32;
        assert_ne!(floor.min(3), median5_u32(5, 1, 4, 2, 3));
        assert_ne!(ceil.max(3), median5_u32(5, 1, 4, 2, 3));
        assert_ne!(
            median5_ref([5, 1, 4, 2, 3]).wrapping_add(1),
            median5_u32(5, 1, 4, 2, 3)
        );
    }

    #[test]
    fn median9_boundaries_and_oracle() {
        assert_eq!(median9_u8([0; 9]), 0);
        assert_eq!(median9_u8([255; 9]), 255);
        assert_eq!(median9_u8([9, 8, 7, 6, 5, 4, 3, 2, 1]), 5);
        assert_eq!(median9_u8([1, 1, 1, 1, 1, 1, 1, 1, 2]), 1);
        assert_eq!(median9_u8([0, 0, 0, 0, 255, 255, 255, 255, 128]), 128);
        // adversarial sweep with duplicates (exercises the eqe tiebreak)
        for seed in 0..64u32 {
            let v: [u8; 9] = core::array::from_fn(|i| ((seed * 31 + i as u32 * 7) % 5) as u8);
            assert_eq!(median9_u8(v), median9_ref(v));
        }
    }

    #[test]
    fn top_k4_boundaries_and_oracle() {
        for lanes in [0u64, u64::MAX, 0x0001_0002_0003_0004, 0xFFFF_FFFF_0000_0001] {
            for k in 0u8..=8 {
                assert_eq!(top_k4_u16(lanes, k), top_k4_ref(lanes, k));
            }
        }
        assert_eq!(top_k4_u16(0x0004_0003_0002_0001, 0), 4);
        assert_eq!(top_k4_u16(0x0004_0003_0002_0001, 3), 1);
    }

    #[test]
    fn select_u128_boundaries() {
        let a = 0x0123_4567_89AB_CDEF_0011_2233_4455_6677_u128;
        let b = 0xFEDC_BA98_7654_3210_8899_AABB_CCDD_EEFF_u128;
        assert_eq!(select_u128(a, b, false), a);
        assert_eq!(select_u128(a, b, true), b);
        assert_eq!(select_u128(0, u128::MAX, true), u128::MAX);
        assert_eq!(select_u128(u128::MAX, 0, false), u128::MAX);
    }

    #[test]
    fn merge_sorted_oracle_and_boundaries() {
        let oracle = |a: [u32; 4], b: [u32; 4]| {
            let mut v = [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]];
            v.sort_unstable();
            v
        };
        assert_eq!(
            merge_sorted_u32x8([1, 3, 5, 7], [2, 4, 6, 8]),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(merge_sorted_u32x8([0; 4], [0; 4]), [0; 8]);
        assert_eq!(
            merge_sorted_u32x8([u32::MAX; 4], [0; 4]),
            [0, 0, 0, 0, u32::MAX, u32::MAX, u32::MAX, u32::MAX]
        );
        for seed in 0..64u32 {
            let mut a = core::array::from_fn(|i| (seed * 13 + i as u32 * 3) % 17);
            let mut b = core::array::from_fn(|i| (seed * 7 + i as u32 * 5) % 17);
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(merge_sorted_u32x8(a, b), oracle(a, b));
        }
    }

    #[test]
    fn stable_kv_sort_oracle_and_stability() {
        let oracle = |mut pairs: [(u32, u32); 8]| {
            pairs.sort_by_key(|p| p.0); // sort_by_key is stable
            pairs
        };
        // stability: equal keys keep insertion order of values
        let pairs = [
            (1u32, 0u32),
            (1, 1),
            (1, 2),
            (1, 3),
            (0, 4),
            (0, 5),
            (2, 6),
            (2, 7),
        ];
        assert_eq!(sort_stable_key_value_u32x8(pairs), oracle(pairs));
        assert_eq!(sort_stable_key_value_u32x8([(0, 0); 8]), [(0, 0); 8]);
        for seed in 0..64u32 {
            let pairs: [(u32, u32); 8] =
                core::array::from_fn(|i| ((seed * 11 + i as u32) % 4, i as u32));
            assert_eq!(sort_stable_key_value_u32x8(pairs), oracle(pairs));
        }
    }

    #[test]
    fn is_sorted_predicate_oracle() {
        let oracle = |s: &[u32]| s.windows(2).all(|w| w[0] <= w[1]);
        assert!(is_sorted_branchless_u32(&[]));
        assert!(is_sorted_branchless_u32(&[1]));
        assert!(is_sorted_branchless_u32(&[1, 1, 1]));
        assert!(!is_sorted_branchless_u32(&[2, 1]));
        for seed in 0..64u32 {
            let s: [u32; 8] = core::array::from_fn(|i| (seed.wrapping_mul(2_654_435_761) >> i) % 7);
            assert_eq!(is_sorted_branchless_u32(&s), oracle(&s));
        }
    }
}

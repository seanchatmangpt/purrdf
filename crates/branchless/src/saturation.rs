//! Saturating and clamping primitives, all constant-shape.
//!
//! Ported from BranchlessCInRust (`bcinr`) with scaffold ABIs stripped:
//! `(val, aux)` register pairs are widened to explicit named parameters.
//!
//! This module supersedes purrdf's `clamped_scaling_u64`
//! (`crates/rdf-core/src/primitives.rs:44`, a `checked_mul`+match form):
//! [`mul_sat_u64`] is the u128 wide-mul + mask-select upgrade (OPT-16).
//!
//! Provenance: `// from bcinr <path>:<lines>` on every ported item.

/// Saturating multiplication for `u64`: returns `a * b`, clamped to
/// [`u64::MAX`] on overflow.
///
/// u128 wide product; an all-ones mask is generated from the non-zero upper
/// half by unsigned subtraction-from-zero (`0u64.wrapping_sub(x)`, avoiding
/// the debug-panic on negating `i64::MIN`) and selects between the low half
/// and `u64::MAX`. Zero conditional branches.
///
/// Supersedes `purrdf_core::primitives::clamped_scaling_u64` (OPT-16).
///
/// ```
/// # use purrdf_branchless::saturation::mul_sat_u64;
/// assert_eq!(mul_sat_u64(10, 20), 200);
/// assert_eq!(mul_sat_u64(u64::MAX, 2), u64::MAX);
/// assert_eq!(mul_sat_u64(0, u64::MAX), 0);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/mul_sat_u64.rs:42-63
#[inline]
#[must_use]
pub const fn mul_sat_u64(a: u64, b: u64) -> u64 {
    let product = (a as u128).wrapping_mul(b as u128);
    let lower = product as u64;
    let upper = (product >> 64) as u64;
    // 0.wrapping_sub(1) = all-ones when upper != 0; unsigned subtraction
    // from zero, never the panicking signed negation.
    let mask = 0u64.wrapping_sub((upper != 0) as u64);
    (lower & !mask) | mask
}

/// Saturating division for `u64`: `a / b`, saturating to [`u64::MAX`] when
/// `b == 0` (division by zero saturates, never traps).
///
/// **The one predictable branch:** `checked_div`'s `None` arm is a single
/// predictable not-taken compare on the hot path; kept over a mask fold for
/// honesty about the semantics.
// from bcinr crates/bcinr-logic/src/algorithms/div_sat_u64.rs:17
#[inline]
#[must_use]
pub const fn div_sat_u64(a: u64, b: u64) -> u64 {
    match a.checked_div(b) {
        Some(q) => q,
        // b == 0: saturate instead of trapping.
        None => u64::MAX,
    }
}

/// Clamps `val` into the ordered range `[lo, hi]` (signed).
///
/// Endpoints are pre-ordered with `min`/`max` (guaranteed `cmov`), so callers
/// may pass an unordered pair. Signed form of bcinr's packed-aux original.
///
/// ```
/// # use purrdf_branchless::saturation::clamp_i64;
/// assert_eq!(clamp_i64(5, 1, 10), 5);
/// assert_eq!(clamp_i64(-5, 1, 10), 1);
/// assert_eq!(clamp_i64(50, 10, 1), 10); // unordered endpoints pre-ordered
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/clamp_i64.rs:23 (widened from packed aux)
#[inline]
#[must_use]
pub const fn clamp_i64(val: i64, lo: i64, hi: i64) -> i64 {
    let (lo, hi) = ordered(lo, hi);
    max_i64(min_i64(val, hi), lo)
}

/// Clamps `val` into `[lo, hi]` (unsigned, explicit endpoints).
// from bcinr crates/bcinr-logic/src/int.rs:510 (widened from packed aux)
#[inline]
#[must_use]
pub const fn clamp_u64(val: u64, lo: u64, hi: u64) -> u64 {
    let (lo, hi) = ordered_u64(lo, hi);
    max_u64(min_u64(val, hi), lo)
}

/// Clamps `val` into `[lo, hi]` (unsigned, **endpoints pre-ordered first**).
///
/// Hardened on copy: bcinr's `clamp_slice_branchless` computed
/// `val.max(min).min(max)` without ordering — an unordered pair silently
/// returns `hi`, clamping nothing. This version pre-orders before clamping,
/// making the contract total.
///
/// ```
/// # use purrdf_branchless::saturation::clamp_slice_u64;
/// // unordered endpoints behave identically to the ordered pair
/// assert_eq!(clamp_slice_u64(5, 10, 1), 5);
/// assert_eq!(clamp_slice_u64(0, 10, 1), 1);
/// assert_eq!(clamp_slice_u64(99, 10, 1), 10);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/clamp_slice_branchless.rs:33 (hardened)
#[inline]
#[must_use]
pub const fn clamp_slice_u64(val: u64, lo: u64, hi: u64) -> u64 {
    let (lo, hi) = ordered_u64(lo, hi);
    max_u64(min_u64(val, hi), lo)
}

/// Signum via two arithmetic shifts: all-ones if negative, 1 if positive,
/// 0 if zero (as a `u64` two's-complement pattern of the input's sign).
// from bcinr crates/bcinr-logic/src/algorithms/branchless_signum_i64.rs:24
#[inline]
#[must_use]
pub const fn signum_i64_bits(v: i64) -> u64 {
    let neg = (v >> 63) as u64; // all-ones if v < 0
    let pos = (v.wrapping_neg() >> 63) as u64 & 1; // 1 if v > 0
    neg | pos
}

/// Minimum of the four `u32` lanes packed in `a`/`b` high and low halves.
// from bcinr crates/bcinr-logic/src/algorithms/min_element_branchless_u32.rs:37
#[inline]
#[must_use]
pub const fn min4_u32(a: u64, b: u64) -> u64 {
    let a0 = (a & 0xFFFF_FFFF) as u32;
    let a1 = (a >> 32) as u32;
    let b0 = (b & 0xFFFF_FFFF) as u32;
    let b1 = (b >> 32) as u32;
    let (_, mn1) = sel_min_u32(a0, a1);
    let (_, mn2) = sel_min_u32(b0, b1);
    let m3 = 0u32.wrapping_sub((mn1 < mn2) as u32);
    ((mn1 & m3) | (mn2 & !m3)) as u64
}

/// Maximum of the four `u32` lanes packed in `a`/`b` high and low halves.
// from bcinr crates/bcinr-logic/src/algorithms/max_element_branchless_u32.rs:32
#[inline]
#[must_use]
pub const fn max4_u32(a: u64, b: u64) -> u64 {
    let a0 = (a & 0xFFFF_FFFF) as u32;
    let a1 = (a >> 32) as u32;
    let b0 = (b & 0xFFFF_FFFF) as u32;
    let b1 = (b >> 32) as u32;
    let (_, mx1) = sel_max_u32(a0, a1);
    let (_, mx2) = sel_max_u32(b0, b1);
    let m3 = 0u32.wrapping_sub((mx1 > mx2) as u32);
    ((mx1 & m3) | (mx2 & !m3)) as u64
}

/// Returns `(min, max)` of two `u32`s packed `(min | max << 32)`, mask-select
/// via XOR-differencing — no branch, no temporary swap.
// from bcinr crates/bcinr-logic/src/algorithms/minmax_element_branchless_u32.rs:32
#[inline]
#[must_use]
pub const fn minmax4_u32(a: u64, b: u64) -> u64 {
    let x = a as u32;
    let y = b as u32;
    let mask = 0u64.wrapping_sub((x < y) as u64);
    let min = (y as u64) ^ (((x as u64) ^ (y as u64)) & mask);
    let max = (x as u64) ^ (((x as u64) ^ (y as u64)) & mask);
    min | (max << 32)
}

/// Overflow-safe floor average: `(a + b) / 2` without widening.
/// Identity `(a & b) + ((a ^ b) >> 1)` (Hacker's Delight 2-15).
///
/// ```
/// # use purrdf_branchless::saturation::avg_u64;
/// assert_eq!(avg_u64(u64::MAX, u64::MAX), u64::MAX);
/// assert_eq!(avg_u64(3, 5), 4);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/avg_u64.rs:25
#[inline]
#[must_use]
pub const fn avg_u64(a: u64, b: u64) -> u64 {
    (a & b).wrapping_add((a ^ b) >> 1)
}

/// Overflow-safe ceiling average: `(a + b + 1) / 2` without widening.
/// Identity `(a | b) - ((a ^ b) >> 1)` (Hacker's Delight 2-15).
///
/// ```
/// # use purrdf_branchless::saturation::avg_ceil_u64;
/// assert_eq!(avg_ceil_u64(3, 4), 4);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/avg_ceil_u64 HD 2-15 ceil identity
#[inline]
#[must_use]
pub const fn avg_ceil_u64(a: u64, b: u64) -> u64 {
    (a | b).wrapping_sub((a ^ b) >> 1)
}

/// Saturating fixed-point lerp between `u8` endpoints: `(a*(256-t) + b*t) >> 8`
/// clamped to `0xFF`. `a`/`b` are the two low bytes of `val`; `t ∈ 0..=255`.
/// Exact — no overflow by construction (all factors ≤ 255/256).
// from bcinr crates/bcinr-logic/src/algorithms/lerp_sat_u8.rs:22
#[inline]
#[must_use]
pub const fn lerp_sat_u8(val: u64, aux: u64) -> u64 {
    let a = val & 0xFF;
    let b = (val >> 8) & 0xFF;
    let t = aux & 0xFF;
    let blended = a.wrapping_mul(256 - t).wrapping_add(b.wrapping_mul(t)) >> 8;
    min_u64(blended, 0xFF)
}

/// Saturating fixed-point lerp between `u32` endpoints packed in `val`
/// (low = `a`, high = `b`), `t ∈ 0..=255` in `aux`, clamped to [`u32::MAX`].
// from bcinr crates/bcinr-logic/src/algorithms/lerp_sat_u32.rs:28
#[inline]
#[must_use]
pub const fn lerp_sat_u32(val: u64, aux: u64) -> u64 {
    let a = val & 0xFFFF_FFFF;
    let b = val >> 32;
    let t = aux & 0xFF;
    let blended = a.wrapping_mul(256 - t).wrapping_add(b.wrapping_mul(t)) >> 8;
    min_u64(blended, u32::MAX as u64)
}

/// `val.saturating_pow(exp)` with the exponent **silently truncated to 8 bits
/// (`exp & 0xFF`)** — bcinr's packed-aux artifact, disclosed here loudly.
/// Callers needing full-range exponents must mask-and-range-check at the call site.
// from bcinr crates/bcinr-logic/src/algorithms/pow_sat_u64.rs:26
#[inline]
#[must_use]
pub const fn pow_sat_u64(val: u64, exp: u64) -> u64 {
    val.saturating_pow((exp & 0xFF) as u32)
}

/// Saturating `C(n, k)` for `k ∈ {0, 1, 2}` (`k` clamped via `.min(2)`).
/// Domain disclosed: `k > 2` returns the `k == 2` value.
// from bcinr crates/bcinr-logic/src/algorithms/binom_sat_u32.rs:22
#[inline]
#[must_use]
pub const fn binom_sat_u32(n: u64, k: u64) -> u64 {
    let k = crate::select::min_u64(k, 2);
    let c0: u64 = 1;
    let c1: u64 = n;
    let c2_full = n.wrapping_mul(n.wrapping_sub(1)) >> 1;
    let c2 = min_u64(c2_full, u32::MAX as u64);
    let m0 = ((k == 0) as u64).wrapping_neg();
    let m1 = ((k == 1) as u64).wrapping_neg();
    let m2 = ((k == 2) as u64).wrapping_neg();
    (m0 & c0) | (m1 & c1) | (m2 & c2)
}

/// Saturating `n!` via a clamped 22-slot table (`0!..=20!` exact, slot 21 =
/// [`u64::MAX`]). Domain disclosed: `n ≤ 20` exact, `n > 20` saturates.
// from bcinr crates/bcinr-logic/src/algorithms/factorial_sat_u32.rs:23
#[inline]
#[must_use]
pub const fn factorial_sat_u32(n: u64) -> u64 {
    const FACT: [u64; 22] = [
        1,
        1,
        2,
        6,
        24,
        120,
        720,
        5040,
        40320,
        362_880,
        3_628_800,
        39_916_800,
        479_001_600,
        6_227_020_800,
        87_178_291_200,
        1_307_674_368_000,
        20_922_789_888_000,
        355_687_428_096_000,
        6_402_373_705_728_000,
        121_645_100_408_832_000,
        2_432_902_008_176_640_000,
        u64::MAX,
    ];
    // Clamped index keeps the path data-independent.
    FACT[(crate::select::min_u64(n, 21)) as usize]
}

/// Signed saturating add (intrinsic form; single instruction at codegen).
// from bcinr crates/bcinr-logic/src/int.rs:126
#[inline]
#[must_use]
pub const fn saturating_add_i64(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}

/// Signed saturating subtraction (intrinsic form).
// from bcinr crates/bcinr-logic/src/int.rs:148
#[inline]
#[must_use]
pub const fn saturating_sub_i64(a: i64, b: i64) -> i64 {
    a.saturating_sub(b)
}

/// Signed saturating multiplication (intrinsic form).
// from bcinr crates/bcinr-logic/src/int.rs:170
#[inline]
#[must_use]
pub const fn saturating_mul_i64(a: i64, b: i64) -> i64 {
    a.saturating_mul(b)
}

/// Canonical all-ones/zero mask type over `u32`; `select` is a two-gate AND/OR.
/// Ported from bcinr-cmca's allocator-facing mask kernel, with the
/// `mutant_7` sabotage cfg stripped (test scaffolding, not the algorithm).
// from bcinr crates/bcinr-cmca/src/fixed.rs:4-24 (mutant_7 cfg stripped)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CanonicalMask {
    /// Raw mask bits: all-ones = true, zero = false.
    pub val: u32,
}

impl CanonicalMask {
    /// The all-ones (true) mask.
    pub const TRUE: Self = Self { val: u32::MAX };
    /// The zero (false) mask.
    pub const FALSE: Self = Self { val: 0 };

    /// Mask-select between `a` (true arm) and `b` (false arm).
    #[inline]
    #[must_use]
    pub const fn select_u32(self, a: u32, b: u32) -> u32 {
        (a & self.val) | (b & !self.val)
    }

    /// Mask-select between signed `i32` arms.
    #[inline]
    #[must_use]
    pub const fn select_i32(self, a: i32, b: i32) -> i32 {
        (a & self.val as i32) | (b & !(self.val as i32))
    }
}

/// Constant-time `a < b` for `u32`, as a [`CanonicalMask`].
// from bcinr crates/bcinr-cmca/src/fixed.rs:26-33
#[inline]
#[must_use]
pub const fn const_lt_u32(a: u32, b: u32) -> CanonicalMask {
    let diff = ((a ^ ((a ^ b) | (a.wrapping_sub(b) ^ b))) >> 31) & 1;
    CanonicalMask {
        val: 0u32.wrapping_sub(diff),
    }
}

/// Constant-time `a == b` for `u32`, as a [`CanonicalMask`].
// from bcinr crates/bcinr-cmca/src/fixed.rs:35-45 (mutant_7 cfg stripped)
#[inline]
#[must_use]
pub const fn const_eq_u32(a: u32, b: u32) -> CanonicalMask {
    let x = a ^ b;
    let nonzero = (x | x.wrapping_neg()) >> 31;
    CanonicalMask {
        val: 0u32.wrapping_sub(1u32.wrapping_sub(nonzero)),
    }
}

/// Constant-time `a < b` for signed `i32` (sign-lattice form), as a mask.
// from bcinr crates/bcinr-cmca/src/fixed.rs:47-55
#[inline]
#[must_use]
pub const fn const_lt_i32(a: i32, b: i32) -> CanonicalMask {
    let diff = (a as u32).wrapping_sub(b as u32);
    let a_sign = (a as u32) >> 31;
    let b_sign = (b as u32) >> 31;
    let diff_sign = diff >> 31;
    let res = (a_sign & (b_sign ^ 1)) | ((!(a_sign ^ b_sign)) & diff_sign);
    CanonicalMask {
        val: 0u32.wrapping_sub(res),
    }
}

/// Constant-time `a == b` for signed `i32`, via the unsigned equality kernel.
// from bcinr crates/bcinr-cmca/src/fixed.rs:57-60
#[inline]
#[must_use]
pub const fn const_eq_i32(a: i32, b: i32) -> CanonicalMask {
    const_eq_u32(a as u32, b as u32)
}

// ---- internal helpers ----

/// Branchless signed max: XOR-differencing mask select (`cmov` form).
#[inline]
const fn max_i64(a: i64, b: i64) -> i64 {
    a ^ ((a ^ b) & ((a < b) as i64).wrapping_neg())
}

/// Branchless signed min.
#[inline]
const fn min_i64(a: i64, b: i64) -> i64 {
    b ^ ((a ^ b) & ((a < b) as i64).wrapping_neg())
}

/// Branchless unsigned max.
#[inline]
const fn max_u64(a: u64, b: u64) -> u64 {
    a ^ ((a ^ b) & ((a < b) as u64).wrapping_neg())
}

/// Branchless unsigned min.
#[inline]
const fn min_u64(a: u64, b: u64) -> u64 {
    b ^ ((a ^ b) & ((a < b) as u64).wrapping_neg())
}

/// Branchless ordered-pair: returns `(lo, hi)` for signed inputs.
#[inline]
const fn ordered(a: i64, b: i64) -> (i64, i64) {
    (min_i64(a, b), max_i64(a, b))
}

/// Branchless ordered-pair for unsigned inputs.
#[inline]
const fn ordered_u64(a: u64, b: u64) -> (u64, u64) {
    (min_u64(a, b), max_u64(a, b))
}

/// Mask-select the minimum of two `u32`s (returns `(mask, min)`).
#[inline]
const fn sel_min_u32(a: u32, b: u32) -> (u32, u32) {
    let m = 0u32.wrapping_sub((a < b) as u32);
    (m, (a & m) | (b & !m))
}

/// Mask-select the maximum of two `u32`s (returns `(mask, max)`).
#[inline]
const fn sel_max_u32(a: u32, b: u32) -> (u32, u32) {
    let m = 0u32.wrapping_sub((a > b) as u32);
    (m, (a & m) | (b & !m))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent u128-reference oracles (bcinr pattern, non-vacuous).

    fn mul_sat_ref(a: u64, b: u64) -> u64 {
        (u128::from(a) * u128::from(b)).min(u128::from(u64::MAX)) as u64
    }

    /// Independent u128 oracle for the fixed-point lerp (bcinr
    /// lerp_sat_u32.rs reference form): accumulate wide, clamp to u32::MAX.
    fn lerp_sat_u32_ref(val: u64, aux: u64) -> u64 {
        let a = u128::from(val & 0xFFFF_FFFF);
        let b = u128::from(val >> 32);
        let t = u128::from(aux & 0xFF);
        let blended = ((a * (256 - t) + b * t) >> 8) as u64;
        blended.min(u64::from(u32::MAX))
    }

    #[test]
    fn mul_sat_u64_boundaries_and_oracle() {
        assert_eq!(mul_sat_u64(10, 20), 200);
        assert_eq!(mul_sat_u64(u64::MAX, 2), u64::MAX);
        assert_eq!(mul_sat_u64(0, u64::MAX), 0);
        assert_eq!(mul_sat_u64(u64::MAX, u64::MAX), u64::MAX);
        for (a, b) in [(42u64, 1337u64), (1 << 63, 3), (u64::MAX, 1), (7, 0)] {
            assert_eq!(mul_sat_u64(a, b), mul_sat_ref(a, b));
        }
    }

    #[test]
    fn div_sat_u64_boundaries() {
        assert_eq!(div_sat_u64(42, 7), 6);
        assert_eq!(div_sat_u64(u64::MAX, 0), u64::MAX); // saturates, never traps
        assert_eq!(div_sat_u64(0, 0), u64::MAX);
        assert_eq!(div_sat_u64(u64::MAX, 1), u64::MAX);
        assert_eq!(div_sat_u64(0, 5), 0);
    }

    #[test]
    fn clamps_order_endpoints() {
        assert_eq!(clamp_i64(5, 1, 10), 5);
        assert_eq!(clamp_i64(-5, 1, 10), 1);
        assert_eq!(clamp_i64(50, 10, 1), 10); // unordered input
        assert_eq!(clamp_i64(i64::MIN, -3, 7), -3);
        assert_eq!(clamp_i64(i64::MAX, -3, 7), 7);
        assert_eq!(clamp_u64(5, 1, 10), 5);
        assert_eq!(clamp_u64(0, 1, 10), 1);
        assert_eq!(clamp_u64(15, 1, 10), 10);
        assert_eq!(clamp_slice_u64(5, 10, 1), 5);
        assert_eq!(clamp_slice_u64(0, 10, 1), 1);
        assert_eq!(clamp_slice_u64(99, 10, 1), 10);
    }

    #[test]
    fn signum_i64_bits_matches_reference() {
        assert_eq!(signum_i64_bits(-7), (-1i64) as u64);
        assert_eq!(signum_i64_bits(0), 0);
        assert_eq!(signum_i64_bits(9), 1);
        assert_eq!(signum_i64_bits(i64::MIN), (-1i64) as u64);
    }

    #[test]
    fn min_max_minmax_lanes_oracle() {
        for (a, b) in [
            (0x0000_0005_0000_0003u64, 0x0000_0002_0000_0009u64),
            (0, u64::MAX),
            (u64::MAX, 0),
            (0x0101_0101_0101_0101, 0x0101_0101_0101_0101),
        ] {
            let lanes = [
                (a & 0xFFFF_FFFF) as u32,
                (a >> 32) as u32,
                (b & 0xFFFF_FFFF) as u32,
                (b >> 32) as u32,
            ];
            assert_eq!(min4_u32(a, b) as u32, *lanes.iter().min().unwrap());
            assert_eq!(max4_u32(a, b) as u32, *lanes.iter().max().unwrap());
            let (x, y) = (a as u32, b as u32);
            let mm = minmax4_u32(a, b);
            assert_eq!(mm & 0xFFFF_FFFF, u64::from(x.min(y)));
            assert_eq!(mm >> 32, u64::from(x.max(y)));
        }
    }

    #[test]
    fn avg_identities_match_wide_oracle() {
        for (a, b) in [
            (3u64, 5u64),
            (u64::MAX, u64::MAX),
            (u64::MAX, 1),
            (0, u64::MAX),
        ] {
            let sum = u128::from(a) + u128::from(b);
            assert_eq!(avg_u64(a, b), (sum / 2) as u64);
            assert_eq!(avg_ceil_u64(a, b), sum.div_ceil(2) as u64);
        }
    }

    #[test]
    fn lerp_exact_forms() {
        assert_eq!(lerp_sat_u8(0x05_03, 128), 4); // (3*128 + 5*128) >> 8 = 4
        assert_eq!(lerp_sat_u8(0x00_03, 255), 0); // t = 255 → b (high byte = 0)
        assert_eq!(lerp_sat_u8(0xFF_00, 255), 254); // (255*255)>>8 — >>8 truncation loses endpoint exactness (bcinr-faithful)
        assert_eq!(lerp_sat_u32(0x0000_0005_0000_0003, 128), 4);
        // Scalar-result contract (bcinr lerp_sat_u32.rs): the lerped value is
        // returned as a plain u32-range scalar, NOT repacked into the high
        // lane. a=0, b=16, t=255 -> (0*1 + 16*255) >> 8 = 15.
        assert_eq!(lerp_sat_u32(0x0000_0010_0000_0000, 255), 0xF);
        for (val, aux) in [
            (0x0000_0005_0000_0003u64, 128u64),
            (0x0000_0010_0000_0000, 255),
            (0xFFFF_FFFF_0000_0000, 1),
            (u64::MAX, 255),
            (0x0000_0002_0000_0001, 3),
        ] {
            assert_eq!(lerp_sat_u32(val, aux), lerp_sat_u32_ref(val, aux));
            assert_eq!(
                lerp_sat_u8(val & 0xFFFF, aux),
                lerp_sat_u32_ref(val & 0xFFFF, aux) & 0xFF,
                "u8 form must agree with the u32 oracle on the low byte pair"
            );
        }
    }

    #[test]
    fn pow_binom_factorial_disclosed_domains() {
        assert_eq!(pow_sat_u64(2, 10), 1024);
        assert_eq!(pow_sat_u64(2, 256), 1); // exponent truncated to 0 (disclosed)
        assert_eq!(pow_sat_u64(u64::MAX, 3), u64::MAX);
        assert_eq!(binom_sat_u32(5, 0), 1);
        assert_eq!(binom_sat_u32(5, 1), 5);
        assert_eq!(binom_sat_u32(5, 2), 10);
        assert_eq!(binom_sat_u32(5, 99), 10); // k clamped to 2 (disclosed)
        assert_eq!(binom_sat_u32(1 << 32, 2), u64::from(u32::MAX));
        assert_eq!(factorial_sat_u32(0), 1);
        assert_eq!(factorial_sat_u32(5), 120);
        assert_eq!(factorial_sat_u32(20), 2_432_902_008_176_640_000);
        assert_eq!(factorial_sat_u32(21), u64::MAX);
        assert_eq!(factorial_sat_u32(u64::MAX), u64::MAX);
    }

    #[test]
    fn saturating_intrinsics_boundaries() {
        assert_eq!(saturating_add_i64(i64::MAX, 1), i64::MAX);
        assert_eq!(saturating_add_i64(i64::MIN, -1), i64::MIN);
        assert_eq!(saturating_sub_i64(i64::MIN, 1), i64::MIN);
        assert_eq!(saturating_mul_i64(i64::MIN, -1), i64::MAX);
        assert_eq!(saturating_mul_i64(i64::MAX, 2), i64::MAX);
    }

    #[test]
    fn canonical_mask_kernels_match_comparison_oracles() {
        for (a, b) in [(1u32, 2u32), (2, 1), (7, 7), (0, u32::MAX), (u32::MAX, 0)] {
            assert_eq!(const_lt_u32(a, b).val, if a < b { u32::MAX } else { 0 });
            assert_eq!(const_eq_u32(a, b).val, if a == b { u32::MAX } else { 0 });
            let (ai, bi) = (a as i32, b as i32);
            assert_eq!(const_lt_i32(ai, bi).val, if ai < bi { u32::MAX } else { 0 });
            assert_eq!(
                const_eq_i32(ai, bi).val,
                if ai == bi { u32::MAX } else { 0 }
            );
            assert_eq!(
                CanonicalMask::TRUE.select_u32(a, b),
                if true { a } else { b }
            );
            assert_eq!(CanonicalMask::FALSE.select_i32(ai, bi), bi);
        }
    }
}

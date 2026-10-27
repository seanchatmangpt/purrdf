//! Fixed-point (Q16.16) arithmetic for cost models.
//!
//! Q16.16 encoding: a value `v` is stored as `round(v * 65536)` in an `i32`.
//!
//! Every function is constant-shape: no data-dependent branches, no heap, no
//! ambient state. Guard branches (`x <= 0` folds, divisor substitution) are
//! cost-invariant.
//!
//! Provenance: ported from BranchlessCInRust (`bcinr-logic/src/fix.rs`,
//! `bcinr-cmca/src/fixed.rs`, `bcinr-logic/src/algorithms/
//! {fixed_point_log2,log2_u64_fixed,exp2_u64_fixed,fp_sqrt_u32_q16}.rs`),
//! bcinr commit `1e92b213`. Scaffold ABIs, `mutant_N` cfg switches, and the
//! data-dependent-FP `fast_inverse_sqrt` are excluded.
//!
//! `#[inline(always)]` on every kernel is part of the ported bcinr
//! constant-time contract (no call-site data leakage), kept verbatim from
//! upstream — hence the file-scoped allow rather than per-call-site hints.
#![allow(clippy::inline_always)]

/// Saturating addition for `u32` without branches (carry-out mask).
///
/// ```
/// # use purrdf_branchless::cost_math::add_sat;
/// assert_eq!(add_sat(10, 20), 30);
/// assert_eq!(add_sat(u32::MAX, 1), u32::MAX);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:30-51 (`add_sat`), scaffold ABI stripped.
#[must_use]
#[inline(always)]
pub const fn add_sat(a: u32, b: u32) -> u32 {
    let res = a.wrapping_add(b);
    res | 0u32.wrapping_sub((res < a) as u32)
}

/// Clamp a `u32` to `[min, max]` with bitwise masks.
///
/// ```
/// # use purrdf_branchless::cost_math::clamp_u32;
/// assert_eq!(clamp_u32(5, 0, 10), 5);
/// assert_eq!(clamp_u32(0, 3, 10), 3);
/// assert_eq!(clamp_u32(15, 0, 10), 10);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:53-79 (`clamp_u32`), scaffold ABI stripped.
// Policy note (reuse audit, dedupe family #6): `purrdf::branchless::saturation` is the
// canonical clamp-family home (ordered-pair hardening, u64 widths); this Q16-module-local
// `const` u32 form is retained for const-context use. Cross-width const-generic
// consolidation is ticketed under OPT-24's register.
#[must_use]
#[inline(always)]
pub const fn clamp_u32(val: u32, min: u32, max: u32) -> u32 {
    let lt_min = (val < min) as u32;
    let res = (min & 0u32.wrapping_sub(lt_min)) | (val & !0u32.wrapping_sub(lt_min));
    let gt_max = (res > max) as u32;
    (max & 0u32.wrapping_sub(gt_max)) | (res & !0u32.wrapping_sub(gt_max))
}

/// Round down to the nearest multiple of `step` (zero step never traps).
///
/// ```
/// # use purrdf_branchless::cost_math::bucketize_u32;
/// assert_eq!(bucketize_u32(17, 5), 15);
/// assert_eq!(bucketize_u32(9, 0), 0);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:80-102 (`bucketize_u32`), scaffold ABI stripped.
#[must_use]
#[inline(always)]
pub const fn bucketize_u32(val: u32, step: u32) -> u32 {
    val.wrapping_div(step.wrapping_add((step == 0) as u32))
        .wrapping_mul(step)
}

/// Q16.16 multiply: `(a * b) >> 16` with an `i64` intermediate.
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_mul, f32_to_q16, q16_to_f32};
/// let r = q16_to_f32(q16_mul(f32_to_q16(2.5), f32_to_q16(3.0)));
/// assert!((r - 7.5).abs() < 0.001);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:104-119 (`q16_mul`).
#[must_use]
#[inline(always)]
pub const fn q16_mul(a: i32, b: i32) -> i32 {
    (((a as i64) * (b as i64)) >> 16) as i32
}

/// Q16.16 divide: `(a << 16) / b`; zero divisor replaced by 1 (documented
/// saturation, not an error path).
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_div, f32_to_q16, q16_to_f32};
/// let r = q16_to_f32(q16_div(f32_to_q16(10.0), f32_to_q16(4.0)));
/// assert!((r - 2.5).abs() < 0.001);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:121-136 (`q16_div`).
#[must_use]
#[inline(always)]
pub const fn q16_div(a: i32, b: i32) -> i32 {
    let safe_b = b | ((b == 0) as i32);
    (((a as i64) * (1 << 16)) / safe_b as i64) as i32
}

/// Q16.16 reciprocal via two Newton-Raphson steps (~28-bit precision);
/// `i32::MAX` for `x == 0` (mask-selected).
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_recip, f32_to_q16, q16_to_f32};
/// let r = q16_to_f32(q16_recip(f32_to_q16(4.0)));
/// assert!((r - 0.25).abs() < 0.001);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:172-208 (`q16_recip`).
#[must_use]
#[inline(always)]
pub fn q16_recip(x: i32) -> i32 {
    let is_zero = i32::from(x == 0);
    let zero_mask = 0i32.wrapping_sub(is_zero);
    let safe_x = (x & !zero_mask) | (1i32 & zero_mask);
    let r0 = ((1i64 << 32) / i64::from(safe_x)) as i32;
    const TWO: i32 = 2 << 16;
    let r1 = q16_mul(r0, TWO.wrapping_sub(q16_mul(safe_x, r0)));
    let r2 = q16_mul(r1, TWO.wrapping_sub(q16_mul(safe_x, r1)));
    (r2 & !zero_mask) | (i32::MAX & zero_mask)
}

/// `f32` -> Q16.16, boundary-only conversion (not for hot paths).
// from bcinr crates/bcinr-logic/src/fix.rs:138-148 (`f32_to_q16`), boundary-only conversion.
#[doc(hidden)]
#[must_use]
#[inline(always)]
pub fn f32_to_q16(x: f32) -> i32 {
    (x * 65536.0) as i32
}

/// Q16.16 -> `f32`, boundary-only conversion (not for hot paths).
// from bcinr crates/bcinr-logic/src/fix.rs:150-170 (`q16_to_f32`), boundary-only conversion.
#[doc(hidden)]
#[must_use]
#[inline(always)]
pub fn q16_to_f32(x: i32) -> f32 {
    x as f32 / 65536.0
}

/// Q16.16 trigonometric constants.
pub mod trig_const {
    /// pi in Q16.16.
    pub const PI: i32 = 205_887;
    /// pi/2 in Q16.16.
    pub const PI_OVER_2: i32 = 102_944;
    /// 2*pi in Q16.16.
    pub const TWO_PI: i32 = 411_775;
    /// 5*pi^2 in Q16.16 (Bhaskara denominator constant, Q16.16 scale).
    pub const FIVE_PI_SQ: i32 = 3_255_680;
}

/// sin on `[0, pi]` via Bhaskara I (integer-only); error < 0.17%.
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_sin_bhaskara, q16_to_f32, trig_const};
/// assert_eq!(q16_sin_bhaskara(0), 0);
/// let s = q16_to_f32(q16_sin_bhaskara(trig_const::PI_OVER_2));
/// assert!((s - 1.0).abs() < 0.002);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:313-340 (`q16_sin_bhaskara`).
#[must_use]
#[inline(always)]
pub fn q16_sin_bhaskara(theta: i32) -> i32 {
    use trig_const::PI;
    let pi_minus_theta = PI - theta;
    let numer: i64 = 16 * i64::from(theta) * i64::from(pi_minus_theta);
    // 5*PI^2 recomputed inline in Q32.32 to match the product's scale
    // (the Q16.16 FIVE_PI_SQ constant would mismatch the Q32.32 product).
    let five_pi_sq: i64 = 5 * i64::from(PI) * i64::from(PI);
    let denom: i64 = five_pi_sq - 4 * i64::from(theta) * i64::from(pi_minus_theta);
    let safe_denom = denom + i64::from(denom == 0);
    ((numer * 65536) / safe_denom) as i32
}

/// sin for arbitrary `theta` (mod 2*pi, sign fold via masks).
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_sin_approx, q16_to_f32, trig_const};
/// assert_eq!(q16_sin_approx(0), 0);
/// let s = q16_to_f32(q16_sin_approx(trig_const::PI_OVER_2));
/// assert!((s - 1.0).abs() < 0.002);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:342-379 (`q16_sin_approx`).
#[must_use]
#[inline(always)]
pub fn q16_sin_approx(theta: i32) -> i32 {
    use trig_const::{PI, TWO_PI};
    let t = {
        let raw = theta % TWO_PI;
        let neg_mask = raw >> 31;
        raw + (TWO_PI & neg_mask)
    };
    let second_half = i32::from(t >= PI);
    let second_mask = 0i32.wrapping_sub(second_half);
    let t_mapped = t - (PI & second_mask);
    let s = q16_sin_bhaskara(t_mapped);
    (s ^ second_mask).wrapping_sub(second_mask)
}

/// cos via `cos(x) = sin(pi/2 - x)`.
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_cos_approx, q16_to_f32, trig_const};
/// let c = q16_to_f32(q16_cos_approx(0));
/// assert!((c - 1.0).abs() < 0.002);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:381-398 (`q16_cos_approx`).
#[must_use]
#[inline(always)]
pub fn q16_cos_approx(theta: i32) -> i32 {
    q16_sin_approx(trig_const::PI_OVER_2.wrapping_sub(theta))
}

/// Exact `floor(log2(x))` for `u32`; `u32::MAX` for `x == 0`.
///
/// ```
/// # use purrdf_branchless::cost_math::ilog2_u32;
/// assert_eq!(ilog2_u32(1), 0);
/// assert_eq!(ilog2_u32(8), 3);
/// assert_eq!(ilog2_u32(15), 3);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:400-424 (`ilog2_u32`).
#[must_use]
#[inline(always)]
pub const fn ilog2_u32(x: u32) -> u32 {
    31u32.wrapping_sub(x.leading_zeros())
}

/// Exact `floor(log2(val))` for `u64`; `0` for `val == 0` (mask-folded).
///
/// ```
/// # use purrdf_branchless::cost_math::ilog2_u64;
/// assert_eq!(ilog2_u64(1), 0);
/// assert_eq!(ilog2_u64(0), 0);
/// assert_eq!(ilog2_u64(u64::MAX), 63);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/log2_u64_fixed.rs:22-88 (`log2_u64_fixed`), scaffold ABI widened.
#[must_use]
#[inline(always)]
pub const fn ilog2_u64(val: u64) -> u64 {
    let nz = (val != 0) as u64;
    let mask = 0u64.wrapping_sub(nz);
    (63u64.wrapping_sub(val.leading_zeros() as u64)) & mask
}

/// Fixed-point log2 of a `u64` with caller-chosen fraction width `fb`
/// (unsigned Qx.fb estimate; `val == 0` maps to 0). Fully mask-folded.
///
/// ```
/// # use purrdf_branchless::cost_math::log2_qfb;
/// assert_eq!(log2_qfb(0, 16), 0);
/// assert_eq!(log2_qfb(1 << 20, 0), 20);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/fixed_point_log2.rs:25-96 (`fixed_point_log2`), scaffold ABI widened.
#[must_use]
#[inline(always)]
pub const fn log2_qfb(val: u64, fb: u32) -> u64 {
    let lz = val.leading_zeros();
    let nz = ((val | val.wrapping_neg()) >> 63) & 1;
    let ip = 63u64.wrapping_sub(lz as u64) & nz.wrapping_neg();
    let mantissa = val.wrapping_shl(lz.wrapping_add(1));
    // `checked_shr(64 - fb)` with mask fold: `fb == 0` yields frac 0.
    let shift = 64u32.wrapping_sub(fb);
    let shiftable = (shift < 64) as u64;
    let frac = mantissa.wrapping_shr(shift & 63) & shiftable.wrapping_neg();
    ip.wrapping_shl(fb).wrapping_add(frac)
}

/// Q16.16 log2 with linear fractional interpolation; error < 1%.
/// One internal branch folds `x <= 0` to `i32::MIN`.
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_log2, f32_to_q16, q16_to_f32};
/// let one = f32_to_q16(1.0);
/// assert!(q16_to_f32(q16_log2(one)).abs() < 0.01);
/// ```
// from bcinr crates/bcinr-logic/src/fix.rs:426-463 (`q16_log2`).
#[must_use]
#[inline(always)]
pub fn q16_log2(x: i32) -> i32 {
    if x <= 0 {
        return i32::MIN;
    }
    let xu = x as u32;
    let high_bit = xu.ilog2();
    let int_part = (high_bit as i32) - 16;
    let mantissa: u32 = if high_bit >= 15 {
        xu >> (high_bit - 15)
    } else {
        xu << (15 - high_bit)
    };
    let frac_bits = mantissa & 0x7FFF;
    let frac_q16 = frac_bits * 2;
    (int_part * 65536) + frac_q16 as i32
}

/// Q16.16 log2 with error-corrected quadratic interpolation (from
/// bcinr-cmca `NonNegativeFixed::log2`, err plumbing stripped). Strictly
/// more accurate than [`q16_log2`] on `[1, 2)`. `x <= 0` folds to
/// `i32::MIN` via mask.
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_log2_corrected, f32_to_q16, q16_to_f32};
/// let two = f32_to_q16(2.0);
/// assert!((q16_to_f32(q16_log2_corrected(two)) - 1.0).abs() < 0.005);
/// ```
// from bcinr crates/bcinr-cmca/src/fixed.rs:227 (`NonNegativeFixed::log2`), err plumbing stripped.
#[must_use]
#[inline(always)]
pub fn q16_log2_corrected(x: i32) -> i32 {
    let xu64 = x as u64;
    let lz = xu64.leading_zeros();
    let nz = ((xu64 | xu64.wrapping_neg()) >> 63) & 1;
    let ip = 63u64.wrapping_sub(u64::from(lz)) & nz.wrapping_neg();
    let mantissa = xu64.wrapping_shl(lz.wrapping_add(1));
    let f = (mantissa >> (64 - 16)) as u32;
    let diff = 65536u32.wrapping_sub(f);
    let correction = (f.wrapping_mul(diff)) >> 16;
    let corrected_frac = f.wrapping_add((correction.wrapping_mul(29_013)) >> 16);
    let res = (ip << 16).wrapping_add(u64::from(corrected_frac));
    let is_zero = u32::from(x <= 0).wrapping_neg();
    let computed = (res as u32).wrapping_sub(16 << 16) as i32;
    let is_zero_i = is_zero as i32;
    (computed & !is_zero_i) | (i32::MIN & is_zero_i)
}

/// Q16 `2^n` over the integer part of the Q16 exponent only; saturates to
/// `u64::MAX` for `int_exp >= 48` via mask (shift clamped, never traps).
///
/// ```
/// # use purrdf_branchless::cost_math::exp2_int_q16;
/// assert_eq!(exp2_int_q16(0), 65536);
/// assert_eq!(exp2_int_q16(65536), 131072);
/// assert_eq!(exp2_int_q16(48 << 16), u64::MAX);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/exp2_u64_fixed.rs:24-88 (`exp2_u64_fixed`), scaffold ABI widened.
#[must_use]
#[inline(always)]
pub const fn exp2_int_q16(val: u64) -> u64 {
    let int_exp = (val >> 16) as u32;
    let saturated = (int_exp >= 48) as u64;
    let sat_mask = saturated.wrapping_neg();
    let safe_exp = int_exp & 63;
    let result = 65536u64.wrapping_shl(safe_exp);
    (result & !sat_mask) | sat_mask
}

/// Q16.16 `2^x` including the fractional part (Taylor/Horner in `f` in
/// `[0,1)`; empirically adjusted coefficients `630, 3637, 15763, 45506`).
/// Saturates to `u32::MAX` for `x >= 16.0`, underflows to `0` for
/// `x < -17.0`, both via masks.
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_exp2, f32_to_q16};
/// assert_eq!(q16_exp2(0), 65536);
/// let one = q16_exp2(f32_to_q16(1.0));
/// assert!((one as f64 / 65536.0 - 2.0).abs() < 0.02);
/// ```
// from bcinr crates/bcinr-cmca/src/fixed.rs:357 (`SignedFixed::exp2`), mutant cfg switches stripped.
#[must_use]
#[inline(always)]
pub fn q16_exp2(x: i32) -> u32 {
    let ip = x >> 16;
    let fp = x.wrapping_sub(ip.wrapping_shl(16));
    let y = fp as u32;
    let res1 = (y.wrapping_mul(630)) >> 16;
    let res2 = (y.wrapping_mul(3637u32.wrapping_add(res1))) >> 16;
    let res3 = (y.wrapping_mul(15763u32.wrapping_add(res2))) >> 16;
    let res4 = (y.wrapping_mul(45506u32.wrapping_add(res3))) >> 16;
    let frac_part = 65536u32.wrapping_add(res4);

    let is_overflow = (((ip.wrapping_sub(16)) >> 31) ^ 1) & 1;
    let is_underflow = ((((-17i32).wrapping_sub(ip)) >> 31) ^ 1) & 1;

    let shl = (ip & 31) as u32;
    let shr = ((ip.wrapping_neg()) & 31) as u32;
    let val_shl = frac_part.wrapping_shl(shl);
    let val_shr = frac_part.wrapping_shr(shr);
    let ip_neg = ((ip >> 31) & 1) as u32;
    // bcinr: ip_neg.select_u32(val_shr, val_shl) — val_shr where ip_neg set.
    // The bit must be expanded to an all-ones/zero mask BEFORE the select.
    let ip_neg_mask = ip_neg.wrapping_neg();
    let val_shifted = (val_shr & ip_neg_mask) | (val_shl & !ip_neg_mask);

    let ovf_mask = (is_overflow as u32).wrapping_neg();
    let unf_mask = (is_underflow as u32).wrapping_neg();
    ovf_mask | (val_shifted & !(ovf_mask | unf_mask))
}

/// Q16.16 `e^x` via `exp(x) = 2^(x * log2 e)` (coefficient 94548, biased
/// +3 ULP to minimize composed error; in-domain for `|x| < ~22717.0`).
///
/// ```
/// # use purrdf_branchless::cost_math::{q16_exp, f32_to_q16};
/// let e = q16_exp(f32_to_q16(1.0));
/// assert!((e as f64 / 65536.0 - 2.718).abs() < 0.01);
/// ```
// from bcinr crates/bcinr-cmca/src/fixed.rs:405 (`SignedFixed::exp`), mutant cfg switches stripped.
#[must_use]
#[inline(always)]
pub fn q16_exp(x: i32) -> u32 {
    let z = ((i64::from(x).wrapping_mul(94_548)) >> 16) as i32;
    q16_exp2(z)
}

/// Constant-time digit-by-digit integer sqrt of `val << 16`.
///
/// For a Q16.16 input `val`, the result is `sqrt(val / 65536)` in Q16.16.
/// 41 fixed reduction steps over `u128`, no early exits. The crate's
/// canonical sqrt (fully data-independent shape; preferred over Newton).
///
/// ```
/// # use purrdf_branchless::cost_math::q16_sqrt_ct;
/// assert_eq!(q16_sqrt_ct(0), 0);
/// let four = q16_sqrt_ct(4u64 << 16);
/// assert!((four as f64 / 65536.0 - 2.0).abs() < 0.01);
/// ```
// from bcinr crates/bcinr-logic/src/algorithms/fp_sqrt_u32_q16.rs:22-90 (`fp_sqrt_u32_q16`), scaffold ABI widened.
#[must_use]
#[inline(always)]
pub fn q16_sqrt_ct(val: u64) -> u64 {
    let mut x = u128::from(val) << 16;
    let mut res = 0u128;
    let mut bit = 1u128 << 80;
    let mut k = 0u32;
    while k < 41 {
        let candidate = res + bit;
        let m = u128::from(x >= candidate).wrapping_neg();
        x -= candidate & m;
        res = (res >> 1) + (bit & m);
        bit >>= 2;
        k += 1;
    }
    res as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_f32(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn test_add_sat_table() {
        for &(a, b, expected) in &[
            (0u32, 0u32, 0u32),
            (42, 0, 42),
            (0, 42, 42),
            (10, 20, 30),
            (u32::MAX, 1, u32::MAX),
            (u32::MAX, u32::MAX, u32::MAX),
            (u32::MAX - 1, 1, u32::MAX),
            (u32::MAX - 5, 5, u32::MAX),
        ] {
            assert_eq!(add_sat(a, b), expected, "add_sat({a}, {b})");
        }
    }

    #[test]
    fn test_clamp_table() {
        for &(val, lo, hi, expected) in &[
            (0u32, 0u32, 0u32, 0u32),
            (5, 0, 10, 5),
            (0, 3, 10, 3),
            (15, 0, 10, 10),
            (3, 3, 10, 3),
            (10, 3, 10, 10),
            (u32::MAX, 0, 100, 100),
            (u32::MAX, 0, u32::MAX, u32::MAX),
        ] {
            assert_eq!(clamp_u32(val, lo, hi), expected, "clamp({val},{lo},{hi})");
        }
    }

    #[test]
    fn test_bucketize_table() {
        for &(val, step, expected) in &[
            (0u32, 8u32, 0u32),
            (16, 8, 16),
            (8, 8, 8),
            (17, 5, 15),
            (9, 5, 5),
            (42, 1, 42),
            (0, 1, 0),
            (9, 0, 0),
            (0, 0, 0),
        ] {
            assert_eq!(
                bucketize_u32(val, step),
                expected,
                "bucketize({val},{step})"
            );
        }
        assert_eq!(bucketize_u32(u32::MAX, 100) % 100, 0);
    }

    #[test]
    fn test_q16_mul_div_recip() {
        let a = f32_to_q16(2.5);
        let b = f32_to_q16(3.0);
        assert!((q16_mul(a, b) - f32_to_q16(7.5)).abs() <= 1);
        assert_eq!(q16_mul(f32_to_q16(5.0), f32_to_q16(1.0)), f32_to_q16(5.0));
        let d = q16_div(f32_to_q16(10.0), f32_to_q16(4.0));
        assert!((d - f32_to_q16(2.5)).abs() <= 2);
        let _ = q16_div(65536, 0);
        let r = q16_recip(f32_to_q16(4.0));
        assert!(approx_f32(q16_to_f32(r), 0.25, 0.01));
        let r1 = q16_recip(f32_to_q16(1.0));
        assert!(approx_f32(q16_to_f32(r1), 1.0, 0.01));
        let _ = q16_recip(0);
    }

    #[test]
    fn test_f32_q16_roundtrip() {
        for &v in &[
            0.0f32,
            1.0,
            -1.0,
            0.5,
            -0.5,
            core::f32::consts::PI,
            100.0,
            -100.0,
        ] {
            let decoded = q16_to_f32(f32_to_q16(v));
            assert!(approx_f32(decoded, v, 0.0001), "roundtrip {v} -> {decoded}");
        }
    }

    #[test]
    fn test_trig_precision() {
        assert_eq!(q16_sin_approx(0), 0);
        let s = q16_to_f32(q16_sin_approx(trig_const::PI_OVER_2));
        assert!(approx_f32(s, 1.0, 0.002), "sin(pi/2) = {s}");
        let s_pi = q16_to_f32(q16_sin_approx(trig_const::PI));
        assert!(s_pi.abs() < 0.002, "sin(pi) = {s_pi}");
        let c = q16_to_f32(q16_cos_approx(0));
        assert!(approx_f32(c, 1.0, 0.002), "cos(0) = {c}");
        let c2 = q16_to_f32(q16_cos_approx(trig_const::PI_OVER_2));
        assert!(c2.abs() < 0.01, "cos(pi/2) = {c2}");
        // sweep [0, 2pi): documented Bhaskara bound (< 0.17%) + Q16 slack
        let mut theta = 0i32;
        while theta < trig_const::TWO_PI {
            let got = q16_to_f32(q16_sin_approx(theta));
            let want = (theta as f32 / 65536.0).sin();
            assert!(
                (got - want).abs() < 0.003,
                "sin({}) = {got}, want {want}",
                theta as f32 / 65536.0
            );
            theta += 4096;
        }
    }

    #[test]
    fn test_ilog2() {
        for &(x, expected) in &[
            (1u32, 0u32),
            (2, 1),
            (3, 1),
            (4, 2),
            (7, 2),
            (8, 3),
            (255, 7),
            (256, 8),
        ] {
            assert_eq!(ilog2_u32(x), expected);
        }
        assert_eq!(ilog2_u64(0), 0);
        assert_eq!(ilog2_u64(u64::MAX), 63);
        assert_eq!(log2_qfb(0, 16), 0);
        assert_eq!(log2_qfb(1 << 20, 0), 20);
    }

    #[test]
    fn test_q16_log2_accuracy() {
        let one = f32_to_q16(1.0);
        assert!(q16_to_f32(q16_log2(one)).abs() < 0.01);
        let four = f32_to_q16(4.0);
        assert!((q16_to_f32(q16_log2(four)) - 2.0).abs() < 0.05);
        let two = f32_to_q16(2.0);
        assert!((q16_to_f32(q16_log2(two)) - 1.0).abs() < 0.05);
        // corrected form must not be worse than linear on [1, 2)
        let x = f32_to_q16(1.5);
        let true_val = 1.5f64.log2();
        let err_lin = (f64::from(q16_log2(x)) / 65536.0 - true_val).abs();
        let err_corr = (f64::from(q16_log2_corrected(x)) / 65536.0 - true_val).abs();
        assert!(
            err_corr <= err_lin,
            "corrected log2 error {err_corr} must not exceed linear {err_lin}"
        );
    }

    /// Independent f64 oracle sweep for [`q16_log2_corrected`], including the
    /// endpoints and the `+16` bias line (`res - 16 << 16`) that converts the
    /// u64 integer-log2 result to the Q16.16 frame. The bias line is verified
    /// exact at every power-of-two Q16.16 bit pattern (`2^j` for
    /// `j in [-16, 14]`) and non-tautologically by the domain sweep: an
    /// oracle framed as raw-integer log2 (missing the /65536) is off by
    /// exactly 16 — precisely the bias — so the sweep pins the frame.
    ///
    /// Measured envelope (release probe, exhaustive `x in [1, 2^24)` + stride
    /// 9973 over `[2^24, i32::MAX]`): max abs error 0.028322 at x=53337.
    /// On `[1, 2)` the corrected form dominates the linear [`q16_log2`]
    /// everywhere (max 0.028322 vs 0.086087), confirming the docstring.
    #[test]
    fn test_q16_log2_corrected_endpoint_sweep() {
        let oracle = |x: i32| (f64::from(x) / 65536.0).log2();
        let checked = |x: i32| {
            let got = f64::from(q16_log2_corrected(x)) / 65536.0;
            let want = oracle(x);
            assert!(
                (got - want).abs() < 0.03,
                "q16_log2_corrected({x}) = {got}, want {want}"
            );
        };

        // Endpoints: every named input-class boundary from the contract,
        // plus i32::MAX. Values < 2^20 are also re-swept exhaustively below.
        for x in [
            1,
            2,
            15,
            16,
            17,
            255,
            256,
            4095,
            4096,
            65_535,
            65_536,
            65_537,
            i32::MAX,
        ] {
            checked(x);
        }
        // Every power-of-two boundary ±1 (raw bit patterns 2^k, k in 0..=30).
        // The k=0, d=-1 neighbor is x=0: outside the oracle's domain, so it
        // is excluded here and pinned by the documented fold asserts below
        // (x <= 0 must return i32::MIN, not an approximate log2).
        for k in 0..=30u32 {
            for d in [-1i32, 0, 1] {
                let x = (1i32 << k).wrapping_add(d);
                if x > 0 {
                    checked(x);
                }
            }
        }
        checked(i32::MAX);
        // x <= 0 folds to i32::MIN (mask-selected), never panics.
        assert_eq!(q16_log2_corrected(0), i32::MIN);
        assert_eq!(q16_log2_corrected(-1), i32::MIN);
        assert_eq!(q16_log2_corrected(i32::MIN), i32::MIN);

        // The +16 bias line: exact at every power-of-two bit pattern.
        let mut j: i32 = -16;
        while j <= 14 {
            let x = 1i32.wrapping_shl((16 + j) as u32);
            assert_eq!(q16_log2_corrected(x), j * 65536, "2^{j}");
            checked(x);
            j += 1;
        }

        // Domain sweep: exhaustive below 2^20, stride above.
        for x in 1..(1 << 20) {
            checked(x);
        }
        let mut x: i64 = 1 << 20;
        while x <= i64::from(i32::MAX) {
            checked(x as i32);
            x += 9973;
        }

        // Docstring dominance claim on [1, 2): corrected <= linear, all x.
        let lin = |x: i32| -> f64 {
            let xu = x as u32;
            let hb = xu.ilog2();
            let mant = if hb >= 15 {
                xu >> (hb - 15)
            } else {
                xu << (15 - hb)
            };
            (f64::from(hb) - 16.0) + (f64::from((mant & 0x7FFF) * 2) / 65536.0)
        };
        for x in 65536..131_072 {
            let want = oracle(x);
            let ec = (f64::from(q16_log2_corrected(x)) / 65536.0 - want).abs();
            let el = (lin(x) - want).abs();
            assert!(
                ec <= el,
                "corrected {ec} exceeds linear {el} at x={x} (1 + {})",
                f64::from(x - 65_536) / 65_536.0
            );
        }
    }

    #[test]
    fn test_exp2_exp() {
        assert_eq!(exp2_int_q16(0), 65536);
        assert_eq!(exp2_int_q16(65536), 131_072);
        assert_eq!(exp2_int_q16(131_072), 262_144);
        assert_eq!(exp2_int_q16(48 << 16), u64::MAX);
        assert_eq!(q16_exp2(0), 65536);
        let one = q16_exp2(f32_to_q16(1.0));
        assert!((f64::from(one) / 65536.0 - 2.0).abs() < 0.02, "2^1 = {one}");
        let half = q16_exp2(f32_to_q16(0.5));
        assert!(
            (f64::from(half) / 65536.0 - core::f64::consts::SQRT_2).abs() < 0.02,
            "2^0.5 = {half}"
        );
        let e = q16_exp(f32_to_q16(1.0));
        assert!(
            (f64::from(e) / 65536.0 - core::f64::consts::E).abs() < 0.02,
            "e = {e}"
        );
        assert_eq!(q16_exp2(16 << 16), u32::MAX);
        assert_eq!(q16_exp2(-17 << 16), 0);
    }

    #[test]
    fn test_q16_sqrt_ct() {
        assert_eq!(q16_sqrt_ct(0), 0);
        let four = q16_sqrt_ct(4u64 << 16);
        assert!((four as f64 / 65536.0 - 2.0).abs() < 0.01);
        let one = q16_sqrt_ct(1u64 << 16);
        assert!((one as f64 / 65536.0 - 1.0).abs() < 0.01);
        let mut v = 0u64;
        while v < 1 << 20 {
            // Q16-in/Q16-out: shift the integer sample into Q16 first
            // (out = sqrt(in·2^16); unshifted input is the bcinr Q8.8 form).
            let s = q16_sqrt_ct(v << 16);
            let want = ((v as f64).sqrt() * 65536.0) as i64;
            assert!(
                (s as i64 - want).abs() <= 1,
                "sqrt_ct({v}) = {s}, want ~{want}"
            );
            v += 99991;
        }
    }
}

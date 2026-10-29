//! Scalar fallbacks for the vector-lane primitives purrdf lacks elsewhere.
//!
//! The SIMD archetype kernels themselves stay in `purrdf-lex::scan`
//! (`ByteClass::find_first`, `find_byte`, `find_needles` own the arch-kernel
//! role for lexical scanning). This module provides only what purrdf has no
//! home for — the software PDEP/PEXT emulations, the pshufb-semantics shuffle,
//! the 16-lane horizontal sum — and the constant-time select/clamp kernels
//! from bcinr's `ct.rs`, all in a safe scalar form so the crate keeps its
//! `#![forbid(unsafe_code)]` guarantee. No runtime feature detection and no
//! `core::arch` intrinsics are used: every function here is the portable
//! path by construction.
//!
//! Provenance: ported from `bcinr` `crates/bcinr-logic/src/simd_dispatch.rs`
//! (:466-492 scalar forms, :932-968 PDEP/PEXT) and `ct.rs` (whole module).
//! bcinr branch `bench/rdtsc-tick-tables`, commit `1e92b213`. Scaffold ABI
//! (`#[no_mangle]`, `(val, aux)` pairs, `_phd_gate` identity anchors) stripped;
//! vacuous mutant scaffolding not ported.
//!
//! Dedupe note: `circuit_breaker`'s mask predicates and `saturation`'s
//! `CanonicalMask` are policy-distinct thin wrappers of the same bool→mask
//! idea this module's ct kernels express; they are kept separate by design
//! (annotation only — see the crate dedupe register).

/// Software PDEP: scatter the bits of `val` into the positions set in `mask`.
///
/// Bit `i` of `val` lands on the `i`-th set bit of `mask` (low to high);
/// mask positions left over are zero. Constant-time: exactly 64 iterations,
/// no data-dependent branching.
///
/// Equivalent to x86 BMI2 `pdep` (which this crate intentionally does not
/// call — no unsafe, no runtime feature detection).
///
/// ```
/// use purrdf_branchless::simd_fallback::pdep_u64;
/// assert_eq!(pdep_u64(0b101, 0b0101_0100), 0b0100_0100);
/// assert_eq!(pdep_u64(u64::MAX, 0), 0);
/// assert_eq!(pdep_u64(u64::MAX, u64::MAX), u64::MAX);
/// ```
#[inline]
#[must_use]
pub fn pdep_u64(val: u64, mask: u64) -> u64 {
    let mut res = 0u64;
    let mut m = mask;
    let mut v = val;
    let mut i = 0;
    while i < 64 {
        let lowest_mask_bit = m & m.wrapping_neg();
        let cond = (lowest_mask_bit != 0) as u64;
        let v_bit = v & 1;
        res |= (v_bit * lowest_mask_bit) * cond;
        v >>= cond;
        m ^= lowest_mask_bit;
        i += 1;
    }
    res
}

/// Software PEXT: gather the bits of `val` selected by `mask` into a
/// right-packed result. Complement of [`pdep_u64`].
///
/// Constant-time: exactly 64 iterations, no data-dependent branching.
///
/// ```
/// use purrdf_branchless::simd_fallback::{pdep_u64, pext_u64};
/// assert_eq!(pext_u64(0b1101_0110, 0b0101_0100), 0b111);
/// // round trip
/// let v = 0x0123_4567_89AB_CDEFu64;
/// let m = 0x0F0F_0F0F_0F0F_0F0Fu64;
/// assert_eq!(pdep_u64(pext_u64(v, m), m), v & m);
/// ```
#[inline]
#[must_use]
pub fn pext_u64(val: u64, mask: u64) -> u64 {
    let mut res = 0u64;
    let mut m = mask;
    let mut shift = 0u64;
    let mut i = 0;
    while i < 64 {
        let lowest_mask_bit = m & m.wrapping_neg();
        let cond = (lowest_mask_bit != 0) as u64;
        let bit = (val & lowest_mask_bit) != 0;
        res |= (bit as u64) << shift;
        shift += cond;
        m ^= lowest_mask_bit;
        i += 1;
    }
    res
}

/// Branchless byte shuffle with `pshufb` semantics: `out[i] = a[mask[i] & 0x0F]`,
/// or `0` when `mask[i] & 0x80 != 0` (the pshufb zeroing rule).
///
/// Scalar 16-lane form; the arch kernels live in `purrdf-lex::scan`.
///
/// ```
/// use purrdf_branchless::simd_fallback::shuffle_u8x16;
/// let a: [u8; 16] = core::array::from_fn(|i| i as u8);
/// let mut mask = [0u8; 16];
/// mask.copy_from_slice(&[15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
/// let r = shuffle_u8x16(a, mask);
/// assert_eq!(r[0], 15);
/// assert_eq!(r[15], 0);
/// let mut zero_mask = [0u8; 16];
/// zero_mask[0] = 0x80;
/// assert_eq!(shuffle_u8x16(a, zero_mask)[0], 0);
/// ```
#[inline]
#[must_use]
pub fn shuffle_u8x16(a: [u8; 16], mask: [u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut i = 0usize;
    while i < 16 {
        let idx = (mask[i] & 0x0F) as usize;
        let zero_lane = mask[i] >> 7;
        let zero_mask = zero_lane.wrapping_neg(); // 0xFF → zero, 0x00 → keep
        out[i] = a[idx] & !zero_mask;
        i += 1;
    }
    out
}

/// Horizontal sum of all 16 u8 lanes (max 16 × 255 = 4080, fits `u32`).
///
/// ```
/// use purrdf_branchless::simd_fallback::horizontal_sum_u8x16;
/// assert_eq!(horizontal_sum_u8x16([0u8; 16]), 0);
/// assert_eq!(horizontal_sum_u8x16([1u8; 16]), 16);
/// assert_eq!(horizontal_sum_u8x16([255u8; 16]), 4080);
/// ```
#[inline]
#[must_use]
pub fn horizontal_sum_u8x16(a: [u8; 16]) -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < 16 {
        sum += a[i] as u32;
        i += 1;
    }
    sum
}

/// MSB-select: per lane, pick `b` when `mask[i]` has its high bit set,
/// else `a` (the `_mm_blendv_epi8` semantic).
///
/// ```
/// use purrdf_branchless::simd_fallback::blend_u8x16;
/// let a = [1u8, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// let b = [10u8, 20, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// let mut mask = [0u8; 16];
/// mask[1] = 0x80; // MSB set → take b
/// mask[2] = 0x7F; // MSB clear → take a
/// let r = blend_u8x16(mask, a, b);
/// assert_eq!(r[1], 20);
/// assert_eq!(r[2], 3);
/// ```
#[inline]
#[must_use]
pub fn blend_u8x16(mask: [u8; 16], a: [u8; 16], b: [u8; 16]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut i = 0usize;
    while i < 16 {
        let m = (mask[i] >> 7).wrapping_neg();
        out[i] = (b[i] & m) | (a[i] & !m);
        i += 1;
    }
    out
}

/// Constant-time select: `a` when `condition` (as bit 0) is 1, else `b`.
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_select_u64;
/// assert_eq!(ct_select_u64(1, 0xAAAA_BBBB, 0xCCCC_DDDD), 0xAAAA_BBBB);
/// assert_eq!(ct_select_u64(0, 0xAAAA_BBBB, 0xCCCC_DDDD), 0xCCCC_DDDD);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_select_u64(condition: u64, a: u64, b: u64) -> u64 {
    let mask = 0u64.wrapping_sub(condition & 1);
    (a & mask) | (b & !mask)
}

/// Constant-time signed select (bit patterns reinterpreted, then cast back).
///
/// Family note: the select-kernel canon lives in [`crate::select`]
/// (`select_u128` at this writing); u64/i64 widths are kept here until a
/// const-generic macro derives every width from one definition.
/// ```
/// use purrdf_branchless::simd_fallback::ct_select_i64;
/// assert_eq!(ct_select_i64(1, -1, 42), -1);
/// assert_eq!(ct_select_i64(0, -1, 42), 42);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_select_i64(condition: u64, a: i64, b: i64) -> i64 {
    let mask = 0u64.wrapping_sub(condition & 1);
    ((a as u64 & mask) | (b as u64 & !mask)) as i64
}

/// Constant-time equality: 1 when `a == b`, 0 otherwise.
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_eq_u64;
/// assert_eq!(ct_eq_u64(100, 100), 1);
/// assert_eq!(ct_eq_u64(100, 101), 0);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_eq_u64(a: u64, b: u64) -> u64 {
    let x = a ^ b;
    let nonzero = (x | x.wrapping_neg()) >> 63;
    1u64.wrapping_sub(nonzero)
}

/// Constant-time unsigned `a < b`: 1/0.
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_lt_u32;
/// assert_eq!(ct_lt_u32(3, 5), 1);
/// assert_eq!(ct_lt_u32(5, 3), 0);
/// assert_eq!(ct_lt_u32(5, 5), 0);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_lt_u32(a: u32, b: u32) -> u32 {
    ((a ^ ((a ^ b) | (a.wrapping_sub(b) ^ b))) >> 31) & 1
}

/// Constant-time signed `a < b` for i64: 1/0, all sign combinations.
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_lt_i64;
/// assert_eq!(ct_lt_i64(-1, 0), 1);
/// assert_eq!(ct_lt_i64(0, -1), 0);
/// assert_eq!(ct_lt_i64(i64::MIN, i64::MAX), 1);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_lt_i64(a: i64, b: i64) -> u64 {
    let ua = a as u64;
    let ub = b as u64;
    let sign_a = ua >> 63;
    let sign_b = ub >> 63;
    let sign_diff = ua.wrapping_sub(ub) >> 63;
    let signs_differ = sign_a ^ sign_b;
    ((signs_differ & sign_a) | ((!signs_differ) & sign_diff)) & 1
}

/// Constant-time conditional XOR swap: swaps when `condition` (bit 0) is 1.
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_conditional_swap_u64;
/// let (mut x, mut y) = (10u64, 20u64);
/// ct_conditional_swap_u64(1, &mut x, &mut y);
/// assert_eq!((x, y), (20, 10));
/// ct_conditional_swap_u64(0, &mut x, &mut y);
/// assert_eq!((x, y), (20, 10));
/// ```
#[inline(always)]
pub fn ct_conditional_swap_u64(condition: u64, a: &mut u64, b: &mut u64) {
    let mask = 0u64.wrapping_sub(condition & 1);
    let diff = (*a ^ *b) & mask;
    *a ^= diff;
    *b ^= diff;
}

/// Constant-time absolute value (wrapping at `i64::MIN`, like `wrapping_abs`).
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_abs_i64;
/// assert_eq!(ct_abs_i64(-5), 5);
/// assert_eq!(ct_abs_i64(5), 5);
/// assert_eq!(ct_abs_i64(i64::MIN), i64::MIN.wrapping_abs());
/// ```
#[inline(always)]
#[must_use]
pub fn ct_abs_i64(x: i64) -> i64 {
    let mask = x >> 63;
    (x ^ mask).wrapping_sub(mask)
}

/// Constant-time minimum of two `u32`.
///
/// Delegates to the canonical XOR-differencing mask form in
/// [`crate::select::min_u32`] — the wrap-add formulation this module
/// previously carried was a duplicate kernel (dedupe family #11).
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_min_u32;
/// assert_eq!(ct_min_u32(3, 5), 3);
/// assert_eq!(ct_min_u32(5, 3), 3);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_min_u32(a: u32, b: u32) -> u32 {
    crate::select::min_u32(a, b)
}

/// Constant-time maximum of two `u32`.
///
/// Delegates to [`crate::select::max_u32`] (dedupe family #11).
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_max_u32;
/// assert_eq!(ct_max_u32(3, 5), 5);
/// assert_eq!(ct_max_u32(5, 3), 5);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_max_u32(a: u32, b: u32) -> u32 {
    crate::select::max_u32(a, b)
}

/// Constant-time clamp to `[lo, hi]`.
///
/// ```
/// use purrdf_branchless::simd_fallback::ct_clamp_u32;
/// assert_eq!(ct_clamp_u32(10, 20, 30), 20);
/// assert_eq!(ct_clamp_u32(25, 20, 30), 25);
/// assert_eq!(ct_clamp_u32(40, 20, 30), 30);
/// ```
#[inline(always)]
#[must_use]
pub fn ct_clamp_u32(val: u32, lo: u32, hi: u32) -> u32 {
    ct_min_u32(ct_max_u32(val, lo), hi)
}

// Non-finite lane masks (OPT-44)
//
// from bcinr crates/bcinr-logic/src/algorithms/is_finite_fp32_branchless.rs
// (:15-27): a value is finite iff its exponent field is not all-ones, decided
// by a masked exponent compare. DIVERGENCE from bcinr: the scaffold shape
// (`fn(u64, u64) -> u64` packing two `(val, aux)` predicates) is replaced by
// the ticket's one-compare finite test per lane -- a lane is non-finite iff
// `(bits << 1)` meets the all-ones-exponent threshold, because the sign bit
// shifts out and every finite exponent shifts strictly below `inf`'s. No
// verbatim bcinr lane-mask kernel existed to copy.
// ---------------------------------------------------------------------------

/// Threshold form of the finite test for one `binary32` bit pattern: the lane
/// is non-finite (`inf`/`-inf`/NaN) iff `(bits << 1) >= 0xFF00_0000`.
///
/// The classic one-compare finite test. Shifting left by one drops the sign
/// bit and aligns the 8-bit exponent at the top of the word; `inf` (exponent
/// `0xFF`) shifts to exactly `0xFF00_0000` and every finite exponent to
/// strictly less, independent of mantissa, sign or subnormal-ness.
///
/// ```
/// use purrdf_branchless::simd_fallback::nonfinite_f32;
///
/// assert!(nonfinite_f32(f32::NAN.to_bits()));
/// assert!(!nonfinite_f32(f32::MIN.to_bits()));
/// ```
// from bcinr is_finite_fp32_branchless.rs:24-26 (masked exponent compare)
#[inline(always)]
#[must_use]
pub fn nonfinite_f32(bits: u32) -> bool {
    (bits << 1) >= 0xFF00_0000
}

/// Threshold form of the finite test for one `binary64` bit pattern: the lane
/// is non-finite iff `(bits << 1) >= 0xFFE0_0000_0000_0000`.
///
/// ```
/// use purrdf_branchless::simd_fallback::nonfinite_f64;
///
/// assert!(nonfinite_f64(f64::NAN.to_bits()));
/// assert!(!nonfinite_f64(f64::MIN.to_bits()));
/// ```
// from bcinr is_finite_fp32_branchless.rs:24-26 (masked exponent compare)
#[inline(always)]
#[must_use]
pub fn nonfinite_f64(bits: u64) -> bool {
    (bits << 1) >= 0xFFE0_0000_0000_0000
}

/// Non-finite mask over one word packing two `binary32` lanes (lane 0 in the
/// low 32 bits): bit `i` of the result covers lane `i`.
///
/// ```
/// use purrdf_branchless::simd_fallback::nonfinite_mask_f32x2;
///
/// let word = u64::from(f32::INFINITY.to_bits())
///     | u64::from(f32::NAN.to_bits()) << 32;
/// assert_eq!(nonfinite_mask_f32x2(word), 0b11);
/// ```
// from bcinr is_finite_fp32_branchless.rs:17-27 (two lanes, one word -- the
// exact packing the scaffold's `(val, aux)` pair implied, made explicit)
#[inline(always)]
#[must_use]
pub fn nonfinite_mask_f32x2(word: u64) -> u64 {
    let low = (word & 0xFFFF_FFFF) as u32;
    let low_nonfinite = u64::from((low << 1) >= 0xFF00_0000);
    let high = (word >> 32) as u32;
    let high_nonfinite = u64::from((high << 1) >= 0xFF00_0000);
    low_nonfinite | (high_nonfinite << 1)
}

/// Non-finite mask over one `binary64` bit pattern (bit 0 set iff NaN/±inf).
///
/// ```
/// assert_eq!(
///     purrdf_branchless::simd_fallback::nonfinite_mask_f64x1(f64::INFINITY.to_bits()),
///     1
/// );
/// ```
// from bcinr is_finite_fp32_branchless.rs:24-26 (masked exponent compare)
#[inline(always)]
#[must_use]
pub fn nonfinite_mask_f64x1(bits: u64) -> u64 {
    u64::from((bits << 1) >= 0xFFE0_0000_0000_0000)
}

/// Index of the first non-finite component in a word buffer, or `None` when
/// every component is finite.
///
/// `f32_lanes`: `true` packs two `binary32` lanes per word (lane 0 in the low
/// 32 bits, `to_bits` order); `false` reads one `binary64` pattern per word.
///
/// ```
/// use purrdf_branchless::simd_fallback::first_nonfinite_index;
///
/// let nan = f64::NAN.to_bits();
/// assert_eq!(first_nonfinite_index(&[1.0f64.to_bits(), nan], false), Some(1));
/// assert_eq!(first_nonfinite_index(&[], false), None);
/// ```
// from bcinr is_finite_fp32_branchless.rs (word walk over the masked compare)
#[inline]
#[must_use]
pub fn first_nonfinite_index(bits: &[u64], f32_lanes: bool) -> Option<usize> {
    if f32_lanes {
        bits.iter().enumerate().find_map(|(word, &w)| {
            let mask = nonfinite_mask_f32x2(w);
            (mask != 0).then(|| word * 2 + mask.trailing_zeros() as usize)
        })
    } else {
        bits.iter()
            .enumerate()
            .find_map(|(word, &w)| nonfinite_f64(w).then_some(word))
    }
}

/// Index of the first non-finite component of an `f32` slice, or `None`.
///
/// This is the single home the HNSW matrix admission finite-check routes
/// through (OPT-44): two lanes per word compare, one branch per two
/// components, identical `(index)` semantics to a scalar
/// `.position(|v| !v.is_finite())` walk.
///
/// ```
/// use purrdf_branchless::simd_fallback::first_nonfinite_f32;
///
/// assert_eq!(first_nonfinite_f32(&[1.0, f32::NAN, 3.0]), Some(1));
/// assert_eq!(first_nonfinite_f32(&[1.0, 2.0, 3.0]), None);
/// ```
// from bcinr is_finite_fp32_branchless.rs (word walk over the masked compare)
#[inline]
#[must_use]
pub fn first_nonfinite_f32(components: &[f32]) -> Option<usize> {
    let mut index = 0;
    while index + 2 <= components.len() {
        let word = u64::from(components[index].to_bits())
            | u64::from(components[index + 1].to_bits()) << 32;
        let mask = nonfinite_mask_f32x2(word);
        if mask != 0 {
            return Some(index + mask.trailing_zeros() as usize);
        }
        index += 2;
    }
    if index < components.len() {
        nonfinite_f32(components[index].to_bits()).then_some(index)
    } else {
        None
    }
}

/// Index of the first non-finite component of an `f64` slice, or `None`.
///
/// ```
/// use purrdf_branchless::simd_fallback::first_nonfinite_f64;
///
/// assert_eq!(first_nonfinite_f64(&[1.0, f64::NEG_INFINITY]), Some(1));
/// assert_eq!(first_nonfinite_f64(&[0.5]), None);
/// ```
// from bcinr is_finite_fp32_branchless.rs (word walk over the masked compare)
#[inline]
#[must_use]
pub fn first_nonfinite_f64(components: &[f64]) -> Option<usize> {
    let mut index = 0;
    while index < components.len() {
        if nonfinite_f64(components[index].to_bits()) {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scalar-oracle differential tests: each primitive is checked against an
    // independently written reference (bit loops / native ops), plus
    // boundaries. Non-vacuous by construction: the references are the native
    // semantic, not a re-derivation of the implementation.

    fn pdep_reference(mut val: u64, mut mask: u64) -> u64 {
        let mut res = 0u64;
        let mut dst = 0u32;
        while mask != 0 {
            let bit = mask & mask.wrapping_neg();
            if val & 1 != 0 {
                res |= bit;
            } else {
                let _ = dst; // positions without a val bit stay zero
            }
            if val & 1 != 0 {
                dst += 0; // val bits consumed in order; res already placed
            }
            val >>= 1;
            mask ^= bit;
        }
        res
    }

    fn pext_reference(mut val: u64, mut mask: u64) -> u64 {
        let mut res = 0u64;
        let mut shift = 0u32;
        while mask != 0 {
            let bit = mask & mask.wrapping_neg();
            if val & bit != 0 {
                res |= 1 << shift;
            }
            shift += 1;
            val &= !bit;
            mask ^= bit;
        }
        res
    }

    #[test]
    fn pdep_matches_reference() {
        for v in [0u64, 1, 0b101, 0x0123_4567_89AB_CDEF, u64::MAX] {
            for m in [0u64, 1, 0b0101_0100, 0x0F0F_0F0F_0F0F_0F0F, u64::MAX] {
                assert_eq!(pdep_u64(v, m), pdep_reference(v, m), "v={v:#x} m={m:#x}");
            }
        }
    }

    #[test]
    fn pext_matches_reference() {
        for v in [0u64, 1, 0b1101_0110, 0x0123_4567_89AB_CDEF, u64::MAX] {
            for m in [0u64, 1, 0b0101_0100, 0x0F0F_0F0F_0F0F_0F0F, u64::MAX] {
                assert_eq!(pext_u64(v, m), pext_reference(v, m), "v={v:#x} m={m:#x}");
            }
        }
    }

    #[test]
    fn pdep_pext_round_trip() {
        for m in [0u64, 1, 0x8080_8080_8080_8080, u64::MAX] {
            assert_eq!(
                pdep_u64(pext_u64(0xDEAD_BEEF_CAFE_F00D, m), m),
                0xDEAD_BEEF_CAFE_F00D & m
            );
        }
    }

    #[test]
    fn shuffle_matches_reference() {
        let a: [u8; 16] = core::array::from_fn(|i| (i as u8).wrapping_mul(17));
        for hi in [0u8, 0x80] {
            for idx in 0u8..=15 {
                let mut mask = [0u8; 16];
                mask[7] = idx | hi;
                let r = shuffle_u8x16(a, mask);
                let want = if hi != 0 { 0 } else { a[(idx & 0x0F) as usize] };
                assert_eq!(r[7], want);
            }
        }
    }

    #[test]
    fn hsum_matches_reference() {
        for fill in [0u8, 1, 255] {
            assert_eq!(horizontal_sum_u8x16([fill; 16]), u32::from(fill) * 16);
        }
        let a: [u8; 16] = core::array::from_fn(|i| i as u8);
        assert_eq!(horizontal_sum_u8x16(a), (0..16).sum::<u32>());
    }

    #[test]
    fn blend_matches_reference() {
        let a: [u8; 16] = core::array::from_fn(|i| i as u8);
        let b: [u8; 16] = core::array::from_fn(|i| 255 - i as u8);
        for m in [0u8, 0x7F, 0x80, 0xFF] {
            let mask = [m; 16];
            let r = blend_u8x16(mask, a, b);
            let want: [u8; 16] = core::array::from_fn(|i| if m & 0x80 != 0 { b[i] } else { a[i] });
            assert_eq!(r, want);
        }
    }

    #[test]
    fn ct_select_boundaries() {
        assert_eq!(ct_select_u64(1, u64::MAX, 0), u64::MAX);
        assert_eq!(ct_select_u64(0, u64::MAX, 0), 0);
        assert_eq!(ct_select_i64(1, i64::MIN, i64::MAX), i64::MIN);
        assert_eq!(ct_select_i64(0, i64::MIN, i64::MAX), i64::MAX);
    }

    #[test]
    fn ct_compare_boundaries() {
        assert_eq!(ct_eq_u64(0, 0), 1);
        assert_eq!(ct_eq_u64(u64::MAX, u64::MAX), 1);
        assert_eq!(ct_eq_u64(u64::MAX, 0), 0);
        assert_eq!(ct_lt_u32(0, u32::MAX), 1);
        assert_eq!(ct_lt_u32(u32::MAX, 0), 0);
        assert_eq!(ct_lt_u32(u32::MAX, u32::MAX), 0);
        assert_eq!(ct_lt_i64(i64::MIN, i64::MAX), 1);
        assert_eq!(ct_lt_i64(i64::MAX, i64::MIN), 0);
        assert_eq!(ct_lt_i64(3, 3), 0);
    }

    #[test]
    fn ct_swap_and_abs() {
        let (mut x, mut y) = (0u64, u64::MAX);
        ct_conditional_swap_u64(1, &mut x, &mut y);
        assert_eq!((x, y), (u64::MAX, 0));
        ct_conditional_swap_u64(0, &mut x, &mut y);
        assert_eq!((x, y), (u64::MAX, 0));
        assert_eq!(ct_abs_i64(i64::MIN), i64::MIN.wrapping_abs());
    }

    #[test]
    fn ct_min_max_clamp_boundaries() {
        assert_eq!(ct_min_u32(0, u32::MAX), 0);
        assert_eq!(ct_max_u32(0, u32::MAX), u32::MAX);
        assert_eq!(ct_clamp_u32(20, 20, 30), 20);
        assert_eq!(ct_clamp_u32(30, 20, 30), 30);
    }

    // ------------------------------------------------------------------
    // OPT-44 non-finite lane masks. Independent oracle: reconstruct the
    // float from bits and use the std `is_finite` predicate -- a different
    // formulation than the `(bits << 1)` threshold compare under test.
    // ------------------------------------------------------------------

    fn nonfinite_oracle_f32(bits: u32) -> bool {
        !f32::from_bits(bits).is_finite()
    }

    fn nonfinite_oracle_f64(bits: u64) -> bool {
        !f64::from_bits(bits).is_finite()
    }

    #[test]
    fn nonfinite_threshold_matches_oracle_exhaustive_boundaries() {
        // Boundary bit patterns: +-0.0, subnormals, min/max finite, +-inf
        // (the exact threshold edge), quiet/signalling NaNs, max pattern.
        let boundary_f32 = [
            0x0000_0000u32, // +0.0
            0x8000_0000,    // -0.0
            0x0000_0001,    // smallest subnormal
            0x8000_0001,    // negative smallest subnormal
            0x0080_0000,    // smallest normal
            0x7F7F_FFFF,    // max finite
            0xFF7F_FFFF,    // min finite
            0x7F7F_FFFE,
            0x7F80_0000, // +inf: (bits << 1) == threshold exactly
            0xFF80_0000, // -inf
            0x7F80_0001, // smallest NaN
            0x7FC0_0000, // quiet NaN
            0x7FFF_FFFF, // max NaN
            0xFFFF_FFFF, // max pattern
        ];
        for bits in boundary_f32 {
            assert_eq!(
                nonfinite_f32(bits),
                nonfinite_oracle_f32(bits),
                "f32 bits {bits:#010x}"
            );
        }
        // f32 threshold-edge neighborhood sweep: every exponent x
        // representative mantissas x both signs.
        for exponent in 0u32..=0xFF {
            for mantissa in [
                0u32,
                1,
                1 << 22,
                0x7F_FFFF,
                0x4000_00,
                0x2A,
                0x1234_56,
                0x55,
            ] {
                for sign in [0u32, 0x8000_0000] {
                    let bits = sign | exponent << 23 | mantissa;
                    assert_eq!(
                        nonfinite_f32(bits),
                        nonfinite_oracle_f32(bits),
                        "f32 bits {bits:#010x}"
                    );
                }
            }
        }
        // f64 boundary patterns, same edge cases.
        let boundary_f64 = [
            0x0000_0000_0000_0000u64,
            0x8000_0000_0000_0000,
            0x0000_0000_0000_0001,
            0x7FEF_FFFF_FFFF_FFFF, // max finite
            0xFFEF_FFFF_FFFF_FFFF, // min finite
            0x7FF7_FFFF_FFFF_FFFF, // just below the threshold
            0x7FF0_0000_0000_0000, // +inf: threshold edge exactly
            0xFFF0_0000_0000_0000, // -inf
            0x7FF0_0000_0000_0001, // smallest NaN
            0x7FF8_0000_0000_0000, // quiet NaN
            0xFFFF_FFFF_FFFF_FFFF,
        ];
        for bits in boundary_f64 {
            assert_eq!(
                nonfinite_f64(bits),
                nonfinite_oracle_f64(bits),
                "f64 bits {bits:#018x}"
            );
        }
    }

    #[test]
    fn nonfinite_mask_f32x2_lane_positions() {
        let f = 1.0f32.to_bits();
        let n = f32::NAN.to_bits();
        let i = f32::INFINITY.to_bits();
        assert_eq!(nonfinite_mask_f32x2(u64::from(f) | u64::from(f) << 32), 0);
        assert_eq!(nonfinite_mask_f32x2(u64::from(n) | u64::from(f) << 32), 1);
        assert_eq!(nonfinite_mask_f32x2(u64::from(f) | u64::from(n) << 32), 2);
        assert_eq!(nonfinite_mask_f32x2(u64::from(i) | u64::from(i) << 32), 3);
    }

    #[test]
    fn first_nonfinite_index_differential() {
        extern crate std;
        use std::vec::Vec;

        let bits = f64::to_bits;
        let nan = f64::NAN.to_bits();
        let pinf = f64::INFINITY.to_bits();
        let ninf = f64::NEG_INFINITY.to_bits();
        let finite_oracle = |components: &[u64]| -> Option<usize> {
            components
                .iter()
                .position(|&w| !f64::from_bits(w).is_finite())
        };
        // f64 lanes (one per word): all-finite, NaN, +-inf, mixed, first-hit.
        let corpora: [&[u64]; 8] = [
            &[],
            &[bits(1.0), bits(2.0), bits(-3.5)],
            &[nan],
            &[pinf, bits(1.0)],
            &[bits(1.0), bits(2.0), ninf],
            &[bits(0.0), nan, bits(0.0), pinf, ninf],
            &[0xFFEF_FFFF_FFFF_FFFF, nan],
            &[nan, nan, nan],
        ];
        for corpus in corpora {
            assert_eq!(
                first_nonfinite_index(corpus, false),
                finite_oracle(corpus),
                "f64 word differential"
            );
        }
        // f32 lanes packed two per word vs a scalar position walk.
        let lanes = [1.0f32, f32::NAN, f32::NEG_INFINITY, 0.0, f32::INFINITY, 2.5];
        let words: Vec<u64> = lanes
            .chunks(2)
            .map(|pair| u64::from(pair[0].to_bits()) | u64::from(pair[1].to_bits()) << 32)
            .collect();
        let expected = lanes.iter().position(|value| !value.is_finite());
        assert_eq!(first_nonfinite_index(&words, true), expected);
        // Adversarial: non-finite in the HIGH lane of the LAST word only.
        let high_only: Vec<u64> = (0..16)
            .map(|i| {
                if i == 15 {
                    u64::from(f32::NAN.to_bits()) << 32
                } else {
                    u64::from(1.0f32.to_bits()) << 32 | u64::from(1.0f32.to_bits())
                }
            })
            .collect();
        assert_eq!(first_nonfinite_index(&high_only, true), Some(31));
    }

    #[test]
    fn first_nonfinite_slice_helpers_differential() {
        extern crate std;
        use std::vec::Vec;

        let mut f32_corpora: Vec<Vec<f32>> = std::vec![
            std::vec![],
            std::vec![1.0],
            std::vec![1.0, 2.0, 3.0, 4.0, 5.0],
            std::vec![f32::NAN],
            std::vec![1.0, 2.0, f32::NAN, 4.0],
            std::vec![f32::NEG_INFINITY, f32::NAN],
            std::vec![1.0; 1025],
            std::vec![1.0; 1024],
        ];
        f32_corpora.push({
            let mut long = std::vec![1.0f32; 997];
            long[996] = f32::INFINITY;
            long
        });
        for corpus in &f32_corpora {
            assert_eq!(
                first_nonfinite_f32(corpus),
                corpus.iter().position(|value| !value.is_finite()),
                "f32 helper differential, len {}",
                corpus.len()
            );
        }
        // Valid-neighbor mirror: every finite boundary pattern must NOT be
        // flagged (over-refusal is the mirror bug of a missed NaN).
        let finite_boundaries = [
            0.0f32,
            -0.0,
            f32::MIN_POSITIVE,
            f32::from_bits(1), // smallest subnormal
            f32::MIN,
            f32::MAX,
        ];
        for value in finite_boundaries {
            assert_eq!(first_nonfinite_f32(&[value]), None);
        }
        let mut f64_corpora: Vec<Vec<f64>> = std::vec![
            std::vec![],
            std::vec![0.0, -0.0, f64::MIN_POSITIVE, f64::from_bits(1)],
            std::vec![1.0; 513],
        ];
        f64_corpora.push({
            let mut long = std::vec![1.0f64; 4096];
            long[3000] = f64::NAN;
            long[4095] = f64::NEG_INFINITY; // first hit wins
            long
        });
        for corpus in &f64_corpora {
            assert_eq!(
                first_nonfinite_f64(corpus),
                corpus.iter().position(|value| !value.is_finite()),
                "f64 helper differential, len {}",
                corpus.len()
            );
        }
        let f64_boundaries = [
            0.0f64,
            -0.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::MIN,
            f64::MAX,
        ];
        for value in f64_boundaries {
            assert_eq!(first_nonfinite_f64(&[value]), None);
        }
    }
}

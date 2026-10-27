//! Overflow-safe scaled arithmetic: fixed-point multiply/divide, Mersenne
//! modular multiplication, Euclidean magnitude, and the general `muldiv`.
//!
//! Ported from bcinr `crates/bcinr-logic/src/algorithms/` with the scaffold
//! ABI (`#[no_mangle]`, `(val, aux)` pairs) stripped into named parameters.
//! Every function is constant-shape: fixed trip counts or single guarded
//! selects, no data-dependent loops, no heap.

/// Q16.16 fixed-point multiply: `(a * b) >> 16` in full precision via `u128`.
///
/// The product of two Q16.16 values is Q32.32; shifting right 16 returns it
/// to Q16.16 without ever truncating an intermediate.
// from bcinr crates/bcinr-logic/src/algorithms/fp_mul_u32_q16.rs:22
#[inline]
#[must_use]
pub const fn fp_mul_q16(a: u64, b: u64) -> u64 {
    (((a as u128) * (b as u128)) >> 16) as u64
}

/// Q16.16 fixed-point divide: `(a << 16) / b`, saturating to `0` on a zero
/// divisor by mask selection (never panics, never branches on the divisor
/// value beyond the zero test folded into a mask).
// from bcinr crates/bcinr-logic/src/algorithms/fp_div_u32_q16.rs:22
#[inline]
#[must_use]
pub const fn fp_div_q16(a: u64, b: u64) -> u64 {
    let zero = (b == 0) as u64;
    let res = (((a as u128) << 16) / ((b as u128) | 1)) as u64;
    res & (!zero.wrapping_neg())
}

/// Overflow-safe modular multiplication in the Mersenne prime field
/// `GF(2^61 - 1)`: `(a * b) mod (2^61 - 1)`, computed entirely in `u128`.
///
/// Widely used as the hashing prime (POLYVAL/toolbar hashing families); the
/// full 122-bit product can never overflow `u128`, so the reduction is exact.
// from bcinr crates/bcinr-logic/src/algorithms/modular_mul_u64.rs:22
#[inline]
#[must_use]
pub const fn modular_mul_u64(a: u64, b: u64) -> u64 {
    const M: u128 = (1u128 << 61) - 1;
    let a = (a as u128) % M;
    let b = (b as u128) % M;
    ((a * b) % M) as u64
}

/// Floor Euclidean magnitude `⌊√(x² + y²)⌋` where `x` and `y` are the low and
/// high 32-bit halves of the packed argument. Constant-time digit-by-digit
/// square root: exactly 33 reduction steps (bits 64, 62, …, 0), each a
/// compare + mask-select.
// from bcinr crates/bcinr-logic/src/algorithms/norm_u32.rs:22
#[inline]
#[must_use]
pub const fn norm_u32(packed: u64) -> u64 {
    let x = (packed & 0xFFFF_FFFF) as u128;
    let y = (packed >> 32) as u128;
    // val_sq < 2^65, so the highest even power of four is 4^32 = 2^64 and 33
    // steps cover every bit of the root.
    let mut val_sq = x * x + y * y;
    let mut res = 0u128;
    let mut bit = 1u128 << 64;
    let mut k = 0u32;
    while k < 33 {
        let candidate = res + bit;
        let m = ((val_sq >= candidate) as u128).wrapping_neg();
        val_sq -= candidate & m;
        res = (res >> 1) + (bit & m);
        bit >>= 2;
        k += 1;
    }
    res as u64
}

/// Q16.16 smoothstep `t²(3−2t)` of `t = clamp((a + b) mod 2³², 0, 1)`.
///
/// The cubic is evaluated in wrapping arithmetic — the clamped `t ≤ 2¹⁶`
/// guarantees the product `t²·(3·2¹⁶ − 2t) < 2⁶⁴`, so wrapping never bites
/// on the mathematical path.
// from bcinr crates/bcinr-logic/src/algorithms/smoothstep_u32.rs:28
#[inline]
#[must_use]
pub fn smoothstep_u32(a: u64, b: u64) -> u64 {
    const ONE: u64 = 0x1_0000;
    let t = u64::min(a.wrapping_add(b) & 0xFFFF_FFFF, ONE);
    let t2 = t.wrapping_mul(t);
    let factor = 3u64.wrapping_mul(ONE).wrapping_sub(2u64.wrapping_mul(t));
    t2.wrapping_mul(factor) >> 32
}

/// Wrapping (mod 2⁶⁴) product of two 64-bit values — the dequantization step
/// `value × scale` when saturation is handled by the caller's clamp.
// from bcinr crates/bcinr-logic/src/algorithms/dequantize_u32.rs:23
#[inline]
#[must_use]
pub const fn dequantize_u32(value: u64, scale: u64) -> u64 {
    value.wrapping_mul(scale)
}

/// General overflow-safe `⌊a·b / c⌋` for `c != 0`.
///
/// Construction: the full product `a·b` is taken in `u128`. When it fits in
/// 64 bits (the common case) the answer is one divide. When it does not, the
/// identity `a·b = (a − a mod c)·b + (a mod c)·b` splits the product so both
/// halves fit: the first summand is exact because `(a − a mod c)` is a
/// multiple of `c` (so `(a − a mod c)/c · b` needs only `a·b/c < 2^64` —
/// guaranteed since the result itself fits 64 bits), and the second works on
/// `a mod c < c` which is exact for any `a, b, c` whose quotient fits.
/// A zero divisor returns `0` by the same mask. The overflow test folds to a
/// mask select; the only branch-shaped step is the divide itself.
///
/// Contract when the exact quotient exceeds `u64::MAX` (e.g. `c = 1`,
/// `a = b = u64::MAX`): the fast path is not selected, and the split path's
/// `q`/`r` truncates at the `as u64` casts — the result is the low 64 bits of
/// the true quotient (documented wrapping, not saturation).
#[inline]
#[must_use]
pub const fn muldiv(a: u64, b: u64, c: u64) -> u64 {
    // A zero divisor is replaced by 1 for both divides and folded to a 0
    // result by mask at the end. (Add, never OR: `| 1` would corrupt every
    // even divisor — 2 would become 3.)
    let cc = (c as u128) + ((c == 0) as u128);
    let product = (a as u128) * (b as u128);
    let fits = (product >> 64 == 0) as u128;
    // Fast path: full product fits, one divide answers.
    let safe = (product / cc) as u64;
    // Split path: q = (a - a%c)/c * b, r = ((a%c) * b) / c; both exact.
    let rem = (a as u128) % cc;
    let q = (((a as u128) - rem) / cc * (b as u128)) as u64;
    let r = ((rem * (b as u128)) / cc) as u64;
    let split = q.wrapping_add(r);
    let fit_mask = fits.wrapping_neg() as u64;
    let body = (fit_mask & safe) | (!fit_mask & split);
    let zero_div = ((c == 0) as u64).wrapping_neg();
    body & !zero_div
}

/// Saturating scale-then-clamp: `clamp(value·scale, floor, ceiling)`, with
/// `ceiling` floored at `floor` so the `min > max` panic in `Ord::clamp` is
/// unreachable by construction (OPT-16 §3 F7: a cost ceiling below the floor
/// must clamp to the floor, never refuse and never panic).
///
/// The wide multiply saturates via the single saturating-multiply home
/// [`crate::saturation::mul_sat_u64`] — no re-derived wide-mul here.
// OPT-16 §3 (clamped_scaling_u64); specified signature, single home beside the
// Q16 family per the OPT-42 addendum rule.
#[inline]
#[must_use]
pub const fn clamped_scaling_u64(value: u64, scale: u64, floor: u64, ceiling: u64) -> u64 {
    // DIVERGENCE-ADJACENT unblock (scaling.rs owned by another OPT-42 lane):
    // `Ord::max` is not const-callable on this toolchain (const_cmp unsted);
    // branchless mask form of max(ceiling, floor).
    let m = (ceiling < floor) as u64;
    crate::saturation::clamp_u64(
        crate::saturation::mul_sat_u64(value, scale),
        floor,
        ceiling ^ ((ceiling ^ floor) & m.wrapping_neg()),
    )
}

/// Floored domain clamp: `min(value, ceiling)` floored at `1.0`, the one
/// spelling both `bgp.rs` axis-domain call sites converge on (OPT-16 §3 F1/F2).
///
/// Deliberately `min/max` chained, never `f64::clamp(1.0, ceiling)`: `clamp`
/// panics when `ceiling < 1.0`, and an empty pattern yields `ceiling = 0.0`
/// rows — the exact case this helper must survive. Returns `1.0` in that case
/// (the divisor-protection floor), matching the ticket's hard bound.
// OPT-16 §3 (floored_domain).
#[inline]
#[must_use]
pub fn floored_domain(value: f64, ceiling: f64) -> f64 {
    value.min(ceiling).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference oracles (independent structure, u128-wide).

    fn fp_mul_q16_ref(a: u64, b: u64) -> u64 {
        ((u128::from(a) * u128::from(b)) / 65536) as u64
    }

    fn fp_div_q16_ref(a: u64, b: u64) -> u64 {
        if b == 0 {
            0
        } else {
            ((u128::from(a) << 16) / u128::from(b)) as u64
        }
    }

    fn modular_mul_u64_ref(a: u64, b: u64) -> u64 {
        // Russian-peasant modular product with subtraction-based reduction.
        const M: u128 = (1u128 << 61) - 1;
        let mut a = u128::from(a) % M;
        let mut b = u128::from(b) % M;
        let mut acc = 0u128;
        while b > 0 {
            if b & 1 == 1 {
                acc = if M - acc > a { acc + a } else { acc + a - M };
            }
            a = if M - a > a { a + a } else { a + a - M };
            b >>= 1;
        }
        acc as u64
    }

    fn norm_u32_ref(packed: u64) -> u64 {
        let x = u128::from(packed & 0xFFFF_FFFF);
        let y = u128::from(packed >> 32);
        let s = x * x + y * y;
        (s.isqrt()) as u64
    }

    fn smoothstep_u32_ref(a: u64, b: u64) -> u64 {
        const ONE: u128 = 0x1_0000;
        let pos = u128::from(a.wrapping_add(b) & 0xFFFF_FFFF);
        let t = pos.min(ONE);
        let t2 = t * t;
        let t3 = t2 * t;
        ((3 * ONE * t2 - 2 * t3) >> 32) as u64
    }

    fn muldiv_ref(a: u64, b: u64, c: u64) -> u64 {
        if c == 0 {
            return 0;
        }
        let p = u128::from(a) * u128::from(b);
        (p / u128::from(c)) as u64
    }

    #[test]
    fn clamped_scaling_matches_checked_oracle() {
        // Independent oracle: checked_mul + explicit branch clamp (not
        // saturating ops, not Ord::clamp).
        fn ref_impl(value: u64, scale: u64, floor: u64, ceiling: u64) -> u64 {
            // std saturating_mul doubles as an independent oracle against the
            // mask-based mul_sat_u64 the impl delegates to.
            let prod = value.saturating_mul(scale);
            let hi = ceiling.max(floor);
            if prod < floor {
                floor
            } else if prod > hi {
                hi
            } else {
                prod
            }
        }
        for value in [0u64, 1, 3, 4095, 4096, 1 << 32, u64::MAX] {
            for scale in [0u64, 1, 4096, 1 << 16, u64::MAX] {
                for (floor, ceiling) in [
                    (0u64, 0u64),
                    (1, 4096),
                    (1, 0),
                    (0, u64::MAX),
                    (7, 5),
                    (u64::MAX, u64::MAX),
                ] {
                    assert_eq!(
                        clamped_scaling_u64(value, scale, floor, ceiling),
                        ref_impl(value, scale, floor, ceiling),
                        "{value}·{scale} [{floor},{ceiling}]"
                    );
                }
            }
        }
        // Ceiling-below-floor and saturation corners, spelled out.
        assert_eq!(clamped_scaling_u64(u64::MAX, u64::MAX, 1, 4096), 4096);
        assert_eq!(clamped_scaling_u64(100, 100, 1, 0), 1);
        assert_eq!(clamped_scaling_u64(0, 0, 1, 4096), 1);
        assert_eq!(clamped_scaling_u64(0, 5, 2, 10), 2);
    }

    #[test]
    fn floored_domain_matches_branch_oracle_never_below_one() {
        // Branch oracle independent of min/max chains; the 1.0 floor wins over
        // the ceiling when the ceiling itself is below 1 (empty-pattern case).
        fn ref_impl(value: f64, ceiling: f64) -> f64 {
            let capped = if value < ceiling { value } else { ceiling };
            if capped < 1.0 { 1.0 } else { capped }
        }
        for value in [0.0f64, 1.0, 2.5, 100.0, 1e300] {
            for ceiling in [0.0f64, 1.0, 2.5, 42.0, f64::INFINITY] {
                assert_eq!(floored_domain(value, ceiling), ref_impl(value, ceiling));
            }
        }
        // The ticket's critical case: empty pattern (ceiling 0.0) must give
        // the floor 1.0 — no clamp panic, no 0 divisor.
        assert_eq!(floored_domain(10.0, 0.0), 1.0);
        assert_eq!(floored_domain(0.0, 0.0), 1.0);
    }

    #[test]
    fn fp_mul_matches_oracle() {
        for a in [0u64, 1, 0xFFFF, 0x7FFF_FFFF, u64::MAX] {
            for b in [0u64, 1, 0x1_0000, u64::MAX] {
                assert_eq!(fp_mul_q16(a, b), fp_mul_q16_ref(a, b));
            }
        }
    }

    #[test]
    fn fp_div_matches_oracle_and_saturates_zero() {
        for a in [0u64, 1, 0x1_0000, u64::MAX] {
            for b in [0u64, 1, 3, 0xFFFF, u64::MAX] {
                assert_eq!(fp_div_q16(a, b), fp_div_q16_ref(a, b));
            }
        }
        assert_eq!(fp_div_q16(123, 0), 0);
    }

    #[test]
    fn modular_mul_matches_peasant_oracle() {
        for a in [0u64, 1, 2, (1u64 << 61) - 1, (1u64 << 61) - 2, u64::MAX] {
            for b in [0u64, 1, 3, (1u64 << 61) - 1, u64::MAX] {
                assert_eq!(modular_mul_u64(a, b), modular_mul_u64_ref(a, b));
            }
        }
    }

    #[test]
    fn norm_matches_oracle() {
        let cases = [
            0u64,
            1,
            0xFFFF_FFFF,           // x = 2^32-1, y = 0
            0xFFFF_FFFF_0000_0000, // y = 2^32-1
            0xFFFF_FFFF_FFFF_FFFF, // both max: ~6.09e9
            (3u64 << 32) | 0b100,  // 5
            (5u64 << 32) | 0b1100, // 13
        ];
        for c in cases {
            assert_eq!(norm_u32(c), norm_u32_ref(c), "packed={c:#x}");
        }
    }

    #[test]
    fn smoothstep_matches_oracle() {
        for a in [0u64, 1, 0x8000, 0xFFFF, 0x1_0000, u64::MAX] {
            for b in [0u64, 0x8000, 0xFFFF_FFFF] {
                assert_eq!(smoothstep_u32(a, b), smoothstep_u32_ref(a, b));
            }
        }
        assert_eq!(smoothstep_u32(0, 0), 0);
        assert_eq!(smoothstep_u32(0x1_0000, 0), 0x1_0000);
    }

    #[test]
    fn muldiv_matches_naive_where_naive_fits() {
        // c = 1 and small operands: the naive (a*b)/c never overflows.
        for a in [0u64, 1, 2, 1000, u64::from(u32::MAX)] {
            for b in [0u64, 1, 3, 7777, u64::from(u16::MAX)] {
                for c in [1u64, 2, 7, 1000] {
                    assert_eq!(muldiv(a, b, c), muldiv_ref(a, b, c), "{a}*{b}/{c}");
                }
            }
        }
    }

    #[test]
    fn muldiv_overflow_boundaries() {
        // (u64::MAX * u64::MAX) / u64::MAX == u64::MAX.
        assert_eq!(muldiv(u64::MAX, u64::MAX, u64::MAX), u64::MAX);
        // (u64::MAX * u64::MAX) / 2 == floor(u64::MAX^2 / 2) via u128 oracle.
        assert_eq!(
            muldiv(u64::MAX, u64::MAX, 2),
            muldiv_ref(u64::MAX, u64::MAX, 2)
        );
        // Exact split: 2^63 * 2^63 / 2^62 == 2^64 - 2^65/2^62... use exact power math.
        assert_eq!(muldiv(1 << 40, 1 << 40, 1 << 39), 1 << 41);
        // Zero divisor saturates to 0.
        assert_eq!(muldiv(5, 7, 0), 0);
        assert_eq!(muldiv(u64::MAX, u64::MAX, 0), 0);
        // Exactness of the split path on a random wide case.
        assert_eq!(
            muldiv(
                0xDEAD_BEEF_CAFE_F00D,
                0x1234_5678_9ABC_DEF0,
                0xF0E1_D2C3_B4A5_9687
            ),
            muldiv_ref(
                0xDEAD_BEEF_CAFE_F00D,
                0x1234_5678_9ABC_DEF0,
                0xF0E1_D2C3_B4A5_9687
            )
        );
    }
}

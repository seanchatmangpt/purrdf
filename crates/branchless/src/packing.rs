//! Bit packing, lane shuffling, and bit-permutation kernels.
//!
//! Ported from `bcinr` (`crates/bcinr-logic/src/algorithms/`) with the
//! scaffold ABI stripped (`#[no_mangle]`, `(val, aux)` pairs → named params)
//! and vacuous-mutant test scaffolding excluded. Bulk bit-packed vectors
//! remain the home of `purrdf-core` (`crates/rdf-core/src/ir/pack/bits.rs`
//! `IntVector` / `RankSelect`) — this module owns only the per-word kernels.
//!
//! Contract: `#![no_std]`, zero heap, zero unsafe, data-independent control
//! flow (fixed-shape loops only).

// ---------------------------------------------------------------------------
// Field deposit / extract (the IntVector inner step)
// ---------------------------------------------------------------------------

/// Deposits the low `k` bits of `value` at bit `offset` of `word`
/// (field insert: `(word & !field_mask) | ((value & field_mask) << offset)`).
///
/// `k` is clamped to 1..=32 and `offset` to 0..=31, so the field always fits
/// a `u64` word. The bcinr form replaces the whole field; the deposit here
/// masks the old field out first (see [`field_extract`] for the inverse read).
///
/// from bcinr algorithms/bitpacking_encode_u32_k.rs:24-31
#[must_use]
pub fn field_deposit(word: u64, value: u64, k: u32, offset: u32) -> u64 {
    let k = k.clamp(1, 32);
    let offset = offset & 31;
    let field_mask = mask_low(k);
    (word & !(field_mask << offset)) | ((value & field_mask) << offset)
}

/// Extracts the `k`-bit field at bit `offset` of `word` (zero-extended).
///
/// from bcinr algorithms/bitpacking_decode_u32_k.rs:24-31
#[must_use]
pub fn field_extract(word: u64, k: u32, offset: u32) -> u64 {
    let k = k.clamp(1, 32);
    let offset = offset & 31;
    (word >> offset) & mask_low(k)
}

/// `field_mask = (1 << k) - 1` without the `k >= 64` shift-overflow branch
/// (`k` is pre-clamped by callers; `mask_low(32)` is the widest field here).
#[inline]
fn mask_low(k: u32) -> u64 {
    (1u64 << k).wrapping_sub(1)
}

// ---------------------------------------------------------------------------
// Elias-Fano per-value decomposition
// ---------------------------------------------------------------------------

/// Splits `value` at width `w` into the Elias-Fano per-value form: low bits in
/// place, plus a single unary marker bit for the high part, packed into one
/// word. `w` is taken mod 64; the high part is capped so the marker stays in
/// word (`hi <= 63 - w`), matching the bcinr boundary behavior.
///
/// from bcinr algorithms/bit_vector_compress_elias_fano.rs:24-31
#[must_use]
pub fn elias_fano_split(value: u64, w: u32) -> u64 {
    let w = w & 63;
    let lo_mask = (1u64 << w).wrapping_sub(1);
    let lo = value & lo_mask;
    let hi = value >> w;
    let hi_capped = crate::select::min_u64(hi, (63 - w) as u64);
    lo | (1u64 << ((w) as u64 + hi_capped))
}

// ---------------------------------------------------------------------------
// SWAR lane delta coding (2×u32)
// ---------------------------------------------------------------------------

/// Per-lane wrapping difference of two 2×u32 lane words:
/// lanes `val - prev` (mod 2³²), packed as `[d1 | d0]`.
///
/// from bcinr algorithms/delta_encode_simd_u32.rs
#[must_use]
pub fn delta_encode_u32x2(value: u64, prev: u64) -> u64 {
    let d0 = (value as u32).wrapping_sub(prev as u32);
    let d1 = ((value >> 32) as u32).wrapping_sub((prev >> 32) as u32);
    ((d0) as u64) | (((d1) as u64) << 32)
}

/// Per-lane wrapping sum of two 2×u32 lane words (the [`delta_encode_u32x2`]
/// inverse): lanes `delta + prev` (mod 2³²).
///
/// from bcinr algorithms/delta_decode_simd_u32.rs
#[must_use]
pub fn delta_decode_u32x2(delta: u64, prev: u64) -> u64 {
    let r0 = (delta as u32).wrapping_add(prev as u32);
    let r1 = ((delta >> 32) as u32).wrapping_add((prev >> 32) as u32);
    ((r0) as u64) | (((r1) as u64) << 32)
}

// ---------------------------------------------------------------------------
// Bit-matrix transposes (delta-swap stages)
// ---------------------------------------------------------------------------

/// Transposes an 8×8 bit matrix held in one `u64` (bit `i*8 + j` → bit
/// `j*8 + i`) via the classic three delta-swap stages
/// (`0x00AA…`, `0x0000CCCC…`, `0x…F0F0F0F0`).
///
/// from bcinr algorithms/bit_matrix_transpose_8x8.rs:24-34
#[must_use]
pub fn transpose8x8(word: u64) -> u64 {
    let mut x = word;
    let t = (x ^ (x >> 7)) & 0x00AA_00AA_00AA_00AA;
    x ^= t ^ (t << 7);
    let t = (x ^ (x >> 14)) & 0x0000_CCCC_0000_CCCC;
    x ^= t ^ (t << 14);
    let t = (x ^ (x >> 28)) & 0x0000_0000_F0F0_F0F0;
    x ^= t ^ (t << 28);
    x
}

/// 64×64 bit-matrix transpose over the XOR-combined pair `(a ^ b)` — the
/// same three-stage delta-swap cascade as [`transpose8x8`], applied to the
/// combined word so the result is the transposed 8×8 block view of `a ^ b`.
///
/// from bcinr algorithms/bit_matrix_transpose_64x64.rs:24-33
#[must_use]
pub fn transpose64x64_combined(a: u64, b: u64) -> u64 {
    transpose8x8(a ^ b)
}

// ---------------------------------------------------------------------------
// Delta-swap / single-bit-swap primitives
// ---------------------------------------------------------------------------

/// Delta-swap stage: exchanges bit pairs a fixed distance apart. Pairs of bits
/// distance `shift` apart selected by `mask` are exchanged (`shift` masked to
/// 0..=63). The classic involutionary permutation primitive behind the
/// transposes and the Benes network.
///
/// from bcinr algorithms/delta_swap_u64.rs:24-33
#[must_use]
pub fn delta_swap(word: u64, mask: u64, shift: u32) -> u64 {
    let shift = shift & 63;
    let t = ((word >> shift) ^ word) & mask;
    word ^ t ^ (t << shift)
}

/// Single bit exchange: swaps bits `i` and `j` of `word` (`i = selector & 63`,
/// `j = (selector >> 6) & 63`). Data-independent: the exchange difference is
/// computed arithmetically and applied by XOR regardless of bit values.
///
/// from bcinr algorithms/bit_swap_u64.rs:24-31
#[must_use]
pub fn bit_swap(word: u64, selector: u64) -> u64 {
    let i = (selector & 63) as u32;
    let j = ((selector >> 6) & 63) as u32;
    let d = ((word >> i) ^ (word >> j)) & 1;
    word ^ ((d << i) | (d << j))
}

/// Fixed shift-8 delta-swap (the Hacker's Delight `bit_permute_step` form).
///
/// from bcinr algorithms/bit_permute_step_u64.rs:24-28
#[must_use]
pub fn permute_step8(word: u64, mask: u64) -> u64 {
    let t = ((word >> 8) ^ word) & mask;
    word ^ t ^ (t << 8)
}

/// Five-to-six-stage Benes-style delta-swap routing network over one word:
/// exchange distances 1, 2, 4, 8, 16, 32 with masks derived by rotating the
/// selector (`aux.rotate_left(11/23/31/43/53)` in the source).
///
/// from bcinr algorithms/benes_network_u64.rs:24-40
#[must_use]
pub fn benes_network(word: u64, selector: u64) -> u64 {
    let t1 = ((word >> 1) ^ word) & selector;
    let s1 = word ^ t1 ^ (t1 << 1);
    let m2 = selector.rotate_left(11);
    let t2 = ((s1 >> 2) ^ s1) & m2;
    let s2 = s1 ^ t2 ^ (t2 << 2);
    let m3 = selector.rotate_left(23);
    let t3 = ((s2 >> 4) ^ s2) & m3;
    let s3 = s2 ^ t3 ^ (t3 << 4);
    let m4 = selector.rotate_left(31);
    let t4 = ((s3 >> 8) ^ s3) & m4;
    let s4 = s3 ^ t4 ^ (t4 << 8);
    let m5 = selector.rotate_left(43);
    let t5 = ((s4 >> 16) ^ s4) & m5;
    let s5 = s4 ^ t5 ^ (t5 << 16);
    let m6 = selector.rotate_left(53);
    let t6 = ((s5 >> 32) ^ s5) & m6;
    s5 ^ t6 ^ (t6 << 32)
}

// ---------------------------------------------------------------------------
// Software PEXT / PDEP (no-unsafe alternatives)
// ---------------------------------------------------------------------------

/// Parallel bit extract (PEXT): packs the bits of `value` selected by `mask`
/// into the low end of the result, in order. Software form of the x86 BMI2
/// `pext` instruction — O(bits) fixed stages of the Hacker's Delight
/// compress network, the no-`unsafe` alternative to `core::arch`.
///
/// from bcinr algorithms/compress_bits_u64.rs:24-96
///
/// Kept as its own formulation (fixed 6-stage network; branch-cheaper than a
/// per-bit loop). The loop-form alternative lives in
/// [`crate::simd_fallback::pext_u64`]; the inverse deposit is delegated —
/// see [`deposit_bits`].
///
/// DIVERGENCE from bcinr: the 6-iteration `for` loop is spelled as a fixed
/// `while` so the body is `const fn`-compatible (`for` desugars to
/// non-const `Iterator` calls). Stage math is byte-identical.
#[must_use]
pub const fn compress_bits(value: u64, mask: u64) -> u64 {
    let mut x = value & mask;
    let mut m = mask;
    let mut mk = !m << 1;
    // Six fixed stages with shifts 1, 2, 4, 8, 16, 32.
    let mut stage = 0u32;
    while stage < 6 {
        let shift = 1u32 << stage;
        let mut mp = mk ^ (mk << 1);
        mp ^= mp << 2;
        mp ^= mp << 4;
        mp ^= mp << 8;
        mp ^= mp << 16;
        mp ^= mp << 32;
        let mv = mp & m;
        m = (m ^ mv) | (mv >> shift);
        let t = x & mv;
        x = (x ^ t) | (t >> shift);
        mk &= !mp;
        stage += 1;
    }
    x
}

/// Parallel bit deposit (PDEP): scatters the low popcount(mask) bits of
/// `value` into the positions selected by `mask` (the [`compress_bits`]
/// inverse; software `pdep`).
///
/// Single home: delegates to [`crate::simd_fallback::pdep_u64`] — the two
/// were byte-for-byte equivalent O(64) software PDEP kernels ported
/// independently from bcinr (`bext_u64.rs` / `parallel_bits_deposit_u64.rs`
/// vs `simd_dispatch.rs:932`); this wrapper keeps the packing-side name for
/// API stability.
///
/// from bcinr algorithms/bext_u64.rs / parallel_bits_deposit_u64.rs:23-33
#[must_use]
#[inline]
pub fn deposit_bits(value: u64, mask: u64) -> u64 {
    crate::simd_fallback::pdep_u64(value, mask)
}

// ---------------------------------------------------------------------------
// Straddling field extract / deposit (u128 window formulation)
// ---------------------------------------------------------------------------

/// Extracts the `k`-bit field starting at bit `offset` of the 128-bit word
/// `(hi:lo)`, zero-extended. Unlike [`field_extract`], the field may straddle
/// the `lo`/`hi` boundary.
///
/// Contract: `k` in 1..=64 (NEVER clamped — a `k` outside that range is a
/// caller bug and yields an unspecified mask), `offset` in 0..=63.
///
/// DIVERGENCE from the OPT-63 ticket formulation: the ticket sketched
/// `hi << (64 - offset)`, which performs a shift-by-64 on `u64` at `offset ==
/// 0` (a hole). This port uses the u128 window
/// `((((hi) as u128) << 64) | (lo) as u128) >> offset` instead, which is
/// hole-free for every `offset` in 0..=63; the low 64 bits of the window are
/// exactly the requested field.
///
/// from bcinr algorithms/bitpacking_encode_u32_k.rs:24-31 (single-word form;
/// this is the u128 two-word generalization — OPT-63 T-C)
#[must_use]
pub const fn field_extract2(lo: u64, hi: u64, k: u32, offset: u32) -> u64 {
    let window = (((hi) as u128) << 64) | (lo) as u128;
    // u128 shift by `offset <= 63` never overflows the 128-bit window.
    let shifted = window >> offset;
    // k == 64 → shift 0 (full mask); k in 1..=63 → (1 << k) - 1. The
    // `& 63` keeps the u64 shift amount in range; the `wrapping_sub` keeps
    // `64 - k` panic-free for any u32 input without a branch or clamp.
    let mask = u64::MAX >> (64u32.wrapping_sub(k) & 63);
    shifted as u64 & mask
}

/// Deposits the low `k` bits of `value` at bit `offset` of the 128-bit word
/// `(hi:lo)`, preserving every bit outside the field. Inverse of
/// [`field_extract2`] for `value` already masked to `k` bits.
///
/// Contract: `k` in 1..=64 (NEVER clamped), `offset` in 0..=63; the field
/// `offset + k <= 128` always fits the pair. Same u128-window formulation as
/// [`field_extract2`] — see its divergence note for why the ticket's
/// `hi << (64 - offset)` sketch was rejected.
///
/// from bcinr algorithms/bitpacking_decode_u32_k.rs:24-31 (single-word form;
/// this is the u128 two-word generalization — OPT-63 T-C)
#[must_use]
pub const fn field_deposit2(lo: u64, hi: u64, value: u64, k: u32, offset: u32) -> (u64, u64) {
    let window = (((hi) as u128) << 64) | (lo) as u128;
    let mask = u64::MAX >> (64u32.wrapping_sub(k) & 63);
    let field_mask = ((mask) as u128) << offset;
    let out = (window & !field_mask) | (((value & mask) as u128) << offset);
    (out as u64, (out >> 64) as u64)
}

// ---------------------------------------------------------------------------
// Morton / space-filling-curve keys (OPT-53)
// ---------------------------------------------------------------------------

/// Spreads the 32 bits of `x` to the even bit positions of a `u64`
/// (bit `i` → bit `2*i`), the 2-D Morton *part* function.
///
/// DIVERGENCE from the OPT-53 composition plan: the plan composed this over
/// `deposit_bits` → `simd_fallback::pdep_u64`, but making that path `const`
/// requires editing `simd_fallback.rs`, which this lane does not own. Per the
/// dispatch fallback clause this is a self-contained fixed 5-stage mask-shift
/// body (XOR-emit form) instead of a `pdep` wrapper.
///
/// from bcinr algorithms/morton_encode_2d_u32.rs:22-33 (spread stages)
#[must_use]
pub const fn morton_spread_2d(x: u32) -> u64 {
    let mut v = (x) as u64;
    v = (v ^ (v << 16)) & 0x0000_FFFF_0000_FFFF;
    v = (v ^ (v << 8)) & 0x00FF_00FF_00FF_00FF;
    v = (v ^ (v << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
    v = (v ^ (v << 2)) & 0x3333_3333_3333_3333;
    v = (v ^ (v << 1)) & 0x5555_5555_5555_5555;
    v
}

/// Compacts the even bit positions of `m` back into a `u32`
/// (bit `2*i` → bit `i`); inverse of [`morton_spread_2d`].
///
/// DIVERGENCE from the OPT-53 composition plan: self-contained fixed 5-stage
/// mask-shift body (OR-collect form) rather than a
/// `compress_bits`/`pext_u64` wrapper — same lane-ownership reason as
/// [`morton_spread_2d`].
///
/// from bcinr algorithms/morton_decode_2d_u32.rs:22-32 (compact stages)
#[must_use]
pub const fn morton_compact_2d(m: u64) -> u32 {
    let mut v = m & 0x5555_5555_5555_5555;
    v = (v | (v >> 1)) & 0x3333_3333_3333_3333;
    v = (v | (v >> 2)) & 0x0F0F_0F0F_0F0F_0F0F;
    v = (v | (v >> 4)) & 0x00FF_00FF_00FF_00FF;
    v = (v | (v >> 8)) & 0x0000_FFFF_0000_FFFF;
    v = (v | (v >> 16)) & 0x0000_0000_FFFF_FFFF;
    v as u32
}

/// Interleaves `(x, y)` into a 2-D Morton key: bit `i` of `x` at even
/// position `2*i`, bit `i` of `y` at odd position `2*i + 1`.
///
/// from bcinr algorithms/morton_encode_2d_u32.rs:22-41
#[must_use]
pub const fn morton_encode_2d(x: u32, y: u32) -> u64 {
    morton_spread_2d(x) | (morton_spread_2d(y) << 1)
}

/// Splits a 2-D Morton key back into `(x, y)`; inverse of
/// [`morton_encode_2d`].
///
/// from bcinr algorithms/morton_decode_2d_u32.rs:22-37
#[must_use]
pub const fn morton_decode_2d(m: u64) -> (u32, u32) {
    (morton_compact_2d(m), morton_compact_2d(m >> 1))
}

/// Spreads the low 21 bits of `x` to positions ≡ 0 (mod 3) of a `u64`
/// (bit `i` → bit `3*i`), the 3-D Morton *part* function (fixed 5-stage
/// mask-shift body; see [`morton_spread_2d`] for the const-composition
/// divergence note). Only bits 0..=20 are meaningful in a 64-bit 3-D key.
///
/// from bcinr algorithms/morton_encode_3d_u32.rs:22-30
#[must_use]
pub const fn morton_spread_3d(x: u32) -> u64 {
    let mut v = (x) as u64 & 0x001F_FFFF;
    v = (v | (v << 32)) & 0x001F_0000_0000_FFFF;
    v = (v | (v << 16)) & 0x001F_0000_FF00_00FF;
    v = (v | (v << 8)) & 0x100F_00F0_0F00_F00F;
    v = (v | (v << 4)) & 0x10C3_0C30_C30C_30C3;
    v = (v | (v << 2)) & 0x1249_2492_4924_9249;
    v
}

/// Compacts the bits of `m` at positions ≡ 0 (mod 3) back into the low 21
/// bits of a `u32`; inverse of [`morton_spread_3d`] on that lane.
///
/// from bcinr algorithms/morton_encode_3d_u32.rs:22-30 (stages reversed)
#[must_use]
pub const fn morton_compact_3d_lane(m: u64) -> u32 {
    let mut v = m & 0x1249_2492_4924_9249;
    v = (v | (v >> 2)) & 0x10C3_0C30_C30C_30C3;
    v = (v | (v >> 4)) & 0x100F_00F0_0F00_F00F;
    v = (v | (v >> 8)) & 0x001F_0000_FF00_00FF;
    v = (v | (v >> 16)) & 0x001F_0000_0000_FFFF;
    v = (v | (v >> 32)) & 0x001F_FFFF;
    v as u32
}

/// Interleaves `(x, y, z)` into a 3-D Morton key: bit `i` of each coordinate
/// at position `3*i + axis`. Only the low 21 bits of each coordinate land in
/// the 64-bit key.
///
/// from bcinr algorithms/morton_encode_3d_u32.rs:22-30
#[must_use]
pub const fn morton_encode_3d(x: u32, y: u32, z: u32) -> u64 {
    morton_spread_3d(x) | (morton_spread_3d(y) << 1) | (morton_spread_3d(z) << 2)
}

/// Splits a 3-D Morton key back into `(x, y, z)`; inverse of
/// [`morton_encode_3d`].
///
/// from bcinr algorithms/morton_encode_3d_u32.rs:22-30 (stages reversed)
#[must_use]
pub const fn morton_decode_3d(m: u64) -> (u32, u32, u32) {
    (
        morton_compact_3d_lane(m),
        morton_compact_3d_lane(m >> 1),
        morton_compact_3d_lane(m >> 2),
    )
}

/// Maps `(x, y)` on the `2^level × 2^level` Hilbert curve to its distance
/// `d` along the curve (the textbook `xy2d`, parameterized by `level`).
///
/// `level` is clamped to 0..=30 with the branchless [`crate::select::min_u32`]
/// (30 levels keeps `d` and the `s * s` weighting inside `u64`). Coordinates
/// are masked to the grid (`& (n - 1)`), and iterations at scales `s >= n`
/// are gated to no-ops with an `s`-presence mask (`0u64.wrapping_sub((s <
/// n) as u64)`), so the loop is a fixed 32 steps (`s` from `1 << 31` down to
/// `1`) regardless of `level`. `level == 0` therefore returns 0 for every
/// input.
///
/// DIVERGENCE from bcinr: the source is a 16-level full unroll over a
/// 16-bit grid (`hilbert_curve_encode_u32.rs`); this is the same quadrant
/// rule derived as a level-parameterized fixed 32-step loop with presence
/// masks, so arbitrary grid orders up to 30 work at the cost of the unroll.
///
/// from bcinr algorithms/hilbert_curve_encode_u32.rs:24-58
#[must_use]
pub const fn hilbert_encode_2d(x: u32, y: u32, level: u8) -> u64 {
    let level = crate::select::min_u32((level) as u32, 30);
    let n = 1u64 << level;
    let side = n.wrapping_sub(1);
    let mut x = (x) as u64 & side;
    let mut y = (y) as u64 & side;
    let mut d = 0u64;
    let mut step = 32u32;
    while step > 0 {
        let s = 1u64 << (step - 1);
        step -= 1;
        // Iterations at scales s >= n are no-ops (level-presence gate).
        let active = 0u64.wrapping_sub((s < n) as u64);
        let rx = ((x & s) != 0) as u64;
        let ry = ((y & s) != 0) as u64;
        let quad = ((3 * rx) ^ ry) & active;
        d = d.wrapping_add(s.wrapping_mul(s).wrapping_mul(quad));
        // ry == 0: swap; additionally reflect when rx == 1. Both isometries
        // are involutions and commute, so T ∘ T = identity.
        let ry0 = ry.wrapping_sub(1); // all-ones iff ry == 0
        let reflect = ry0 & 0u64.wrapping_sub(rx) & active;
        let swap = ry0 & active;
        let nx = (s.wrapping_sub(1).wrapping_sub(x) & reflect) | (x & !reflect);
        let ny = (s.wrapping_sub(1).wrapping_sub(y) & reflect) | (y & !reflect);
        x = (ny & swap) | (nx & !swap);
        y = (nx & swap) | (ny & !swap);
    }
    d
}

/// Maps a Hilbert distance `d < 4^level` back to its `(x, y)` point on the
/// `2^level × 2^level` curve (the textbook `d2xy`); inverse of
/// [`hilbert_encode_2d`]. `d` outside `0..4^level` yields an unspecified
/// point.
///
/// Consumes the curve digits MSB-first: `t` is aligned so the topmost digit
/// sits at bits 63..62 and shifts left twice per step; each step applies the
/// inverse quadrant isometry `T_s` (swap, plus reflect when `ry == 0 &&
/// rx == 1` — an involution, hence its own inverse) to the accumulated
/// point, then adds `s * (rx, ry)`. Level clamp and presence gating match
/// [`hilbert_encode_2d`] (`level == 0` decodes everything to `(0, 0)`).
///
/// DIVERGENCE from bcinr: the source decodes LSB-first over a fixed 16-level
/// unroll (`hilbert_curve_decode_u32.rs`); MSB-first consumption is required
/// here so the level parameter can clamp the digit stream — the quadrant
/// masks are the source's verbatim.
///
/// from bcinr algorithms/hilbert_curve_decode_u32.rs:24-50
#[must_use]
pub const fn hilbert_decode_2d(d: u64, level: u8) -> (u32, u32) {
    let level = crate::select::min_u32((level) as u32, 30);
    let n = 1u64 << level;
    let mut t = d;
    let mut x = 0u64;
    let mut y = 0u64;
    let mut s = 1u64;
    while s < n {
        let rx = (t >> 1) & 1;
        let ry = (t ^ rx) & 1;
        let ry0 = ry.wrapping_sub(1); // all-ones iff ry == 0
        let reflect = ry0 & 0u64.wrapping_sub(rx);
        let swap = ry0;
        let nx = (s.wrapping_sub(1).wrapping_sub(x) & reflect) | (x & !reflect);
        let ny = (s.wrapping_sub(1).wrapping_sub(y) & reflect) | (y & !reflect);
        x = ((ny & swap) | (nx & !swap)).wrapping_add(s.wrapping_mul(rx));
        y = ((nx & swap) | (ny & !swap)).wrapping_add(s.wrapping_mul(ry));
        t >>= 2;
        s <<= 1;
    }
    (x as u32, y as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    // no_std crate: the std-dependent branchy oracles live only in tests.
    extern crate std;

    /// Bit-by-bit reference: set low `k` bits of `value` at `offset`.
    fn field_deposit_ref(word: u64, value: u64, k: u32, offset: u32) -> u64 {
        let mut out = word;
        for i in 0..k.min(32) {
            let bit = (value >> i) & 1;
            out = (out & !(1 << (offset + i))) | (bit << (offset + i));
        }
        out
    }

    fn elias_fano_ref(value: u64, w: u32) -> u64 {
        let w = w & 63;
        let lo = value & ((1u64 << w).wrapping_sub(1));
        let hi = (value >> w).min((63 - w) as u64);
        let mut out = lo;
        out |= 1u64 << ((w) as u64 + hi);
        out
    }

    fn transpose8x8_ref(word: u64) -> u64 {
        let mut res = 0u64;
        for i in 0..8 {
            for j in 0..8 {
                if (word >> (i * 8 + j)) & 1 != 0 {
                    res |= 1 << (j * 8 + i);
                }
            }
        }
        res
    }

    fn compress_ref(value: u64, mask: u64) -> u64 {
        let mut res = 0u64;
        let mut r = 0u32;
        for i in 0..64 {
            if (mask >> i) & 1 == 1 {
                res |= ((value >> i) & 1) << r;
                r += 1;
            }
        }
        res
    }

    fn deposit_ref(value: u64, mask: u64) -> u64 {
        let mut res = 0u64;
        let mut v = 0u32;
        for i in 0..64 {
            if (mask >> i) & 1 == 1 {
                res |= ((value >> v) & 1) << i;
                v += 1;
            }
        }
        res
    }

    fn delta_swap_ref(word: u64, mask: u64, shift: u32) -> u64 {
        // Independent derivation (bcinr reference form): spell out the
        // exchange with named temporaries and two explicit XORs rather than
        // the fused single-expression impl. `mask` marks the lower bit of
        // each exchanged pair (pair bases may overlap; the fused form is the
        // defined semantics).
        let shift = shift & 63;
        let lo = word & mask;
        let hi = (word >> shift) & mask;
        let diff = lo ^ hi;
        let mut r = word ^ diff;
        r ^= diff << shift;
        r
    }

    #[test]
    fn field_deposit_extract_roundtrip() {
        for k in 1u32..=32 {
            for offset in [0u32, 1, 15, 31] {
                for value in [0u64, 1, u64::MAX, 0xDEAD_BEEF] {
                    let word = 0x1234_5678_9ABC_DEF0;
                    let enc = field_deposit(word, value, k, offset);
                    assert_eq!(enc, field_deposit_ref(word, value, k, offset));
                    assert_eq!(field_extract(enc, k, offset), value & mask_low(k));
                }
            }
        }
    }

    #[test]
    fn elias_fano_split_matches_reference() {
        for w in [0u32, 1, 7, 16, 32, 63] {
            for value in [0u64, 1, 0xFFFF_FFFF_FFFF_FFFF, 0x0123_4567_89AB_CDEF] {
                assert_eq!(elias_fano_split(value, w), elias_fano_ref(value, w));
            }
        }
    }

    #[test]
    fn delta_lanes_roundtrip() {
        for value in [0u64, 1, u64::MAX, 0xA | (0x14 << 32)] {
            for prev in [0u64, 5 | (3 << 32), u64::MAX] {
                let enc = delta_encode_u32x2(value, prev);
                assert_eq!(delta_decode_u32x2(enc, prev), value);
                assert_eq!(enc as u32, (value as u32).wrapping_sub(prev as u32));
            }
        }
    }

    #[test]
    fn transposes_match_bit_reference() {
        for word in [
            0u64,
            1,
            u64::MAX,
            0x0F0F_0F0F_F0F0_F0F0,
            0x8000_0000_0000_0001,
        ] {
            assert_eq!(transpose8x8(word), transpose8x8_ref(word));
            // transpose is an involution
            assert_eq!(transpose8x8(transpose8x8(word)), word);
        }
        assert_eq!(
            transpose64x64_combined(0xA5A5_A5A5_5A5A_5A5A, 0),
            transpose8x8(0xA5A5_A5A5_5A5A_5A5A)
        );
    }

    #[test]
    fn delta_swap_matches_reference_and_involutes() {
        for mask in [0u64, 1, u64::MAX, 0xAAAA_AAAA_AAAA_AAAA] {
            for shift in [0u32, 1, 7, 31, 63] {
                for word in [0u64, 1, u64::MAX] {
                    let got = delta_swap(word, mask, shift);
                    assert_eq!(got, delta_swap_ref(word, mask, shift));
                }
            }
        }
        // shift=0: identity (pairs exchange with themselves).
        assert_eq!(delta_swap(0xDEAD_BEEF, u64::MAX, 0), 0xDEAD_BEEF);
    }

    #[test]
    fn bit_swap_exchanges_selected_bits() {
        for selector in [0u64, 1, 63, 64, 0x3F | (0x3F << 6), 5] {
            for word in [0u64, 1, u64::MAX, 0xF0F0_F0F0_F0F0_F0F0] {
                let i = (selector & 63) as u32;
                let j = ((selector >> 6) & 63) as u32;
                let got = bit_swap(word, selector);
                let bi = (word >> i) & 1;
                let bj = (word >> j) & 1;
                let want = (word & !(1 << i) & !(1 << j)) | (bi << j) | (bj << i);
                assert_eq!(got, want);
            }
        }
    }

    #[test]
    fn permute_step8_is_fixed_delta_swap() {
        for mask in [0u64, u64::MAX, 0x00FF_00FF_00FF_00FF] {
            for word in [0u64, 1, u64::MAX] {
                assert_eq!(permute_step8(word, mask), delta_swap(word, mask, 8));
            }
        }
    }

    #[test]
    fn compress_deposit_roundtrip_and_oracles() {
        for mask in [0u64, 1, u64::MAX, 0b1011_0101_1100_0011] {
            for value in [0u64, 1, u64::MAX, 0x0123_4567_89AB_CDEF] {
                let c = compress_bits(value, mask);
                assert_eq!(c, compress_ref(value, mask));
                let d = deposit_bits(value, mask);
                assert_eq!(d, deposit_ref(value, mask));
                // Round trip: deposit of the compressed bits restores the
                // selected positions.
                assert_eq!(deposit_bits(compress_bits(value, mask), mask), value & mask);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Straddling fields (OPT-63 T-C)
    // -----------------------------------------------------------------------

    /// Scalar bit-by-bit oracle for [`field_extract2`].
    fn field_extract2_ref(lo: u64, hi: u64, k: u32, offset: u32) -> u64 {
        let mut out = 0u64;
        for i in 0..k {
            let bit_pos = offset + i;
            let bit = if bit_pos < 64 {
                (lo >> bit_pos) & 1
            } else if bit_pos < 128 {
                (hi >> (bit_pos - 64)) & 1
            } else {
                0
            };
            out |= bit << i;
        }
        out
    }

    /// Scalar bit-by-bit oracle for [`field_deposit2`].
    fn field_deposit2_ref(lo: u64, hi: u64, value: u64, k: u32, offset: u32) -> (u64, u64) {
        let mut lo = lo;
        let mut hi = hi;
        for i in 0..k {
            let bit_pos = offset + i;
            let bit = (value >> i) & 1;
            if bit_pos < 64 {
                lo = (lo & !(1u64 << bit_pos)) | (bit << bit_pos);
            } else if bit_pos < 128 {
                let p = bit_pos - 64;
                hi = (hi & !(1u64 << p)) | (bit << p);
            }
        }
        (lo, hi)
    }

    #[test]
    fn field2_roundtrip_all_offsets() {
        for offset in 0u32..=63 {
            for k in [1u32, 31, 32, 33, 63, 64] {
                for value in [0u64, 1, u64::MAX, 0xDEAD_BEEF_CAFE_F00D] {
                    let lo = 0x1234_5678_9ABC_DEF0;
                    let hi = 0x0FED_CBA9_8765_4321;
                    let (elo, ehi) = field_deposit2(lo, hi, value, k, offset);
                    // Deposit preserves every bit outside the field and
                    // writes exactly the masked value inside it.
                    let mask = u64::MAX >> (64u32.wrapping_sub(k) & 63);
                    let field_mask = ((mask) as u128) << offset;
                    let kept = ((((hi) as u128) << 64) | (lo) as u128) & !field_mask;
                    let got_pair = (((ehi) as u128) << 64) | (elo) as u128;
                    assert_eq!(got_pair & !field_mask, kept, "outside bits changed");
                    assert_eq!(
                        got_pair & field_mask,
                        ((value & mask) as u128) << offset,
                        "field bits wrong"
                    );
                    // Roundtrip.
                    assert_eq!(field_extract2(elo, ehi, k, offset), value & mask);
                    // Oracles.
                    assert_eq!((elo, ehi), field_deposit2_ref(lo, hi, value, k, offset));
                    assert_eq!(
                        field_extract2(elo, ehi, k, offset),
                        field_extract2_ref(elo, ehi, k, offset)
                    );
                }
            }
        }
    }

    #[test]
    fn field2_k64_never_clamped_all_offsets() {
        // k == 64 must take the full-width mask path (no clamp to 32/63) at
        // every offset, including offset 0 and 63 (both boundary holes).
        for offset in 0u32..=63 {
            let v = 0xDEAD_BEEF_CAFE_F00Du64;
            let (lo, hi) = field_deposit2(!0u64, !0u64, v, 64, offset);
            assert_eq!(field_extract2(lo, hi, 64, offset), v);
            assert_eq!(field_extract2(!0u64, !0u64, 64, offset), u64::MAX);
            // Extraction straddles at offset 63: bit 63 of lo + top 63..1? —
            // exactly the low 64 bits of (hi:lo) >> 63.
            let window = ((((hi) as u128) << 64) | (lo) as u128) >> offset;
            assert_eq!(field_extract2(lo, hi, 64, offset), window as u64);
        }
    }

    #[test]
    fn field2_adjacent_boundaries_match_oracle() {
        // k=33 @ offset=31 straddles the 32-bit middle; k=1 @ offset=63
        // straddles the lo/hi seam with a single bit.
        for (k, offset) in [(33u32, 31u32), (1, 63), (64, 1), (2, 63)] {
            let lo = 0xDEAD_BEEF_CAFE_F00D;
            let hi = 0xBAAD_F00D_D15E_A5E5;
            let v = 0x1234_5678_9ABC_DEF0;
            let (elo, ehi) = field_deposit2(lo, hi, v, k, offset);
            assert_eq!((elo, ehi), field_deposit2_ref(lo, hi, v, k, offset));
            assert_eq!(
                field_extract2(elo, ehi, k, offset),
                v & (u64::MAX >> (64u32.wrapping_sub(k) & 63))
            );
            assert_eq!(
                field_extract2(lo, hi, k, offset),
                field_extract2_ref(lo, hi, k, offset)
            );
        }
    }

    // -----------------------------------------------------------------------
    // Morton keys (OPT-53)
    // -----------------------------------------------------------------------

    fn morton_spread_2d_ref(x: u32) -> u64 {
        let mut r = 0u64;
        let mut i = 0u32;
        while i < 32 {
            r |= (((x >> i) & 1) as u64) << (2 * i);
            i += 1;
        }
        r
    }

    fn morton_encode_2d_ref(x: u32, y: u32) -> u64 {
        let mut r = 0u64;
        let mut i = 0u32;
        while i < 32 {
            r |= (((x >> i) & 1) as u64) << (2 * i);
            r |= (((y >> i) & 1) as u64) << (2 * i + 1);
            i += 1;
        }
        r
    }

    fn morton_spread_3d_ref(x: u32) -> u64 {
        let mut r = 0u64;
        let mut i = 0u32;
        while i < 21 {
            r |= (((x >> i) & 1) as u64) << (3 * i);
            i += 1;
        }
        r
    }

    fn morton_encode_3d_ref(x: u32, y: u32, z: u32) -> u64 {
        let mut r = 0u64;
        let mut i = 0u32;
        while i < 21 {
            r |= (((x >> i) & 1) as u64) << (3 * i);
            r |= (((y >> i) & 1) as u64) << (3 * i + 1);
            r |= (((z >> i) & 1) as u64) << (3 * i + 2);
            i += 1;
        }
        r
    }

    #[test]
    fn morton_2d_matches_bit_oracle_and_roundtrips() {
        for x in [
            0u32,
            1,
            2,
            0xFFFF_FFFF,
            0xA5A5_5A5A,
            0x8000_0000,
            0x0000_FFFF,
        ] {
            for y in [0u32, 1, 0xFFFF_FFFF, 0x0F0F_0F0F, 7] {
                assert_eq!(morton_spread_2d(x), morton_spread_2d_ref(x));
                let m = morton_encode_2d(x, y);
                assert_eq!(m, morton_encode_2d_ref(x, y));
                assert_eq!(morton_decode_2d(m), (x, y));
                // Lane separation: y lives in the odd positions only.
                assert_eq!(morton_encode_2d(0, y), morton_spread_2d(y) << 1);
            }
        }
    }

    #[test]
    fn morton_3d_matches_bit_oracle_and_roundtrips() {
        for x in [0u32, 1, 0x1F_FFFF, 0x15_5555, 0x0010_0000] {
            for y in [0u32, 1, 0x1F_FFFF, 0x0A_2A2A] {
                for z in [0u32, 1, 0x1F_FFFF, 0x3] {
                    assert_eq!(morton_spread_3d(x), morton_spread_3d_ref(x));
                    let m = morton_encode_3d(x, y, z);
                    assert_eq!(m, morton_encode_3d_ref(x, y, z));
                    assert_eq!(morton_decode_3d(m), (x, y, z));
                }
            }
        }
    }

    #[test]
    fn morton_composed_over_compress_deposit_kernels() {
        // The spread is exactly a pdep onto the even-lane mask, and the
        // compact exactly a pext of it — proving the mask-shift bodies and
        // the compress/deposit kernels compute the same permutation.
        const EVEN: u64 = 0x5555_5555_5555_5555;
        for x in [0u32, 1, 0xFFFF_FFFF, 0xDEAD_BEEF, 0x0102_0304] {
            assert_eq!(morton_spread_2d(x), deposit_bits((x) as u64, EVEN));
            let m = morton_encode_2d(x, 0x0F0F_0F0F);
            assert_eq!(u64::from(morton_compact_2d(m)), compress_bits(m, EVEN));
            assert_eq!((morton_compact_2d(m)) as u64, compress_ref(m, EVEN));
        }
    }

    // -----------------------------------------------------------------------
    // Hilbert curve (OPT-53)
    // -----------------------------------------------------------------------

    /// Textbook branchy Wikipedia `xy2d`, parameterized by grid order.
    fn hilbert_xy2d_ref(xi: i64, yi: i64, level: u8) -> u64 {
        let n: i64 = 1i64 << level;
        let mut x = xi;
        let mut y = yi;
        let mut d = 0i64;
        let mut s = n / 2;
        while s > 0 {
            let rx = i64::from((x & s) > 0);
            let ry = i64::from((y & s) > 0);
            d += s * s * ((3 * rx) ^ ry);
            if ry == 0 {
                if rx == 1 {
                    x = s - 1 - x;
                    y = s - 1 - y;
                }
                std::mem::swap(&mut x, &mut y);
            }
            s /= 2;
        }
        d as u64
    }

    /// Textbook branchy Wikipedia `d2xy`, parameterized by grid order.
    fn hilbert_d2xy_ref(d: u64, level: u8) -> (u32, u32) {
        let n: i64 = 1i64 << level;
        let mut t = d as i64;
        let mut x = 0i64;
        let mut y = 0i64;
        let mut s = 1i64;
        while s < n {
            let rx = 1 & (t / 2);
            let ry = 1 & (t ^ rx);
            if ry == 0 {
                if rx == 1 {
                    x = s - 1 - x;
                    y = s - 1 - y;
                }
                std::mem::swap(&mut x, &mut y);
            }
            x += s * rx;
            y += s * ry;
            t /= 4;
            s *= 2;
        }
        (x as u32, y as u32)
    }

    #[test]
    fn hilbert_roundtrip_small_levels_exhaustive() {
        for level in 1u8..=6 {
            let n = 1u32 << level;
            for x in 0..n {
                for y in 0..n {
                    let d = hilbert_encode_2d(x, y, level);
                    assert_eq!(
                        d,
                        hilbert_xy2d_ref(i64::from(x), i64::from(y), level),
                        "enc ({x},{y}) L{level}"
                    );
                    assert_eq!(
                        hilbert_decode_2d(d, level),
                        (x, y),
                        "roundtrip ({x},{y}) L{level}"
                    );
                    assert_eq!(
                        hilbert_decode_2d(d, level),
                        hilbert_d2xy_ref(d, level),
                        "dec vs wiki ({x},{y}) L{level}"
                    );
                }
            }
        }
    }

    #[test]
    fn hilbert_roundtrip_corners_and_sampled_high_levels() {
        for level in 7u8..=16 {
            let n = 1u32 << level;
            let side = n - 1;
            let mut pts = [(0u32, 0u32); 69];
            pts[0] = (0, 0);
            pts[1] = (side, 0);
            pts[2] = (0, side);
            pts[3] = (side, side);
            pts[4] = (n / 2, n / 2);
            // Deterministic LCG sample.
            let mut state = 0x2545_F491_4F6C_DD1Du64;
            let mut i = 5;
            while i < 69 {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                pts[i] = ((state >> 33) as u32 & side, (state >> 20) as u32 & side);
                i += 1;
            }
            for &(x, y) in pts.iter() {
                let d = hilbert_encode_2d(x, y, level);
                assert_eq!(d, hilbert_xy2d_ref(i64::from(x), i64::from(y), level));
                assert_eq!(hilbert_decode_2d(d, level), (x, y));
                assert_eq!(hilbert_d2xy_ref(d, level), (x, y));
            }
        }
    }

    #[test]
    fn hilbert_contract_pins() {
        // level 0 degenerates to a single cell: everything maps to 0 / (0,0).
        assert_eq!(hilbert_encode_2d(0, 0, 0), 0);
        assert_eq!(hilbert_encode_2d(5, 7, 0), 0);
        assert_eq!(hilbert_decode_2d(0, 0), (0, 0));
        assert_eq!(hilbert_decode_2d(u64::MAX, 0), (0, 0));
        // level > 30 clamps to 30 via select::min (same result as L30).
        assert_eq!(hilbert_encode_2d(3, 5, 255), hilbert_encode_2d(3, 5, 30));
        assert_eq!(
            hilbert_decode_2d(0x1234_5678, 200),
            hilbert_decode_2d(0x1234_5678, 30)
        );
        // Origin maps to 0 at every level.
        for level in 1u8..=30 {
            assert_eq!(hilbert_encode_2d(0, 0, level), 0);
        }
        // Level-1 orientation pins (matches the branchy wiki oracle): the
        // curve opens along +y from the origin.
        assert_eq!(hilbert_encode_2d(0, 1, 1), 1);
        assert_eq!(hilbert_encode_2d(1, 1, 1), 2);
        assert_eq!(hilbert_encode_2d(1, 0, 1), 3);
        assert_eq!(hilbert_decode_2d(1, 1), (0, 1));
        assert_eq!(hilbert_decode_2d(3, 1), (1, 0));
        // Mid-curve spot check on the 4x4 grid.
        assert_eq!(hilbert_encode_2d(2, 1, 2), 13);
        assert_eq!(hilbert_decode_2d(13, 2), (2, 1));
    }
}

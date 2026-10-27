//! SWAR (SIMD Within A Register) byte-scan and word kernels.
//!
//! A `u64` is treated as 8 parallel byte lanes; every primitive here is pure
//! integer arithmetic with no data-dependent branches, no heap, and no
//! ambient state. Slice forms process 8-byte chunks with a scalar tail.
//!
//! Provenance: ported from `bcinr` (`crates/bcinr-logic/src/scan.rs`,
//! `src/swar_str.rs`, `src/algorithms/{simd_memchr_u8x16,find_first_of_branchless}.rs`),
//! branch `bench/rdtsc-tick-tables` @ `1e92b213`. Scaffold ABIs stripped;
//! `parse_4_hex_digits` hardened to true SWAR (the bcinr original was a
//! scalar per-byte loop).

/// Broadcast mask: lowest bit of every byte lane.
const ONES: u64 = 0x0101_0101_0101_0101;
/// Broadcast mask: bit 7 (MSB) of every byte lane.
const HIGHS: u64 = 0x8080_8080_8080_8080;
/// Broadcast mask: bits 0..=6 of every byte lane (cascade-safe form).
const LO7: u64 = 0x7F7F_7F7F_7F7F_7F7F;

/// Mask with `0x80` set in each byte lane where `lo <= byte <= hi`.
///
/// Hacker's Delight range check: a lane is in range iff neither the
/// `word - lo` nor the `(hi+1) - word` subtraction borrows into bit 7.
///
/// from bcinr swar_str.rs:44-73 (`swar_is_in_range`, private there).
#[inline]
#[must_use]
pub fn byte_lane_in_range(word: u64, lo: u8, hi: u8) -> u64 {
    // Borrow-free per-lane >= (Hacker's Delight 6-1): the minuend lanes are
    // pre-set to >= 0x80 and the subtrahend lanes masked to <= 0x7F, so no
    // subtraction can borrow out of its lane (the naive
    // `!(word-lo | hi+1-word) & HIGHS` form — also in the bcinr original —
    // corrupts lane i+1 whenever lane i borrows, e.g. "1A2f" where '1' < 'A').
    debug_assert!(hi <= 0x7F, "range endpoints must be ASCII (MSB clear)");
    let lo_b = ((lo) as u64) * ONES;
    let hi_p1 = ((hi) as u64).wrapping_add(1) * ONES;
    let ge_lo = ((word | HIGHS).wrapping_sub(lo_b)) & HIGHS;
    // `over` = w >= hi+1; membership is the negation (checks `w <= hi`
    // exactly, including the w == hi+1 boundary that an inclusive-ge form
    // misclassifies — ':' vs '9').
    let over = ((word | HIGHS).wrapping_sub(hi_p1 & LO7)) & HIGHS;
    let msb_clear = !word & HIGHS;
    ge_lo & !over & msb_clear
}

/// Mask with `0x80` set in every byte lane where `word` holds `byte`.
///
/// from bcinr swar_str.rs:112-117 (`find_byte_in_word`); `const` added so
/// const callers ([`horizontal_count_eq_u8`]) can delegate to this one body.
#[inline]
#[must_use]
pub const fn find_byte_in_word(word: u64, byte: u8) -> u64 {
    let xored = word ^ (((byte) as u64) * ONES);
    xored.wrapping_sub(ONES) & !xored & HIGHS
}

/// Number of lanes (0-8) in `word` equal to `byte`.
///
/// from bcinr swar_str.rs:140-144 (`count_byte_in_word`).
#[inline]
#[must_use]
pub fn count_byte_in_word(word: u64, byte: u8) -> u32 {
    find_byte_in_word(word, byte).count_ones()
}

/// Whether `byte` appears anywhere in `word`.
///
/// from bcinr swar_str.rs:165-169 (`has_byte_in_word`).
#[inline]
#[must_use]
pub fn has_byte_in_word(word: u64, byte: u8) -> bool {
    find_byte_in_word(word, byte) != 0
}

/// Byte index (0-7) of the first lane equal to `byte`, or [`None`].
///
/// from bcinr swar_str.rs:196-210 (`first_byte_position`).
#[inline]
#[must_use]
pub fn first_byte_position(word: u64, byte: u8) -> Option<u32> {
    let mask = find_byte_in_word(word, byte);
    if mask == 0 {
        None
    } else {
        Some(mask.trailing_zeros() / 8)
    }
}

/// Lowercase ASCII letters (`A`-`Z` → `a`-`z`) across all 8 lanes.
///
/// Sets bit 5 only on lanes the range check flags as uppercase.
/// from bcinr swar_str.rs:240-245 (`to_lower_ascii_word`).
#[inline]
#[must_use]
pub fn to_lower_ascii_word(word: u64) -> u64 {
    word | (byte_lane_in_range(word, b'A', b'Z') >> 2)
}

/// Uppercase ASCII letters (`a`-`z` → `A`-`Z`) across all 8 lanes.
///
/// from bcinr swar_str.rs:268-273 (`to_upper_ascii_word`).
#[inline]
#[must_use]
pub fn to_upper_ascii_word(word: u64) -> u64 {
    word & !(byte_lane_in_range(word, b'a', b'z') >> 2)
}

/// Per-lane classification bitfield (one byte per lane).
///
/// Lane bits: `0x01` digit, `0x02` alpha, `0x04` whitespace
/// (`0x09`-`0x0D` or `0x20`), `0x08` uppercase, `0x10` lowercase.
/// from bcinr swar_str.rs:365-397 (`swar_classify_bytes`).
#[inline]
#[must_use]
pub fn classify_bytes(word: u64) -> u64 {
    let is_digit = byte_lane_in_range(word, b'0', b'9');
    let is_upper = byte_lane_in_range(word, b'A', b'Z');
    let is_lower = byte_lane_in_range(word, b'a', b'z');
    let is_ws = byte_lane_in_range(word, b'\t', b'\r') | byte_lane_in_range(word, b' ', b' ');
    let is_alpha = is_upper | is_lower;
    (is_digit >> 7)
        | ((is_alpha >> 7) << 1)
        | ((is_ws >> 7) << 2)
        | ((is_upper >> 7) << 3)
        | ((is_lower >> 7) << 4)
}

/// Parses exactly 8 ASCII decimal digits from `word`, MSB-first.
///
/// Returns [`None`] if any lane is outside `'0'`..=`'9'`.
/// from bcinr swar_str.rs:409-506 (`parse_8_decimal_digits`).
#[inline]
#[must_use]
pub fn parse_8_decimal_digits(word: u64) -> Option<u32> {
    let digits = word.wrapping_sub(0x3030_3030_3030_3030);
    // A lane overflows bit 7 iff its digit value was >= 10, or the original
    // lane was < '0' (the subtraction wrapped).
    let overflow = digits | digits.wrapping_add(0x7676_7676_7676_7676);
    if overflow & HIGHS != 0 {
        return None;
    }
    // Combine byte pairs → 16-bit lanes → 32-bit halves, big-endian numeric
    // order (lane 0 is the most significant digit).
    let t1 = (digits & 0x00FF_00FF_00FF_00FF)
        .wrapping_mul(10)
        .wrapping_add((digits >> 8) & 0x00FF_00FF_00FF_00FF);
    let t2 = (t1 & 0x0000_FFFF_0000_FFFF)
        .wrapping_mul(100)
        .wrapping_add((t1 >> 16) & 0x0000_FFFF_0000_FFFF);
    let lo32 = (t2 & 0xFFFF_FFFF) as u32;
    let hi32 = ((t2 >> 32) & 0xFFFF_FFFF) as u32;
    Some(lo32.wrapping_mul(10_000).wrapping_add(hi32))
}

/// Parses exactly 4 ASCII hex digits from `word` (little-endian lanes),
/// MSB-first numeric order.
///
/// True SWAR (hardened from bcinr swar_str.rs:524-566, which used a scalar
/// per-byte loop): per-lane classify via range masks, nibble select via
/// mask arithmetic, single validity flag.
#[inline]
#[must_use]
pub fn parse_4_hex_digits(word: u32) -> Option<u32> {
    let w = (word) as u64;
    // Hex digit ranges per lane: '0'-'9' → 0-9; 'A'-'F' and 'a'-'f' → 10-15.
    // Per-lane fixups (bcinr's original is a scalar 4-byte loop with
    // big-endian numeric order; this form is fully lane-parallel — the
    // previous port masked with 0x00FF_00FF.. which dropped the odd byte
    // positions where the letter fixups land).
    let is_upper = byte_lane_in_range(w, b'A', b'F');
    let is_lower = byte_lane_in_range(w, b'a', b'f');
    let is_digit = byte_lane_in_range(w, b'0', b'9');
    let all_hex = is_upper | is_lower | is_digit;
    // All four byte lanes (positions 0..3) must be hex digits. Lanes 4..7 of
    // `w` are zero and can never match a digit range, so checking only the
    // low four high-bits is exact.
    const LOW4_HIGHS: u64 = HIGHS >> 32; // 0x0000_0000_8080_8080
    if all_hex & LOW4_HIGHS != LOW4_HIGHS {
        return None;
    }
    // Per-lane digit value: b - '0', minus 7 when uppercase, minus 0x27
    // when lowercase ('0/1' range masks shift to 0/1 per lane, so the
    // scalar multiplies stay lane-local — no cross-lane carries).
    let up = is_upper >> 7;
    let dn = is_lower >> 7;
    let value = w
        .wrapping_sub(0x30u64 * ONES)
        .wrapping_sub(up.wrapping_mul(7))
        .wrapping_sub(dn.wrapping_mul(0x27));
    // Nibble fit: each of lanes 0..3 must be ≤ 15 (lane + 0x70 keeps the
    // high bit clear exactly when the lane holds 0..=15).
    let nibble_fit = (value | value.wrapping_add(0x70u64 * ONES)) & LOW4_HIGHS;
    if nibble_fit != 0 {
        return None;
    }
    // Big-endian numeric order: lane 0 is the most significant nibble.
    Some(
        (((value & 0xFF) as u32) << 12)
            | ((((value >> 8) & 0xFF) as u32) << 8)
            | ((((value >> 16) & 0xFF) as u32) << 4)
            | (((value >> 24) & 0xFF) as u32),
    )
}

/// Cascade-safe per-byte match mask: `0x80` in each lane of `word` equal to
/// the needle byte.
///
/// Avoids the borrow cross-talk of the subtract form on adjacent matches.
/// from bcinr algorithms/simd_memchr_u8x16.rs:32-39 (`simd_memchr_u8x16`),
/// scaffold ABI removed; from bcinr algorithms/find_first_of_branchless.rs:33-43.
#[inline]
#[must_use]
pub fn find_byte_mask_word(word: u64, byte: u8) -> u64 {
    let x = word ^ (((byte) as u64) * ONES);
    !(((x & LO7).wrapping_add(LO7) | x) & HIGHS) & HIGHS
}

/// Byte index (0-7) of the first lane equal to `byte`; 8 on no match.
///
/// from bcinr algorithms/find_first_of_branchless.rs:33-43
/// (`find_first_of_branchless`), scaffold ABI removed.
#[inline]
#[must_use]
pub fn find_first_of_word(word: u64, byte: u8) -> u64 {
    (find_byte_mask_word(word, byte).trailing_zeros() >> 3) as u64
}

/// Bit `i` of the result set iff `bytes[i] == target` (first 64 bytes only).
///
/// from bcinr scan.rs:102-118 (`find_byte_mask`).
#[inline]
#[must_use]
pub fn find_byte_mask(bytes: &[u8], target: u8) -> u64 {
    let len = bytes.len().min(64);
    let mut mask = 0u64;
    for (i, &b) in bytes[..len].iter().enumerate() {
        mask |= ((b == target) as u64) << i;
    }
    mask
}

/// Per-8-byte-chunk match bitmask for `target` (first 64 bytes only).
///
/// Chunk `w` contributes its 8 lane bits at bit offset `8*w`. This is the
/// Mycroft kernel with the chunk sweep wrapper added (bcinr shipped the
/// word form inline inside `swar_find_all_positions`).
/// from bcinr scan.rs:390-440 (`swar_find_all_positions`).
#[inline]
#[must_use]
pub fn swar_find_all_positions(bytes: &[u8], target: u8) -> u64 {
    let bytes = &bytes[..bytes.len().min(64)];
    let (chunks, remainder) = bytes.as_chunks::<8>();
    let mut result = 0u64;
    for (w, chunk) in chunks.iter().enumerate() {
        let word = u64::from_le_bytes(*chunk);
        // Pack one bit per matched lane: extract each lane's 0x80 bit.
        let lanes = find_byte_in_word(word, target);
        let packed = ((lanes >> 7) & 1)
            | ((lanes >> 15) & 1) << 1
            | ((lanes >> 23) & 1) << 2
            | ((lanes >> 31) & 1) << 3
            | ((lanes >> 39) & 1) << 4
            | ((lanes >> 47) & 1) << 5
            | ((lanes >> 55) & 1) << 6
            | ((lanes >> 63) & 1) << 7;
        result |= packed << (w * 8);
    }
    let base = chunks.len() * 8;
    for (i, &b) in remainder.iter().enumerate() {
        result |= ((b == target) as u64) << (base + i);
    }
    result
}

/// Length of the longest prefix of `bytes` where every byte equals `target`.
///
/// from bcinr scan.rs:459-490 (`count_leading_eq_u8`).
#[inline]
#[must_use]
pub fn count_leading_eq_u8(bytes: &[u8], target: u8) -> usize {
    let (chunks, remainder) = bytes.as_chunks::<8>();
    let mut count = 0usize;
    let mut done = 0usize;
    for chunk in chunks {
        let word = u64::from_le_bytes(*chunk);
        let all_match = usize::from(find_byte_in_word(word, target) == HIGHS);
        count += 8 * all_match * (1 - done);
        done |= 1 - all_match;
    }
    for &b in remainder {
        let eq = usize::from(b == target);
        count += eq * (1 - done);
        done |= 1 - eq;
    }
    count
}

/// Whether every byte in `bytes` has bit 7 clear (7-bit ASCII).
///
/// from bcinr scan.rs:161-186 (`is_ascii_u64_slice`) /
/// swar_str.rs:323-347 (`is_all_ascii`).
#[inline]
#[must_use]
pub fn is_all_ascii(bytes: &[u8]) -> bool {
    let mut acc = 0u64;
    let (chunks, remainder) = bytes.as_chunks::<8>();
    for chunk in chunks {
        acc |= u64::from_le_bytes(*chunk);
    }
    for &b in remainder {
        acc |= (b) as u64;
    }
    acc & HIGHS == 0
}

/// Number of consecutive ASCII spaces at the start of `bytes`.
///
/// The offset accumulates only while the current position equals the running
/// count, so the scan stops at the first non-space without a branch.
/// from bcinr scan.rs:133-146 (`skip_spaces`).
#[inline]
#[must_use]
pub fn skip_spaces(bytes: &[u8]) -> usize {
    let mut offset = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        let is_space = usize::from(b == b' ');
        let mask = usize::from(offset == i);
        offset += is_space & mask;
    }
    offset
}

/// Occurrences of `target` in `bytes` via 8-byte SWAR chunks + scalar tail.
///
/// from bcinr swar_str.rs:248-266 (`count_byte_in_slice`).
#[must_use]
pub fn count_byte_in_slice(bytes: &[u8], target: u8) -> usize {
    let mut count = 0usize;
    let (chunks, remainder) = bytes.as_chunks::<8>();
    for chunk in chunks {
        count += count_byte_in_word(u64::from_le_bytes(*chunk), target) as usize;
    }
    for &b in remainder {
        count += usize::from(b == target);
    }
    count
}

/// First index of `target` in `bytes`, or [`None`].
///
/// from bcinr swar_str.rs:283-313 (`find_first_byte_in_slice`).
#[must_use]
pub fn find_first_byte_in_slice(bytes: &[u8], target: u8) -> Option<usize> {
    let mut offset = 0usize;
    let (chunks, remainder) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_le_bytes(*chunk);
        if let Some(pos) = first_byte_position(word, target) {
            return Some(offset + pos as usize);
        }
        offset += 8;
    }
    for &b in remainder {
        if b == target {
            return Some(offset);
        }
        offset += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Horizontal reductions over 8 byte lanes (pure word kernels)
// ---------------------------------------------------------------------------

/// Per-lane unsigned `a >= b` mask: `0xFF` in each byte lane where the
/// 8-bit unsigned lane of `a` is `>=` that of `b`, else `0x00`.
///
/// Folds the *real* top bit back in (`a_i >= b_i <=> (a7 & !b7) |
/// ((a7 == b7) & (a_lo7 >= b_lo7))`), unlike the ASCII-only
/// [`byte_lane_in_range`] which assumes MSB-clear lanes.
///
/// from bcinr reduce.rs:109-122 (`swar_byte_ge_mask`).
#[inline]
#[must_use]
const fn swar_byte_ge_mask(a: u64, b: u64) -> u64 {
    let a7 = a & HIGHS;
    let b7 = b & HIGHS;
    // Borrow into bit 7 of the low-7-bit subtraction: set iff a_lo7 >= b_lo7.
    let borrow = (a | HIGHS).wrapping_sub(b & LO7) & HIGHS;
    // Fold in the genuine top bits; expand bit 7 to a full 0xFF lane mask.
    let ge = ((a7 & !b7) | (!(a7 ^ b7) & borrow)) & HIGHS;
    ge.wrapping_sub(ge >> 7) | ge
}

/// Per-lane unsigned byte max of two packed SWAR words.
///
/// from bcinr reduce.rs:127-130 (`swar_byte_max`).
#[inline]
#[must_use]
const fn swar_byte_max(a: u64, b: u64) -> u64 {
    let mask = swar_byte_ge_mask(a, b);
    (a & mask) | (b & !mask)
}

/// Per-lane unsigned byte min of two packed SWAR words.
///
/// from bcinr reduce.rs:135-138 (`swar_byte_min`).
#[inline]
#[must_use]
const fn swar_byte_min(a: u64, b: u64) -> u64 {
    let mask = swar_byte_ge_mask(a, b);
    (b & mask) | (a & !mask)
}

/// Sum of all 8 byte lanes of `word`, in `0..=2040`.
///
/// Carry-save tree: bytes -> u16 lanes -> u32 lanes -> final.
///
/// from bcinr reduce.rs:212-226 (`swar_horizontal_sum`).
#[inline]
#[must_use]
pub const fn horizontal_sum_u8x8(word: u64) -> u32 {
    let s2 = (word & 0x00FF_00FF_00FF_00FF).wrapping_add((word >> 8) & 0x00FF_00FF_00FF_00FF);
    let s4 = (s2 & 0x0000_FFFF_0000_FFFF).wrapping_add((s2 >> 16) & 0x0000_FFFF_0000_FFFF);
    ((s4 & 0x0000_0000_FFFF_FFFF).wrapping_add(s4 >> 32)) as u32
}

/// Maximum byte lane of `word` (true 8-bit unsigned compare).
///
/// 3-step tournament: adjacent pairs, then 16-, then 32-bit strides.
///
/// from bcinr reduce.rs:159-167 (`horizontal_max_u8x8`), narrowed to `u8`.
#[inline]
#[must_use]
pub const fn horizontal_max_u8x8(word: u64) -> u8 {
    let v1 = swar_byte_max(word, word >> 8);
    let v2 = swar_byte_max(v1, v1 >> 16);
    let v3 = swar_byte_max(v2, v2 >> 32);
    (v3 & 0xFF) as u8
}

/// Minimum byte lane of `word` (true 8-bit unsigned compare).
///
/// from bcinr reduce.rs:186-194 (`horizontal_min_u8x8`), narrowed to `u8`.
#[inline]
#[must_use]
pub const fn horizontal_min_u8x8(word: u64) -> u8 {
    let v1 = swar_byte_min(word, word >> 8);
    let v2 = swar_byte_min(v1, v1 >> 16);
    let v3 = swar_byte_min(v2, v2 >> 32);
    (v3 & 0xFF) as u8
}

/// Bitwise OR of all 8 byte lanes of `word`.
///
/// from bcinr reduce.rs (horizontal OR fold).
#[inline]
#[must_use]
pub const fn horizontal_or_u8x8(word: u64) -> u8 {
    let f = word | (word >> 8);
    let f = f | (f >> 16);
    (f | (f >> 32)) as u8
}

/// Bitwise AND of all 8 byte lanes of `word`.
///
/// from bcinr reduce.rs (horizontal AND fold).
#[inline]
#[must_use]
pub const fn horizontal_and_u8x8(word: u64) -> u8 {
    let f = word & (word >> 8);
    let f = f & (f >> 16);
    (f & (f >> 32)) as u8
}

/// Bitwise XOR of all 8 byte lanes of `word`.
///
/// from bcinr reduce.rs (horizontal XOR fold).
#[inline]
#[must_use]
pub const fn horizontal_xor_u8x8(word: u64) -> u8 {
    let f = word ^ (word >> 8);
    let f = f ^ (f >> 16);
    (f ^ (f >> 32)) as u8
}

/// Number of lanes (0-8) of `word` equal to `target`.
///
/// DIVERGENCE from bcinr (reduce.rs:280 `swar_count_eq_u8`): delegates to
/// [`find_byte_in_word`] + `count_ones` instead of a second Mycroft body —
/// one home for the zero-byte kernel (helpers one-home rule).
#[inline]
#[must_use]
pub const fn horizontal_count_eq_u8(word: u64, target: u8) -> u32 {
    find_byte_in_word(word, target).count_ones()
}

// ---------------------------------------------------------------------------
// Fixed-width parallel scans (const, `while` loops, no data branches)
// ---------------------------------------------------------------------------

/// Inclusive prefix sum of 16 `u32` lanes (Hillis-Steele, wrapping).
///
/// Up-passes at strides 1/2/4/8; the reversed per-pass iteration makes each
/// stride's down-sweep propagation implicit (every output index is covered).
///
/// from bcinr scan.rs:199-251 (`prefix_sum_u32x16`), unrolled body restated
/// as const-stable `while` loops.
#[inline]
#[must_use]
pub const fn prefix_sum_u32x16(arr: [u32; 16]) -> [u32; 16] {
    let mut a = arr;
    let mut stride = 1usize;
    while stride < 16 {
        let mut i = 15usize;
        while i >= stride {
            a[i] = a[i].wrapping_add(a[i - stride]);
            i -= 1;
        }
        stride <<= 1;
    }
    a
}

/// Exclusive prefix sum of 16 `u32` lanes: `out[0] == 0` always,
/// `out[i] == arr[0] + .. + arr[i-1]` (wrapping).
///
/// from bcinr scan.rs:265-271 (`exclusive_scan_u32x16`), const restated.
#[inline]
#[must_use]
pub const fn exclusive_scan_u32x16(arr: [u32; 16]) -> [u32; 16] {
    let inc = prefix_sum_u32x16(arr);
    let mut out = [0u32; 16];
    let mut i = 1usize;
    while i < 16 {
        out[i] = inc[i - 1];
        i += 1;
    }
    out
}

/// Segmented inclusive prefix sum of 8 `u32` lanes.
///
/// A segment begins wherever `flags[i]` is `true` (accumulator reset via
/// mask); lanes before the first flag form an implicit segment at index 0.
///
/// from bcinr scan.rs:295-306 (`segmented_prefix_sum_u32x8`), const restated.
#[inline]
#[must_use]
pub const fn segmented_prefix_sum_u32x8(values: [u32; 8], flags: [bool; 8]) -> [u32; 8] {
    let mut out = [0u32; 8];
    let mut acc: u32 = 0;
    let mut i = 0usize;
    while i < 8 {
        let reset = ((flags[i]) as u32).wrapping_neg();
        acc &= !reset;
        acc = acc.wrapping_add(values[i]);
        out[i] = acc;
        i += 1;
    }
    out
}

/// Inclusive prefix maximum of 16 `u32` lanes.
///
/// Mask-form max (`b ^ ((a ^ b) & ((a < b) as u32).wrapping_neg())`) —
/// `Ord::max` is not const-callable and the `>> 31` signed trick breaks for
/// lanes >= 2^31. Output is monotonically non-decreasing.
///
/// from bcinr scan.rs:334-345 (`prefix_max_u32x16`), max via mask form.
#[inline]
#[must_use]
pub const fn prefix_max_u32x16(arr: [u32; 16]) -> [u32; 16] {
    let mut out = arr;
    let mut i = 1usize;
    while i < 16 {
        let prev = out[i - 1];
        let cur = out[i];
        out[i] = cur ^ ((prev ^ cur) & ((prev < cur) as u32).wrapping_neg());
        i += 1;
    }
    out
}

/// Inclusive prefix XOR of 8 `u64` lanes.
///
/// Applied to `[0,1,..,7]` this yields the binary-reflected Gray code.
///
/// from bcinr scan.rs:368-378 (`prefix_xor_u64x8`), const restated.
#[inline]
#[must_use]
pub const fn prefix_xor_u64x8(arr: [u64; 8]) -> [u64; 8] {
    let mut a = arr;
    let mut i = 1usize;
    while i < 8 {
        a[i] ^= a[i - 1];
        i += 1;
    }
    a
}

// ---------------------------------------------------------------------------
// Slice reductions
// ---------------------------------------------------------------------------

/// Bitwise OR of every element of `slice`; `0` for the empty slice.
///
/// from bcinr reduce.rs:23-27 (`horizontal_or_u32`).
#[inline]
#[must_use]
pub fn horizontal_or_u32(slice: &[u32]) -> u32 {
    let mut acc = 0u32;
    for &v in slice {
        acc |= v;
    }
    acc
}

/// Bitwise AND of every element of `slice`; `0` for the empty slice.
///
/// DIVERGENCE from the mathematical AND identity (`u32::MAX`): bcinr
/// reduce.rs:39 pins `0` as the empty-slice result because `0` is the safe
/// AND-then-use identity (an empty AND yielding all-ones would let callers
/// act on garbage). Kept deliberately — do not "fix" this to `u32::MAX`.
///
/// from bcinr reduce.rs:46-51 (`horizontal_and_u32`).
#[inline]
#[must_use]
pub fn horizontal_and_u32(slice: &[u32]) -> u32 {
    let mut acc = u32::MAX;
    for &v in slice {
        acc &= v;
    }
    acc & ((slice.len() != 0) as u32).wrapping_neg()
}

/// Bitwise XOR of every element of `slice`; `0` for the empty slice.
///
/// from bcinr reduce.rs:69-73 (`horizontal_xor_u32`).
#[inline]
#[must_use]
pub fn horizontal_xor_u32(slice: &[u32]) -> u32 {
    let mut acc = 0u32;
    for &v in slice {
        acc ^= v;
    }
    acc
}

/// Minimum of `slice`; [`u32::MAX`] for the empty slice.
///
/// Unsigned borrow via `u64` promotion (bit 32 of `a - b` set iff `a < b`)
/// — correct across the full unsigned range, unlike the `>> 31` signed
/// trick which breaks near [`u32::MAX`].
///
/// from bcinr reduce.rs:315-329 (`reduce_min_u32`).
#[inline]
#[must_use]
pub fn reduce_min_u32(slice: &[u32]) -> u32 {
    let mut acc = u32::MAX;
    for &b in slice {
        let a = acc;
        let borrow = ((((a) as u64).wrapping_sub((b) as u64)) >> 32) as u32 & 1;
        let neg_borrow = borrow.wrapping_neg();
        acc = b.wrapping_add(a.wrapping_sub(b) & neg_borrow);
    }
    acc
}

/// Maximum of `slice`; `0` for the empty slice.
///
/// from bcinr reduce.rs:342-356 (`reduce_max_u32`).
#[inline]
#[must_use]
pub fn reduce_max_u32(slice: &[u32]) -> u32 {
    let mut acc = 0u32;
    for &b in slice {
        let a = acc;
        let borrow = ((((b) as u64).wrapping_sub((a) as u64)) >> 32) as u32 & 1;
        let neg_nb = (1u32 - borrow).wrapping_neg();
        acc = a.wrapping_add(b.wrapping_sub(a) & neg_nb);
    }
    acc
}

/// Sum of `slice` accumulated into a `u64` (no `u32` overflow); `0` empty.
///
/// from bcinr reduce.rs:369-375 (`reduce_sum_u64`).
#[inline]
#[must_use]
pub fn reduce_sum_u64(slice: &[u32]) -> u64 {
    let mut acc = 0u64;
    for &v in slice {
        acc = acc.wrapping_add((v) as u64);
    }
    acc
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec::Vec;

    // Independent structure: per-byte loop reference (different from SWAR).
    fn ref_positions(bytes: &[u8], target: u8) -> u64 {
        let mut m = 0u64;
        for (i, &b) in bytes.iter().take(64).enumerate() {
            if b == target {
                m |= 1 << i;
            }
        }
        m
    }

    #[test]
    fn word_kernels_match_reference() {
        let word = u64::from_le_bytes(*b"hello wo");
        assert_eq!(first_byte_position(word, b'l'), Some(2));
        assert_eq!(first_byte_position(word, b'z'), None);
        // "hello wo" (8 bytes) holds 'l' only at lanes 2 and 3 — the 4th l of
        // "hello world" fell off the 8-byte truncation.
        assert_eq!(count_byte_in_word(word, b'l'), 2);
        assert!(has_byte_in_word(word, b'o'));
        assert!(!has_byte_in_word(word, b'z'));
        assert_eq!(
            to_lower_ascii_word(u64::from_le_bytes(*b"HELLO WO")).to_le_bytes(),
            *b"hello wo"
        );
        assert_eq!(to_upper_ascii_word(word).to_le_bytes(), *b"HELLO WO");
    }

    #[test]
    fn classify_matches_reference() {
        // 'A': alpha|upper = 0x0A; '5': digit = 0x01; ' ': ws = 0x04.
        let word = u64::from_le_bytes(*b"A5 z    ");
        let cls = classify_bytes(word).to_le_bytes();
        assert_eq!(cls[0], 0x0A);
        assert_eq!(cls[1], 0x01);
        assert_eq!(cls[2], 0x04);
        assert_eq!(cls[3], 0x12); // 'z': alpha|lower
        assert_eq!(cls[4], 0x04); // lane 4 is ' ' (ws), not NUL
    }

    #[test]
    fn parse_decimal_boundaries() {
        assert_eq!(
            parse_8_decimal_digits(u64::from_le_bytes(*b"12345678")),
            Some(12_345_678)
        );
        assert_eq!(
            parse_8_decimal_digits(u64::from_le_bytes(*b"00000000")),
            Some(0)
        );
        assert_eq!(
            parse_8_decimal_digits(u64::from_le_bytes(*b"1234567X")),
            None
        );
        assert_eq!(
            parse_8_decimal_digits(u64::from_le_bytes(*b"/2345678")),
            None
        );
        assert_eq!(
            parse_8_decimal_digits(u64::from_le_bytes(*b"99999999")),
            Some(99_999_999)
        );
    }

    #[test]
    fn parse_hex_swar_matches_scalar_reference() {
        // Scalar reference (independent structure).
        fn reference(bytes: [u8; 4]) -> Option<u32> {
            let mut result = 0u32;
            for &b in &bytes {
                let d = match b {
                    b'0'..=b'9' => (b - b'0') as u32,
                    b'A'..=b'F' => (b - b'A') as u32 + 10,
                    b'a'..=b'f' => (b - b'a') as u32 + 10,
                    _ => return None,
                };
                result = (result << 4) | d;
            }
            Some(result)
        }
        for bytes in [b"1A2f", b"0000", b"FFFF", b"ffff", b"dead", b"BEEF"] {
            let w = u32::from_le_bytes(*bytes);
            assert_eq!(parse_4_hex_digits(w), reference(*bytes), "input {bytes:?}");
        }
        assert_eq!(parse_4_hex_digits(u32::from_le_bytes(*b"1G2f")), None);
        assert_eq!(parse_4_hex_digits(u32::from_le_bytes(*b"1a2g")), None);
        assert_eq!(parse_4_hex_digits(u32::from_le_bytes(*b"1a:2")), None);
    }

    #[test]
    fn slice_kernels_match_reference() {
        let data = b"hello world, hello!";
        assert_eq!(count_byte_in_slice(data, b'l'), 5);
        assert_eq!(find_first_byte_in_slice(data, b'o'), Some(4));
        assert_eq!(find_first_byte_in_slice(data, b'z'), None);
        assert_eq!(count_leading_eq_u8(b"aaabcd", b'a'), 3);
        assert_eq!(count_leading_eq_u8(b"xxxx", b'x'), 4);
        assert_eq!(count_leading_eq_u8(b"", b'x'), 0);
        assert_eq!(skip_spaces(b"   hello"), 3);
        assert_eq!(skip_spaces(b"hello"), 0);
        assert!(is_all_ascii(b"hello world"));
        assert!(!is_all_ascii(b"caf\xC3\xA9"));
        assert!(is_all_ascii(b""));
    }

    #[test]
    fn find_all_positions_exhaustive_vs_reference() {
        // Exhaustive over a small alphabet at multiple lengths (real mutant
        // killer, not the vacuous `if m != baseline` form).
        for len in 0..=20usize {
            for target in [b'a', b'\0', 0xFF] {
                let mut bytes = [b'b'; 20];
                for (i, slot) in bytes.iter_mut().enumerate().take(len) {
                    *slot = if (i * 7 + usize::from(target)) % 3 == 0 {
                        target
                    } else {
                        b'b'
                    };
                }
                assert_eq!(
                    swar_find_all_positions(&bytes[..len], target),
                    ref_positions(&bytes[..len], target),
                    "len {len} target {target}"
                );
            }
        }
    }

    #[test]
    fn word_match_kernels() {
        let word = u64::from_le_bytes(*b"abcabcab");
        assert_eq!(find_byte_mask_word(word, b'a'), 0x0080_0000_8000_0080);
        assert_eq!(find_first_of_word(word, b'c'), 2);
        assert_eq!(find_first_of_word(word, b'z'), 8);
        // Cascade-safety: adjacent matching lanes must not cross-talk.
        assert_eq!(
            find_byte_mask_word(u64::from_le_bytes(*b"aaaaaaaa"), b'a'),
            HIGHS
        );
        assert_eq!(find_first_of_word(0, 0), 0);
        assert_eq!(
            find_byte_mask(b"hello world", b'l'),
            (1 << 2) | (1 << 3) | (1 << 9)
        );
        assert_eq!(find_byte_mask(&[], b'x'), 0);
    }

    // --- horizontal reduce/scan family (OPT-55) ----------------------------

    /// Scalar per-lane oracle: independent formulation (loop over
    /// `to_le_bytes`, no SWAR).
    fn lanes(word: u64) -> [u8; 8] {
        word.to_le_bytes()
    }

    /// Deterministic xorshift64 PRNG (same construction as the bcinr tests).
    fn xorshift(seed: u64) -> impl Iterator<Item = u64> {
        let mut s = seed;
        std::iter::from_fn(move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            Some(s)
        })
    }

    #[test]
    fn horizontal_word_kernels_canonical() {
        let word = u64::from_le_bytes([3, 1, 4, 1, 5, 9, 2, 6]);
        assert_eq!(horizontal_sum_u8x8(word), 31);
        assert_eq!(horizontal_max_u8x8(word), 9);
        assert_eq!(horizontal_min_u8x8(word), 1);
        assert_eq!(horizontal_or_u8x8(word), 0x0F);
        assert_eq!(horizontal_and_u8x8(word), 0);
        assert_eq!(horizontal_xor_u8x8(word), 0x0F);
        assert_eq!(horizontal_count_eq_u8(word, 1), 2);
        assert_eq!(horizontal_count_eq_u8(word, 7), 0);
        // u64::MAX: every lane 0xFF.
        assert_eq!(horizontal_sum_u8x8(u64::MAX), 2040);
        assert_eq!(horizontal_max_u8x8(u64::MAX), 0xFF);
        assert_eq!(horizontal_min_u8x8(u64::MAX), 0xFF);
        assert_eq!(horizontal_or_u8x8(u64::MAX), 0xFF);
        assert_eq!(horizontal_and_u8x8(u64::MAX), 0xFF);
        assert_eq!(horizontal_xor_u8x8(u64::MAX), 0);
        assert_eq!(horizontal_count_eq_u8(u64::MAX, 0xFF), 8);
        // Zero word.
        assert_eq!(horizontal_sum_u8x8(0), 0);
        assert_eq!(horizontal_max_u8x8(0), 0);
        assert_eq!(horizontal_min_u8x8(0), 0);
        assert_eq!(horizontal_count_eq_u8(0, 0), 8);
    }

    #[test]
    fn horizontal_word_kernels_high_bit_lanes() {
        // Lanes [6,5,4,3,2,1,0x7F,0x80]: the 0x80 lane must win the max even
        // though its low 7 bits are 0 — kills low-7-bit-only compare mutants.
        let word = u64::from_le_bytes([0x80, 0x7F, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(horizontal_max_u8x8(word), 0x80);
        assert_eq!(horizontal_min_u8x8(word), 0x01);
        // Same-direction kills for the ge_mask: 0x80 >= 0x7F, 0x7F < 0x80.
        assert_eq!(horizontal_min_u8x8(u64::from_le_bytes([0xFF; 8])), 0xFF);
        assert_eq!(horizontal_max_u8x8(0x8080_8080_8080_8080), 0x80);
        assert_eq!(
            horizontal_min_u8x8(0xFF80_8182_8384_8586u64.swap_bytes()),
            0x80
        );
    }

    #[test]
    fn horizontal_word_kernels_oracle() {
        for word in xorshift(0x9E37_79B9_7F4A_7C15).take(20_000) {
            let b = lanes(word);
            assert_eq!(
                horizontal_sum_u8x8(word),
                b.iter().map(|&x| (x) as u32).sum::<u32>()
            );
            assert_eq!(
                horizontal_max_u8x8(word),
                b.iter().fold(0u8, |a, &x| if x > a { x } else { a })
            );
            assert_eq!(
                horizontal_min_u8x8(word),
                b.iter().fold(0xFFu8, |a, &x| if x < a { x } else { a })
            );
            assert_eq!(horizontal_or_u8x8(word), b.iter().fold(0u8, |a, &x| a | x));
            assert_eq!(
                horizontal_and_u8x8(word),
                b.iter().fold(0xFFu8, |a, &x| a & x)
            );
            assert_eq!(horizontal_xor_u8x8(word), b.iter().fold(0u8, |a, &x| a ^ x));
        }
    }

    #[test]
    fn horizontal_count_eq_cross_oracle() {
        // Cross-oracle against the pre-existing count_byte_in_word over words
        // truncated to every length 0..=8 (adversarial tail shapes).
        for word in xorshift(0xDEAD_BEEF_CAFE_F00D).take(5_000) {
            for len in 0..=8usize {
                let masked = if len == 8 {
                    word
                } else {
                    word & ((1u64 << (8 * len)) - 1)
                };
                for target in [0u8, 0x01, 0x7F, 0x80, 0xFF] {
                    assert_eq!(
                        horizontal_count_eq_u8(masked, target),
                        count_byte_in_word(masked, target),
                        "word {masked:#x} len {len} target {target:#x}"
                    );
                }
            }
        }
    }

    #[test]
    fn scan_kernels_match_scalar_prefix() {
        // Scalar running-prefix oracle over deterministic pseudo-random inputs,
        // including wrapping (all-ones) values.
        let mut rng = xorshift(0x0123_4567_89AB_CDEF);
        for _ in 0..256 {
            let mut v16 = [0u32; 16];
            for slot in &mut v16 {
                slot = (rng.next().unwrap() & 0xFFFF_FFFF) as u32;
            }
            v16[0] = u32::MAX;
            v16[7] = 0x8000_0000;

            let mut pref = [0u32; 16];
            let mut acc = 0u32;
            for i in 0..16 {
                acc = acc.wrapping_add(v16[i]);
                pref[i] = acc;
            }
            assert_eq!(prefix_sum_u32x16(v16), pref);

            let mut excl = [0u32; 16];
            for i in 1..16 {
                excl[i] = pref[i - 1];
            }
            assert_eq!(exclusive_scan_u32x16(v16), excl);
            assert_eq!(exclusive_scan_u32x16(v16)[0], 0);

            let mut pmax = [0u32; 16];
            let mut m = 0u32;
            for i in 0..16 {
                m = m.max(v16[i]);
                pmax[i] = m;
            }
            assert_eq!(prefix_max_u32x16(v16), pmax);
            // Monotonicity.
            assert!(prefix_max_u32x16(v16).windows(2).all(|w| w[1] >= w[0]));

            let mut v8 = [0u64; 8];
            for slot in &mut v8 {
                slot = rng.next().unwrap();
            }
            let mut px = [0u64; 8];
            let mut a = 0u64;
            for i in 0..8 {
                a ^= v8[i];
                px[i] = a;
            }
            assert_eq!(prefix_xor_u64x8(v8), px);
        }
    }

    #[test]
    fn prefix_xor_gray_code() {
        let natural: [u64; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(prefix_xor_u64x8(natural), [0, 1, 3, 0, 4, 1, 7, 0]);
    }

    #[test]
    fn segmented_prefix_sum_documented_example() {
        let v = [1u32, 2, 3, 4, 5, 6, 7, 8];
        let f = [false, false, false, true, false, false, true, false];
        assert_eq!(segmented_prefix_sum_u32x8(v, f), [1, 3, 6, 4, 9, 15, 7, 15]);
        // Leading flag: implicit segment empty, accumulator restarts at lane 0.
        assert_eq!(
            segmented_prefix_sum_u32x8(v, [true; 8]),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        // No flags: plain inclusive scan (wrapping).
        let ones = [u32::MAX; 8];
        let mut acc = 0u32;
        let mut want = [0u32; 8];
        for i in 0..8 {
            acc = acc.wrapping_add(ones[i]);
            want[i] = acc;
        }
        assert_eq!(segmented_prefix_sum_u32x8(ones, [false; 8]), want);
    }

    #[test]
    fn exclusive_scan_zero_prefix_and_boundaries() {
        assert_eq!(exclusive_scan_u32x16([0u32; 16]), [0u32; 16]);
        let ones = [1u32; 16];
        assert_eq!(
            exclusive_scan_u32x16(ones),
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
        assert_eq!(prefix_sum_u32x16([0u32; 16]), [0u32; 16]);
        assert_eq!(
            prefix_sum_u32x16(ones),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
        assert_eq!(prefix_max_u32x16([0u32; 16]), [0u32; 16]);
        // High-bit lanes through the mask-form max.
        let mut high = [0u32; 16];
        high[0] = 0x8000_0000;
        high[2] = u32::MAX;
        let pm = prefix_max_u32x16(high);
        assert_eq!(pm[0], 0x8000_0000);
        assert_eq!(pm[2], u32::MAX);
        assert_eq!(pm[15], u32::MAX);
    }

    #[test]
    fn slice_reductions_match_iterator_oracle() {
        let values = [0u32, u32::MAX, 0x8000_0000, 1u32];
        let mut rng = xorshift(0x5EED_5EED_5EED_5EED);
        for len in [0usize, 1, 2, 3, 7, 8, 9, 16, 17, 31, 64] {
            let slice: Vec<u32> = (0..len)
                .map(|i| {
                    if i % 5 == 0 {
                        values[i % values.len()]
                    } else {
                        (rng.next().unwrap() & 0xFFFF_FFFF) as u32
                    }
                })
                .collect();
            let s = &slice[..];
            assert_eq!(horizontal_or_u32(s), s.iter().fold(0u32, |a, &x| a | x));
            let and_ref = s.iter().fold(0xFF, |a, &x| a & x);
            let and_want = if s.len() == 0 { 0 } else { and_ref };
            assert_eq!(horizontal_and_u32(s), and_want);
            assert_eq!(horizontal_xor_u32(s), s.iter().fold(0u32, |a, &x| a ^ x));
            let min_ref = s.iter().fold(u32::MAX, |a, &x| if x < a { x } else { a });
            assert_eq!(reduce_min_u32(s), min_ref);
            let max_ref = s.iter().fold(0u32, |a, &x| if x > a { x } else { a });
            assert_eq!(reduce_max_u32(s), max_ref);
            assert_eq!(
                reduce_sum_u64(s),
                s.iter().map(|&x| (x) as u64).sum::<u64>()
            );
        }
    }

    #[test]
    fn empty_slice_returns_identity_not_panic() {
        let empty: &[u32] = &[];
        assert_eq!(horizontal_or_u32(empty), 0);
        // AND is pinned to 0 (bcinr reduce.rs:39 safe AND-then-use identity),
        // NOT the mathematical u32::MAX — do not "fix".
        assert_eq!(horizontal_and_u32(empty), 0);
        assert_eq!(horizontal_xor_u32(empty), 0);
        assert_eq!(reduce_min_u32(empty), u32::MAX);
        assert_eq!(reduce_max_u32(empty), 0);
        assert_eq!(reduce_sum_u64(empty), 0);
    }

    #[test]
    fn slice_reductions_extremes() {
        assert_eq!(reduce_min_u32(&[u32::MAX, u32::MAX - 1, 0]), 0);
        assert_eq!(reduce_min_u32(&[0x8000_0000, 1, 0xFFFF_FFFF]), 1);
        assert_eq!(reduce_max_u32(&[0x8000_0000, 1, 0x7FFF_FFFF]), 0x8000_0000);
        assert_eq!(reduce_sum_u64(&[u32::MAX; 64]), 64 * (u32::MAX) as u64);
        assert_eq!(horizontal_or_u32(&[0xAAAA_AAAA, 0x5555_5555]), u32::MAX);
        assert_eq!(horizontal_and_u32(&[0xAAAA_AAAA, 0x5555_5555]), 0);
        assert_eq!(horizontal_xor_u32(&[u32::MAX, u32::MAX]), 0);
        assert_eq!(horizontal_and_u32(&[u32::MAX]), u32::MAX);
    }
}

// `count_byte_in_slice` / `find_first_byte_in_slice` are the bcinr slice
// kernels (swar_str.rs:248-321); kept as public API for consumers that want
// the count/first-index forms rather than the bitmask form.

//! ASCII/UTF-8 byte classification and hex/decimal parsing kernels.
//!
//! Ported from BranchlessCInRust (`bcinr`), scaffold ABIs stripped.
//! Constant-shape: masked classification, no data-dependent branches on the
//! hot path, zero heap.
//!
//! Canonical homes: `classify_bytes` and `parse_8_decimal_digits` live in
//! [`crate::swar`] (with their shared `byte_lane_in_range` kernel); this
//! module re-exports them for the char-classification surface.

// Canonical home: `crate::swar::classify_bytes`
// (from bcinr crates/bcinr-logic/src/swar_str.rs:353-390, `swar_classify_bytes`)
/// Classifies each byte of a `u64` word into a per-lane bitfield:
/// bit 0 digit (`0`-`9`), bit 1 alpha, bit 2 whitespace (`\t`-`\r`, ` `),
/// bit 3 uppercase, bit 4 lowercase.
pub use crate::swar::classify_bytes;

// Canonical home: `crate::swar::parse_8_decimal_digits`
// (from bcinr crates/bcinr-logic/src/swar_str.rs:400-473)
/// Parses exactly 8 ASCII decimal digits from a word; `None` if any lane is
/// outside `'0'`-`'9'`.
pub use crate::swar::parse_8_decimal_digits;

// from bcinr crates/bcinr-logic/src/algorithms/ascii_to_lowercase_simd.rs:28-37
/// Folds every ASCII uppercase lane to lowercase; other lanes unchanged.
#[inline]
#[must_use]
pub fn ascii_to_lowercase_word(word: u64) -> u64 {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const H: u64 = 0x8080_8080_8080_8080;
    const LO7: u64 = 0x7F7F_7F7F_7F7F_7F7F;
    let low = word & LO7;
    let upper = ONES.wrapping_mul(127 + 0x5B).wrapping_sub(low);
    let lower = low.wrapping_add(ONES.wrapping_mul(127 - 0x40));
    let mask = upper & !word & lower & H;
    word.wrapping_add(mask >> 2)
}

// from bcinr crates/bcinr-logic/src/algorithms/ascii_to_uppercase_simd.rs
/// Folds every ASCII lowercase lane to uppercase; other lanes unchanged.
#[inline]
#[must_use]
pub fn ascii_to_uppercase_word(word: u64) -> u64 {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const H: u64 = 0x8080_8080_8080_8080;
    const LO7: u64 = 0x7F7F_7F7F_7F7F_7F7F;
    let low = word & LO7;
    // mirror of the lowercase kernel: lanes in 'a'..='z' become 'A'..='Z'
    let upper = ONES.wrapping_mul(127 + 0x7B).wrapping_sub(low);
    let lower = low.wrapping_add(ONES.wrapping_mul(127 - 0x60));
    let mask = upper & !word & lower & H;
    word.wrapping_sub(mask >> 2)
}

// from bcinr crates/bcinr-logic/src/algorithms/hex_decode_simd.rs:35-42
/// One hex ASCII byte -> 4-bit nibble, else 0 (SWAR sign-bit range masks).
#[inline]
#[must_use]
fn decode_hex_nibble(c: u64) -> u64 {
    let gt = |a: u64, b: u64| 0u64.wrapping_sub(b.wrapping_sub(a) >> 63);
    let rng = |lo: u64, hi: u64| gt(c, lo - 1) & gt(hi, c.wrapping_sub(1));
    (c.wrapping_sub(0x30) & rng(0x30, 0x39))   // '0'..'9' -> 0..9
        | (c.wrapping_sub(0x57) & rng(0x61, 0x66)) // 'a'..'f' -> 10..15
        | (c.wrapping_sub(0x37) & rng(0x41, 0x46)) // 'A'..'F' -> 10..15
}

// from bcinr crates/bcinr-logic/src/algorithms/hex_decode_simd.rs:22-29
/// Decodes one hex pair into a byte; invalid nibbles contribute 0.
#[inline]
#[must_use]
pub fn hex_decode_pair(c_hi: u8, c_lo: u8) -> u8 {
    ((decode_hex_nibble(u64::from(c_hi)) << 4) | decode_hex_nibble(u64::from(c_lo))) as u8
}

// from bcinr crates/bcinr-logic/src/algorithms/hex_encode_chunk8.rs:28-43
/// Encodes the 8 nibbles of the low 32 bits as uppercase hex ASCII, packed
/// little-endian (nibble `j` -> byte `j`).
#[inline]
#[must_use]
pub fn hex_encode_chunk8_upper(val: u64) -> u64 {
    hex_encode_chunk8_inner(val, 0x07)
}

// from bcinr crates/bcinr-logic/src/algorithms/hex_encode_simd.rs:28-43 (the `+0x27` twin)
/// [`hex_encode_chunk8_upper`] with lowercase letters.
#[inline]
#[must_use]
pub fn hex_encode_chunk8_lower(val: u64) -> u64 {
    hex_encode_chunk8_inner(val, 0x27)
}

#[inline]
#[must_use]
fn hex_encode_chunk8_inner(val: u64, letter_correction: u64) -> u64 {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const H: u64 = 0x8080_8080_8080_8080;
    const LO7: u64 = 0x7F7F_7F7F_7F7F_7F7F;
    let mut s = val & 0xFFFF_FFFF;
    s = (s | (s << 16)) & 0x0000_FFFF_0000_FFFF;
    s = (s | (s << 8)) & 0x00FF_00FF_00FF_00FF;
    s = (s | (s << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
    let low = s & LO7;
    let upper = ONES.wrapping_mul(127 + 16).wrapping_sub(low);
    let lower = low.wrapping_add(ONES.wrapping_mul(127 - 9));
    let mask = upper & !s & lower & H;
    s.wrapping_add(ONES.wrapping_mul(0x30))
        .wrapping_add((mask >> 7).wrapping_mul(letter_correction))
}

// from bcinr crates/bcinr-logic/src/parse.rs:65-89 (`parse_hex_u32`)
/// Parses 1-8 ASCII hex characters into a `u32`; `Err(())` on empty,
/// oversize, or invalid input.
#[inline]
#[must_use = "parse result — ignoring discards the parsed value and cursor"]
#[allow(clippy::result_unit_err)]
pub fn parse_hex_u32(bytes: &[u8]) -> Result<u32, ()> {
    let mut res = 0u32;
    let len = bytes.len();
    let mut err = u32::from(len == 0 || len > 8);
    (0..8).for_each(|i| {
        let b = bytes.get(i).copied().unwrap_or(0) & 0u8.wrapping_sub(u8::from(i < len));
        let is_digit = u32::from(b.is_ascii_digit());
        let is_upper = u32::from((b'A'..=b'F').contains(&b));
        let is_lower = u32::from((b'a'..=b'f').contains(&b));
        let val = (is_digit * u32::from(b.wrapping_sub(b'0')))
            | (is_upper * u32::from(b.wrapping_sub(b'A').wrapping_add(10)))
            | (is_lower * u32::from(b.wrapping_sub(b'a').wrapping_add(10)));
        err |= (!(is_digit | is_upper | is_lower) & u32::from(i < len)) & 1;
        res = (res << (4 * u32::from(i < len))) | (val & 0u32.wrapping_sub(u32::from(i < len)));
    });
    [Err(()), Ok(res)][usize::from(err == 0)]
}

// from bcinr crates/bcinr-logic/src/parse.rs:123-160 (`parse_decimal_u64`)
/// Parses 1-20 ASCII decimal characters into a `u64`; `Err(())` on empty,
/// oversize, invalid, or overflow.
#[inline]
#[must_use = "parse result — ignoring discards the parsed value and cursor"]
#[allow(clippy::result_unit_err)]
pub fn parse_decimal_u64(bytes: &[u8]) -> Result<u64, ()> {
    let len = bytes.len();
    let mut err = u32::from(len == 0 || len > 20);
    let mut acc: u128 = 0;
    (0..20).for_each(|i| {
        let in_range = u8::from(i < len);
        let b = bytes.get(i).copied().unwrap_or(0) & 0u8.wrapping_sub(in_range);
        let is_digit = u32::from(b.is_ascii_digit());
        err |= (!is_digit & u32::from(in_range)) & 1;
        let digit = u128::from(b.wrapping_sub(b'0'));
        // Out-of-range iterations must leave acc unchanged (identity multiplier),
        // not shift the value left one decade per padding iteration.
        let mult: u128 = 1 + 9 * u128::from(in_range);
        acc = acc * mult + digit * u128::from(in_range);
    });
    err |= u32::from(acc > u128::from(u64::MAX));
    // `acc` exceeds u64::MAX only on the overflow error path, where the Ok arm
    // is never selected; the default is discarded along with the Err.
    [Err(()), Ok(u64::try_from(acc).unwrap_or(0))][usize::from(err == 0)]
}

// ─── UTF-8 const classifiers (bcinr utf8.rs:29-142, gate stub excluded) ───

/// `true` iff `byte` is a UTF-8 continuation byte (`10xxxxxx`).
#[inline]
#[must_use]
pub const fn is_continuation_byte(byte: u8) -> bool {
    (byte & 0xC0) == 0x80
}

/// `true` iff `byte` is ASCII (`0x00`-`0x7F`).
#[inline]
#[must_use]
pub const fn is_ascii_byte(byte: u8) -> bool {
    byte < 0x80
}

/// `true` iff `byte` leads a 2-byte UTF-8 sequence (`110xxxxx`).
#[inline]
#[must_use]
pub const fn is_2byte_lead(byte: u8) -> bool {
    (byte & 0xE0) == 0xC0
}

/// `true` iff `byte` leads a 3-byte UTF-8 sequence (`1110xxxx`).
#[inline]
#[must_use]
pub const fn is_3byte_lead(byte: u8) -> bool {
    (byte & 0xF0) == 0xE0
}

/// `true` iff `byte` leads a 4-byte UTF-8 sequence (`11110xxx`).
#[inline]
#[must_use]
pub const fn is_4byte_lead(byte: u8) -> bool {
    (byte & 0xF8) == 0xF0
}

/// Counts codepoints branchlessly: every non-continuation byte starts one.
#[inline]
#[must_use]
pub fn count_codepoints(bytes: &[u8]) -> usize {
    let mut count = 0;
    (0..bytes.len()).for_each(|i| {
        count += usize::from((bytes[i] & 0xC0) != 0x80);
    });
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_and_case_kernels() {
        // Oracle: per-byte scalar loop (independent structure).
        fn classify_ref(word: u64) -> u64 {
            let mut out = 0u64;
            for lane in 0..8 {
                let b = (word >> (8 * lane)) & 0xFF;
                let mut flags = 0u64;
                if (0x30..=0x39).contains(&b) {
                    flags |= 0x01;
                }
                if (0x41..=0x5A).contains(&b) || (0x61..=0x7A).contains(&b) {
                    flags |= 0x02;
                }
                if (0x09..=0x0D).contains(&b) || b == 0x20 {
                    flags |= 0x04;
                }
                if (0x41..=0x5A).contains(&b) {
                    flags |= 0x08;
                }
                if (0x61..=0x7A).contains(&b) {
                    flags |= 0x10;
                }
                out |= flags << (8 * lane);
            }
            out
        }

        let words = [
            u64::from_le_bytes(*b"A       "),
            u64::from_le_bytes(*b"aZ9 _-\x00\x00"),
            0,
            u64::MAX,
        ];
        for &w in &words {
            assert_eq!(classify_bytes(w), classify_ref(w), "word={w:#x}");
            // Case folding: only case-significant lanes move.
            let lower = ascii_to_lowercase_word(w);
            let upper = ascii_to_uppercase_word(w);
            for lane in 0..8 {
                let b = (w >> (8 * lane)) & 0xFF;
                let got_lo = (lower >> (8 * lane)) & 0xFF;
                let got_up = (upper >> (8 * lane)) & 0xFF;
                if (0x41..=0x5A).contains(&b) {
                    assert_eq!(got_lo, b + 0x20);
                    assert_eq!(got_up, b);
                } else if (0x61..=0x7A).contains(&b) {
                    assert_eq!(got_lo, b);
                    assert_eq!(got_up, b - 0x20);
                } else {
                    assert_eq!(got_lo, b);
                    assert_eq!(got_up, b);
                }
            }
        }
    }

    #[test]
    fn hex_kernels_roundtrip() {
        // Oracle: scalar nibble loop (independent structure).
        fn encode_ref(val: u64, upper: bool) -> u64 {
            let x = val & 0xFFFF_FFFF;
            let mut out = 0u64;
            for j in 0..8 {
                let nib = (x >> (4 * j)) & 0xF;
                let ch = if nib < 10 {
                    0x30 + nib
                } else if upper {
                    0x41 + nib - 10
                } else {
                    0x61 + nib - 10
                };
                out |= ch << (8 * j);
            }
            out
        }
        for &v in &[0, 1, 0x1234_5678, 0xFFFF_FFFF] {
            assert_eq!(hex_encode_chunk8_upper(v), encode_ref(v, true));
            assert_eq!(hex_encode_chunk8_lower(v), encode_ref(v, false));
        }
        // Nibble decode oracle via char::to_digit (independent structure).
        for c in 0u64..=0xFF {
            let expected = char::from_u32(c as u32)
                .and_then(|ch| ch.to_digit(16))
                .unwrap_or(0);
            assert_eq!(decode_hex_nibble(c), u64::from(expected), "c={c:#x}");
        }
        // Pair decode roundtrip over the full byte space.
        for hi in 0u64..=0xFF {
            for lo in 0u64..=0xFF {
                let expected = ((char::from_u32(u32::try_from(hi).unwrap_or(0))
                    .and_then(|c| c.to_digit(16))
                    .map_or(0, u64::from))
                    << 4)
                    | char::from_u32(u32::try_from(lo).unwrap_or(0))
                        .and_then(|c| c.to_digit(16))
                        .map_or(0, u64::from);
                assert_eq!(hex_decode_pair(hi as u8, lo as u8), expected as u8);
            }
        }
    }

    #[test]
    fn parses_match_oracles() {
        // parse_8_decimal_digits oracle: scalar fold (independent structure).
        fn dec8_ref(word: u64) -> Option<u32> {
            let bytes = word.to_le_bytes();
            let mut acc: u64 = 0;
            for &b in &bytes {
                if !(0x30..=0x39).contains(&b) {
                    return None;
                }
                acc = acc * 10 + u64::from(b - b'0');
            }
            u32::try_from(acc).ok()
        }
        for &word in &[
            u64::from_le_bytes(*b"12345678"),
            u64::from_le_bytes(*b"00000000"),
            u64::from_le_bytes(*b"99999999"),
            u64::from_le_bytes(*b"1234567X"),
            0,
            u64::MAX,
            u64::from_le_bytes(*b"/0000000"), // 0x2F < '0'
        ] {
            assert_eq!(
                parse_8_decimal_digits(word),
                dec8_ref(word),
                "word={word:#x}"
            );
        }

        // parse_hex_u32: boundary vectors (from bcinr parse.rs tests).
        assert_eq!(parse_hex_u32(b""), Err(()));
        assert_eq!(parse_hex_u32(b"0"), Ok(0));
        assert_eq!(parse_hex_u32(b"F"), Ok(15));
        assert_eq!(parse_hex_u32(b"f"), Ok(15));
        assert_eq!(parse_hex_u32(b"FFFFFFFF"), Ok(u32::MAX));
        assert_eq!(parse_hex_u32(b"aabbccdd"), Ok(0xAABB_CCDD));
        assert_eq!(parse_hex_u32(b"123456789"), Err(()));
        assert_eq!(parse_hex_u32(b"0G"), Err(()));

        // parse_decimal_u64: boundary vectors incl. u64::MAX and overflow.
        assert_eq!(parse_decimal_u64(b""), Err(()));
        assert_eq!(parse_decimal_u64(b"0"), Ok(0));
        assert_eq!(parse_decimal_u64(b"42"), Ok(42));
        assert_eq!(parse_decimal_u64(b"007"), Ok(7));
        assert_eq!(parse_decimal_u64(b"18446744073709551615"), Ok(u64::MAX));
        assert_eq!(parse_decimal_u64(b"18446744073709551616"), Err(()));
        assert_eq!(parse_decimal_u64(b"99999999999999999999"), Err(()));
        assert_eq!(parse_decimal_u64(b"123456789012345678901"), Err(()));
        assert_eq!(parse_decimal_u64(b"12x"), Err(()));
        assert_eq!(parse_decimal_u64(b" 1"), Err(()));
    }

    #[test]
    fn utf8_classifiers_and_count() {
        assert!(is_continuation_byte(0x80));
        assert!(is_continuation_byte(0xBF));
        assert!(!is_continuation_byte(0x41));
        assert!(!is_continuation_byte(0xC2));
        assert!(is_ascii_byte(0x00));
        assert!(!is_ascii_byte(0x80));
        assert!(is_2byte_lead(0xC2));
        assert!(!is_2byte_lead(0xE0));
        assert!(is_3byte_lead(0xE0));
        assert!(!is_3byte_lead(0xF0));
        assert!(is_4byte_lead(0xF0));
        assert!(!is_4byte_lead(0xF8));
        assert_eq!(count_codepoints(b"hello"), 5);
        assert_eq!(count_codepoints(&[0xC3, 0xA9]), 1);
        assert_eq!(count_codepoints(&[0xE4, 0xB8, 0x96]), 1);
        assert_eq!(count_codepoints(&[0xF0, 0x9F, 0x98, 0x80]), 1);
        assert_eq!(count_codepoints(b""), 0);
    }
}

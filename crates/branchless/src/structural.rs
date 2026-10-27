//! JSON structural / escape character masks and URL percent-encoding helpers.
//!
//! Ported from bcinr `algorithms/json_find_string_escapes_simd.rs:22`,
//! `algorithms/json_find_structural_simd.rs:22`,
//! `algorithms/url_encode_branchless.rs:23`, and
//! `algorithms/url_decode_branchless.rs:28`. The single-word lane kernels are
//! verbatim in spirit (scaffold ABI stripped); the slice-level chunked
//! wrappers are this port's addition — bcinr's bodies process one `u64` word
//! per call and never touch a slice.
//!
//! Dedupe note: [`crate::swar::find_byte_mask_word`] is the canonical
//! carry-safe char-equality kernel; the word kernels below are
//! predicate-specialized compositions over it (needle sets OR-ed, gated by a
//! SWAR non-zero active mask), not independent equality implementations.

/// Bit mask over the (little-endian) bytes of `word` marking lanes equal to
/// `\\` (0x5C) or `"` (0x22), AND-gated by the per-lane active mask in
/// `active` (lane active iff its byte is non-zero).
///
/// Bit `i` of the result is set iff byte `i` of `word` is escape-relevant and
/// byte `i` of `active` is non-zero.
///
/// from bcinr algorithms/json_find_string_escapes_simd.rs:22 (lane kernel);
/// recomposed over [`crate::swar::find_byte_mask_word`] (the canonical
/// carry-safe char-equality kernel) instead of the bcinr per-lane closure.
#[inline]
#[must_use]
pub fn escape_mask_word(word: u64, active: u64) -> u64 {
    // Lane-active gate in SWAR: MSB set iff the lane byte is non-zero
    // ((x & LO7) + LO7 | x raises the MSB exactly when x != 0).
    let nonzero = ((active & 0x7F7F_7F7F_7F7F_7F7F).wrapping_add(0x7F7F_7F7F_7F7F_7F7F) | active)
        & 0x8080_8080_8080_8080;
    (crate::swar::find_byte_mask_word(word, b'\\') | crate::swar::find_byte_mask_word(word, b'"'))
        & nonzero
}

/// Bit mask over the (little-endian) bytes of `word` marking lanes holding a
/// JSON structural character — `{` `}` `[` `]` `:` `,` — AND-gated by the
/// per-lane active mask (lane active iff its byte is non-zero), the simdjson
/// stage-1 shape.
///
/// from bcinr algorithms/json_find_structural_simd.rs:22 (lane kernel);
/// recomposed over [`crate::swar::find_byte_mask_word`] (the canonical
/// carry-safe char-equality kernel) instead of the bcinr per-lane closure.
#[inline]
#[must_use]
pub fn structural_mask_word(word: u64, active: u64) -> u64 {
    let nonzero = ((active & 0x7F7F_7F7F_7F7F_7F7F).wrapping_add(0x7F7F_7F7F_7F7F_7F7F) | active)
        & 0x8080_8080_8080_8080;
    b"{}[]:,"
        .iter()
        .copied()
        .map(|c| crate::swar::find_byte_mask_word(word, c))
        .fold(0, |acc, m| acc | m)
        & nonzero
}

/// Position bit mask of escape-relevant bytes (`\\` or `"`) over the first
/// `bytes.len().min(64)` bytes of `bytes` — bit `i` set iff `bytes[i]` is
/// escape-relevant.
///
/// Slice wrapper (this port's addition; bcinr processes one word per call).
/// Inputs longer than 64 bytes are capped: only the first 64 positions are
/// represented. Zero allocation, constant-shape per chunk.
#[must_use]
pub fn escape_mask(bytes: &[u8]) -> u64 {
    let mut mask = 0u64;
    let mut bit = 0u64;
    let (chunks, tail) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_le_bytes(*chunk);
        mask |= shift_mask(escape_mask_word(word, u64::MAX), bit);
        bit += 8;
    }
    // `bit < 64` enforces the documented 64-position cap: a tail that starts
    // at or past position 64 is outside the representable window entirely.
    if !tail.is_empty() && bit < 64 {
        let mut word = [0u8; 8];
        word[..tail.len()].copy_from_slice(tail);
        // Padded zero lanes can never match a non-zero delimiter, so the
        // gate is fully open; this avoids a per-byte active-mask subtlety.
        mask |= shift_mask(escape_mask_word(u64::from_le_bytes(word), u64::MAX), bit);
    }
    mask
}

/// Position bit mask of JSON structural bytes (`{` `}` `[` `]` `:` `,`) over
/// the first `bytes.len().min(64)` bytes of `bytes` — bit `i` set iff
/// `bytes[i]` is structural.
///
/// Slice wrapper (this port's addition); capped at 64 positions like
/// [`escape_mask`].
#[must_use]
pub fn structural_mask(bytes: &[u8]) -> u64 {
    let mut mask = 0u64;
    let mut bit = 0u64;
    let (chunks, tail) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_le_bytes(*chunk);
        mask |= shift_mask(structural_mask_word(word, u64::MAX), bit);
        bit += 8;
    }
    if !tail.is_empty() && bit < 64 {
        let mut word = [0u8; 8];
        word[..tail.len()].copy_from_slice(tail);
        mask |= shift_mask(
            structural_mask_word(u64::from_le_bytes(word), u64::MAX),
            bit,
        );
    }
    mask
}

// Per-8-byte-lane MSB masks (bit 7 of each lane) must be re-packed into
// sequential position bits: lane 0 keeps bit 7 -> position bit 0, etc.
#[inline]
fn shift_mask(lane_msb: u64, bit: u64) -> u64 {
    // MSB-compress: each lane's bit 7 (bit 8k+7) maps to bit k of the
    // compressed byte via the delta-encoding constant (bit 8k+7 + bit 7-7k
    // sums into bit 56+k of the product), then shifts into place.
    let compressed = (lane_msb.wrapping_mul(0x0002_0408_1020_4081) >> 56) & 0xFF;
    compressed << bit
}

/// Percent-encode one byte into packed ASCII lanes: `0x25` (`%`), high hex
/// digit, low hex digit — byte 0 is `%`, byte 1 the uppercase high nibble,
/// byte 2 the uppercase low nibble.
///
/// **Scalar-per-call helper** (one byte per invocation; callers batch). The
/// hex-digit alpha correction is a single comparison-mask, not a branch.
///
/// from bcinr algorithms/url_encode_branchless.rs:23 (scaffold `val + aux`
/// input replaced by an explicit byte parameter).
#[inline]
#[must_use]
pub fn url_encode_byte(b: u8) -> u32 {
    fn hex(nibble: u32) -> u32 {
        let n = nibble & 0xF;
        // mask = 1 iff n > 9 (needs 'A'..'F')
        let alpha = 9u32.wrapping_sub(n) >> 31;
        (u32::from(b'0')) + n + alpha.wrapping_mul(7)
    }
    let b = u32::from(b);
    u32::from(b'%') | (hex(b >> 4) << 8) | (hex(b & 0xF) << 16)
}

/// Decode the two hex digits packed in bytes 1 and 2 of a `%HH` triple into
/// one byte value. Bytes are taken at fixed offsets 1 (high nibble) and 2
/// (low nibble), matching the [`url_encode_byte`] packing.
///
/// **Scalar-per-call helper** (one `%HH` triple per invocation).
///
/// from bcinr algorithms/url_decode_branchless.rs:28 (scaffold `val + aux`
/// input replaced by an explicit `%HH` triple).
#[inline]
#[must_use]
pub fn url_decode_triple(triple: [u8; 3]) -> u8 {
    fn unhex(c: u8) -> u8 {
        (c & 0xF) + 9 * ((c >> 6) & 1)
    }
    (unhex(triple[1]) << 4) | unhex(triple[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent per-byte oracles (deliberately different formulation).
    fn escape_reference(bytes: &[u8]) -> u64 {
        bytes
            .iter()
            .enumerate()
            .take(64)
            .fold(0u64, |acc, (i, &b)| {
                if b == b'\\' || b == b'"' {
                    acc | (1 << i)
                } else {
                    acc
                }
            })
    }

    fn structural_reference(bytes: &[u8]) -> u64 {
        bytes
            .iter()
            .enumerate()
            .take(64)
            .fold(0u64, |acc, (i, &b)| {
                if matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
                    acc | (1 << i)
                } else {
                    acc
                }
            })
    }

    #[test]
    fn escape_mask_word_exhaustive_bytes() {
        // Every byte value, in every lane position, against the oracle.
        for b in 0..=255u8 {
            let word = u64::from_ne_bytes([b; 8]);
            let got = escape_mask_word(word, u64::MAX);
            let want = if b == b'\\' || b == b'"' {
                0x8080_8080_8080_8080
            } else {
                0
            };
            assert_eq!(got, want, "byte {b:#04x}");
        }
    }

    #[test]
    fn structural_mask_word_exhaustive_bytes() {
        const STRUCTURAL: [u8; 6] = *b"{}[]:,";
        for b in 0..=255u8 {
            let word = u64::from_ne_bytes([b; 8]);
            let got = structural_mask_word(word, u64::MAX);
            let want = if STRUCTURAL.contains(&b) {
                0x8080_8080_8080_8080
            } else {
                0
            };
            assert_eq!(got, want, "byte {b:#04x}");
        }
    }

    #[test]
    fn active_gate_respects_zero_lanes() {
        // Zero bytes in `active` suppress their lanes even on hits.
        assert_eq!(escape_mask_word(0x2222_2222_2222_2222, 0), 0);
        assert_eq!(structural_mask_word(0x3A3A_3A3A_3A3A_3A3A, 0), 0);
        // Only lane 3 active.
        let active = 0x0000_0000_00FF_0000u64;
        assert_eq!(escape_mask_word(0x5C5C_5C5C_5C5C_5C5C, active), 0x0080_0000);
    }

    #[test]
    fn boundary_words_match_oracle() {
        for &(word, active) in &[
            (0u64, 0u64),
            (u64::MAX, u64::MAX),
            (u64::MAX, 0u64),
            (0u64, u64::MAX),
            (0x225C_7B7D_3A2C_2231, u64::MAX),
        ] {
            let w = word.to_le_bytes();
            let a = active.to_le_bytes();
            let want_esc = {
                let mut m = 0u64;
                for i in 0..8 {
                    if a[i] != 0 && (w[i] == b'\\' || w[i] == b'"') {
                        m |= 0x80 << (8 * i);
                    }
                }
                m
            };
            assert_eq!(escape_mask_word(word, active), want_esc);
        }
    }

    #[test]
    fn slice_wrappers_match_oracle() {
        let cases: [&[u8]; 7] = [b"", b"a", b"ab", b"abc", b"abcd", b"abcde", b"abcdefg"];
        for case in cases {
            assert_eq!(escape_mask(case), escape_reference(case), "{case:?}");
            assert_eq!(
                structural_mask(case),
                structural_reference(case),
                "{case:?}"
            );
        }
        // Exactly one word.
        assert_eq!(escape_mask(b"he\\lo wo"), escape_reference(b"he\\lo wo"));
        // Multi-word with tail, hits past byte 8.
        let long = b"plain text {a:1} then \"quoted\" tail\\";
        assert_eq!(escape_mask(long), escape_reference(long));
        assert_eq!(structural_mask(long), structural_reference(long));
        // 64-byte cap documented: hits at position 64+ are not represented.
        let over = [b'x'; 65];
        assert_eq!(escape_mask(&over), 0);
    }

    #[test]
    fn url_round_trip_all_bytes() {
        for b in 0..=255u8 {
            let packed = url_encode_byte(b);
            assert_eq!(packed & 0xFF, u32::from(b'%'));
            let triple = [
                (packed & 0xFF) as u8,
                (packed >> 8) as u8,
                (packed >> 16) as u8,
            ];
            // Uppercase hex digits only.
            for c in [triple[1], triple[2]] {
                assert!(
                    c.is_ascii_digit() || (b'A'..=b'F').contains(&c),
                    "{b:#04x} -> {c}"
                );
            }
            assert_eq!(url_decode_triple(triple), b, "round trip {b:#04x}");
        }
    }

    #[test]
    fn url_encode_known_vectors() {
        // Direct packing checks (byte0 '%', byte1 hi, byte2 lo).
        let p = url_encode_byte(0xAF);
        assert_eq!((p & 0xFF) as u8, b'%');
        assert_eq!(((p >> 8) & 0xFF) as u8, b'A');
        assert_eq!(((p >> 16) & 0xFF) as u8, b'F');
        let p = url_encode_byte(0x05);
        assert_eq!(((p >> 8) & 0xFF) as u8, b'0');
        assert_eq!(((p >> 16) & 0xFF) as u8, b'5');
    }
}

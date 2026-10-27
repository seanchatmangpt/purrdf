//! Cross-module integration tests for `purrdf-branchless`.
//!
//! Chicago-style: every assertion runs a ported primitive against an
//! independent reference formulation (scalar loops / std sort / u128
//! reference). Deterministic, dependency-free.

use purrdf_branchless as b;

/// Deterministic xorshift64* — no external fuzz dependency.
struct Xorshift(u64);
impl Xorshift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn rand_array<const N: usize>(rng: &mut Xorshift, lo: u32, hi: u32) -> [u32; N] {
    // widen to u64 first: hi - lo + 1 overflows u32 when lo=0, hi=u32::MAX
    let span = u64::from(hi) - u64::from(lo) + 1;
    std::array::from_fn(|_| lo + (rng.next() % span) as u32)
}

#[test]
fn median3_agrees_with_sort_network() {
    let mut rng = Xorshift(0x2026_1026);
    for _ in 0..10_000 {
        let (a, b, c) = (rng.next() as u32, rng.next() as u32, rng.next() as u32);
        let mut v = [a, b, c];
        v.sort_unstable();
        assert_eq!(b::select::median3_u32(a, b, c), v[1]);
    }
}

#[test]
fn sort8_and_sort16_vs_std_sort() {
    let mut rng = Xorshift(0x5EED_0001);
    for _ in 0..5_000 {
        let mut v: [u32; 8] = rand_array(&mut rng, 0, u32::MAX);
        let mut want = v;
        want.sort_unstable();
        b::sort_small::sort8(&mut v);
        assert_eq!(v, want);
    }
    for _ in 0..2_000 {
        let mut v: [u32; 16] = rand_array(&mut rng, 0, 255);
        let mut want = v;
        want.sort_unstable();
        v = b::sort16::batcher_sort16_u32(v);
        assert_eq!(v, want);
    }
    // adversarial shapes
    for mut v in [
        std::array::from_fn::<u32, 8, _>(|i| 7 - i as u32),
        [0u32; 8],
        [u32::MAX; 8],
    ] {
        let mut want = v;
        want.sort_unstable();
        b::sort_small::sort8(&mut v);
        assert_eq!(v, want);
    }
}

#[test]
fn eytzinger_end_to_end_agrees_with_binary_search() {
    for n in [0usize, 1, 7, 64, 1000] {
        let sorted: Vec<u64> = (0..n)
            .map(|i| (i as u64).wrapping_mul(3).wrapping_add(1))
            .collect();
        let mut layout = vec![0u64; n + 1];
        if n > 0 {
            b::eytzinger::build_eytzinger_layout(&sorted, &mut layout);
        }
        let mut perm = vec![0usize; n + 1]; // 1-indexed: perm[0] unused per API doc
        let mut sizes = vec![0u32; n + 1];
        // Module convention: `len` = layout length (n + 1), matching the
        // module's own tests; sizes is 1-indexed over layout slots.
        b::eytzinger::build_layout_permutation(&sorted, &mut perm);
        b::eytzinger::build_subtree_sizes(n + 1, &mut sizes);
        let mut rng = Xorshift(0xE771_6E77 ^ n as u64);
        for _ in 0..2_000.min(200 * (n + 2)) {
            let target = rng.next();
            let want = sorted.binary_search(&target).ok();
            let got = b::eytzinger::search_eytzinger_u64(&layout, target).map(|slot| perm[slot]);
            assert_eq!(got, want, "search n={n} target={target}");
            let lb = b::eytzinger::lower_bound_eytzinger(&layout, &sizes, target);
            let want_lb = sorted.partition_point(|x| *x < target);
            assert_eq!(lb, want_lb, "lower_bound n={n} eytzinger n={n}");
        }
    }
}

#[test]
fn mul_sat_u64_agrees_with_u128_reference() {
    let mut rng = Xorshift(0x6C95_0002);
    let pairs: [(u64, u64); 5] = [
        (u64::MAX, u64::MAX),
        (u64::MAX, 1),
        (0, u64::MAX),
        (1u64 << 32, 1u64 << 32),
        ((1u64 << 32) - 1, (1u64 << 32) + 1),
    ];
    for (a, c) in pairs {
        let want = (u128::from(a) * u128::from(c)).min(u128::from(u64::MAX)) as u64;
        assert_eq!(b::saturation::mul_sat_u64(a, c), want);
    }
    for _ in 0..10_000 {
        let a = rng.next();
        let c = rng.next();
        let want = (u128::from(a) * u128::from(c)).min(u128::from(u64::MAX)) as u64;
        assert_eq!(b::saturation::mul_sat_u64(a, c), want);
    }
}

#[test]
fn wyhash_deterministic_and_avalanche_sane() {
    let inputs: Vec<Vec<u8>> = (0u64..64)
        .map(|i| {
            let n = (i % 37) as usize;
            (0..n).map(|j| i.wrapping_mul(j as u64 + 1) as u8).collect()
        })
        .collect();
    let h1: Vec<u64> = inputs.iter().map(|d| b::hash::wyhash_64(d, 0)).collect();
    let h2: Vec<u64> = inputs.iter().map(|d| b::hash::wyhash_64(d, 0)).collect();
    assert_eq!(h1, h2, "wyhash determinism");
    let base = b::hash::wyhash_64(&[0xAB_u8; 32], 0);
    for byte in 0..32usize {
        let mut d = vec![0xAB_u8; 32];
        d[byte] ^= 0x01;
        let diff = (base ^ b::hash::wyhash_64(&d, 0)).count_ones();
        assert!((12..=52).contains(&diff), "avalanche band: {diff}");
    }
}

#[test]
fn bloom_and_linear_counting_sanity() {
    let mut word = 0u64;
    for i in 0u64..64 {
        word = b::filter::bloom_add(word, i);
        assert!(b::filter::bloom_query(i, word), "bloom must admit {i}");
    }
    let mut bm = [0u64; 4];
    for i in 0u64..64 {
        b::bitmap::linear_counting_add(&mut bm, i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
    let est = b::bitmap::linear_counting_estimate(&bm);
    assert!((40..=64).contains(&est), "estimate sanity: {est}");
}

#[test]
fn swar_find_all_positions_matches_scalar_reference() {
    for len in 0..=64usize {
        let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
        for needle in [0u8, 11, 37, 255] {
            let mut want = 0u64;
            for (i, x) in data.iter().enumerate() {
                if *x == needle {
                    want |= 1 << i;
                }
            }
            assert_eq!(b::swar::swar_find_all_positions(&data, needle), want);
        }
    }
}

// ---------------------------------------------------------------------------
// Dedupe-consolidation regression pins: the module kernels were consolidated
// onto single homes; these tests pin the shared behavior from the consumer
// side so a delegation regression fails loudly.
// ---------------------------------------------------------------------------

/// Scalar per-byte reference for `classify_bytes` (5 flag bits per lane,
/// LSB-first flag order per the swar kernel: digit, alpha, ws, upper, lower).
fn classify_reference(word: u64) -> u64 {
    let mut out = 0u64;
    for lane in 0..8usize {
        let b = ((word >> (lane * 8)) & 0xFF) as u8;
        let is_digit = b.is_ascii_digit();
        let is_alpha = b.is_ascii_alphabetic();
        let is_ws = matches!(b, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ');
        let is_upper = b.is_ascii_uppercase();
        let is_lower = b.is_ascii_lowercase();
        let mut flags = 0u64;
        if is_digit {
            flags |= 1;
        }
        if is_alpha {
            flags |= 1 << 1;
        }
        if is_ws {
            flags |= 1 << 2;
        }
        if is_upper {
            flags |= 1 << 3;
        }
        if is_lower {
            flags |= 1 << 4;
        }
        out |= flags << (lane * 8);
    }
    out
}

#[test]
fn classify_bytes_matches_scalar_reference() {
    let mut rng = Xorshift(0xC1A5_5F01);
    let adversarial: [u64; 20] = [
        0,
        u64::MAX,
        u64::from_le_bytes(*b"0Aa \t\r~\x00"),
        u64::from_le_bytes(*b"ZZZZZZZZ"),
        u64::from_le_bytes(*b"aaaaaaaa"),
        u64::from_le_bytes(*b"99999999"),
        u64::from_le_bytes(*b" \t\n\r\x0B\x0C  "),
        u64::from_le_bytes(*b"Aa0Zz9 ~"),
        0x7F7F_7F7F_7F7F_7F7F,
        0x8080_8080_8080_8080,
        u64::from_le_bytes(*b"\x00\x1F\x20\x7F\x80\xFF\x41\x61"),
        1,
        1 << 63,
        u64::from_le_bytes(*b"        "),
        u64::from_le_bytes(*b"0000000a"),
        u64::from_le_bytes(*b"a0000000"),
        u64::from_le_bytes(*b"~~0Aa0~\x7F"),
        u64::from_le_bytes(*b"\x20\x20\x41\x20\x20\x61\x20\x20"),
        0x5555_5555_5555_5555,
        0xAAAA_AAAA_AAAA_AAAA,
    ];
    for w in adversarial {
        assert_eq!(
            b::swar::classify_bytes(w),
            classify_reference(w),
            "swar::classify_bytes {w:#x}"
        );
        // char_class re-exports the consolidated single home; same behavior.
        assert_eq!(
            b::char_class::classify_bytes(w),
            classify_reference(w),
            "char_class::classify_bytes {w:#x}"
        );
    }
    for _ in 0..2_000 {
        let w = rng.next();
        assert_eq!(b::swar::classify_bytes(w), classify_reference(w));
        assert_eq!(b::char_class::classify_bytes(w), classify_reference(w));
    }
}

#[test]
fn parse_8_decimal_digits_shared_behavior() {
    let mut rng = Xorshift(0xDE_C1A5);
    let valid: [(u64, u32); 4] = [
        (u64::from_le_bytes(*b"12345678"), 12_345_678),
        (u64::from_le_bytes(*b"00000000"), 0),
        (u64::from_le_bytes(*b"99999999"), 99_999_999),
        (u64::from_le_bytes(*b"70000000"), 70_000_000),
    ];
    for (w, want) in valid {
        assert_eq!(b::swar::parse_8_decimal_digits(w), Some(want));
        assert_eq!(b::char_class::parse_8_decimal_digits(w), Some(want));
    }
    // Any non-digit lane makes both reject identically.
    for _ in 0..4_000 {
        let w = rng.next();
        let a = b::swar::parse_8_decimal_digits(w);
        let c = b::char_class::parse_8_decimal_digits(w);
        assert_eq!(a, c, "shared parse behavior {w:#x}");
        if let Some(v) = a {
            // round-trip check against a scalar fold
            let bytes = w.to_le_bytes();
            let want = (0..8).fold(0u64, |acc, i| acc * 10 + u64::from(bytes[i] - b'0'));
            assert_eq!(u64::from(v), want);
        }
    }
}

#[test]
fn bitmap_word_kernels_agree_with_bitset_conventions() {
    let mut rng = Xorshift(0xB17_5E7);
    // Exhaustive single-bit words.
    for bit in 0..64usize {
        let w = 1u64 << bit;
        for pos in [0usize, bit, 63] {
            // bitmap::rank_word is EXCLUSIVE (bits < pos); bitset::rank_u64 is
            // INCLUSIVE (bits <= pos).
            let want_excl = if pos == 0 { 0 } else { u64::from(pos > bit) };
            assert_eq!(
                b::bitmap::rank_word(w, pos),
                want_excl,
                "rank_word {w:#x} pos {pos}"
            );
            assert_eq!(
                b::bitset::rank_u64(w, pos),
                usize::from(pos >= bit),
                "rank_u64 {w:#x} pos {pos}"
            );
        }
        // select: sentinel vs Option.
        assert_eq!(b::bitmap::select_word(w, 0), bit as u64);
        assert_eq!(b::bitmap::select_word(w, 1), 64);
        assert_eq!(b::bitset::select_bit_u64(w, 0), Some(bit));
        assert_eq!(b::bitset::select_bit_u64(w, 1), None);
    }
    // Random-word cross-checks.
    for _ in 0..2_000 {
        let w = rng.next();
        let pos = (rng.next() % 64) as usize;
        let excl_ref = (0..pos).filter(|i| w >> i & 1 == 1).count() as u64;
        assert_eq!(
            b::bitmap::rank_word(w, pos),
            excl_ref,
            "rank_word {w:#x} pos {pos}"
        );
        let incl_ref = (0..=pos).filter(|i| w >> i & 1 == 1).count();
        assert_eq!(b::bitset::rank_u64(w, pos), incl_ref);
        let n = rng.next() % 65;
        let want_sel = {
            let mut seen = 0u64;
            let mut ans = 64u64;
            for i in 0..64 {
                if w >> i & 1 == 1 {
                    if seen == n {
                        ans = i as u64;
                        break;
                    }
                    seen += 1;
                }
            }
            ans
        };
        assert_eq!(b::bitmap::select_word(w, n), want_sel);
        assert_eq!(
            b::bitset::select_bit_u64(w, n as usize),
            if want_sel == 64 {
                None
            } else {
                Some(want_sel as usize)
            }
        );
    }
}

#[test]
fn deposit_bits_matches_pdep() {
    let mut rng = Xorshift(0x0FDE_9AD5);
    assert_eq!(b::packing::deposit_bits(0xFFFF_FFFF, u64::MAX), 0xFFFF_FFFF);
    assert_eq!(b::packing::deposit_bits(0, 0x1234), 0);
    for _ in 0..5_000 {
        let val = rng.next();
        let mask = rng.next();
        assert_eq!(
            b::packing::deposit_bits(val, mask),
            b::simd_fallback::pdep_u64(val, mask),
            "pdep {val:#x} {mask:#x}"
        );
    }
}

#[test]
fn first_match_byte_agrees_with_swar() {
    let mut rng = Xorshift(0x0F1B_57A1);
    for _ in 0..5_000 {
        let w = rng.next();
        let byte = (rng.next() & 0xFF) as u8;
        assert_eq!(
            b::search::first_match_byte_u64(w, byte),
            b::swar::first_byte_position(w, byte).map(|p| p as usize),
            "first-match {w:#x} byte {byte}"
        );
    }
    // Multi-match: both must report the LOWEST matching lane.
    let w = u64::from_le_bytes(*b"aXaXaXaX");
    assert_eq!(b::search::first_match_byte_u64(w, b'a'), Some(0));
    assert_eq!(b::swar::first_byte_position(w, b'a'), Some(0));
}

#[test]
fn ct_min_u32_equals_select_min_u32() {
    let mut rng = Xorshift(0x617_4A10);
    for (a, c) in [
        (0u32, 0u32),
        (u32::MAX, 0),
        (0, u32::MAX),
        (u32::MAX, u32::MAX),
        (7, 9),
        (9, 7),
    ] {
        assert_eq!(b::simd_fallback::ct_min_u32(a, c), b::select::min_u32(a, c));
    }
    for _ in 0..5_000 {
        let a = rng.next() as u32;
        let c = rng.next() as u32;
        assert_eq!(
            b::simd_fallback::ct_min_u32(a, c),
            b::select::min_u32(a, c),
            "ct_min {a} {c}"
        );
    }
}

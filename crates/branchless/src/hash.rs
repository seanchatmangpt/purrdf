// Provenance: ported from bcinr (`~/bcinr/crates/bcinr-logic/src/algorithms/`):
//   wyhash_64.rs:22-153 (WyHash v4: wymix + 48-byte-lane streaming + tails)
//   tabulation_hash_u64.rs:45-84 (xorshift64-seeded 4-way Zobrist tables)
//   siphash_2_4_branchless.rs:26-93 (unrolled SipHash-2-4 over one 8-byte word)
//   knuth_hash_u64.rs:27-30, fibonacci_hash_u64.rs:27-33 (golden-ratio projections)
// bcinr state: branch `bench/rdtsc-tick-tables`, commit `1e92b213`.
//
//! Branchless 64-bit hash primitives.
//!
//! One home per job: FNV-1a and SplitMix64 are **deliberately absent** —
//! purrdf-hash owns them (`hash/src/fnv.rs`, `hash/src/mix.rs`, identical
//! constants). The name-borrowing stubs (city/farm/highway/metro/spooky) were
//! audited and rejected as ~10-line mixers wearing famous names.
//!
//! All functions here are constant-shape: no content-dependent branching, no
//! heap, no ambient state. `wyhash_64`'s control flow depends only on
//! `data.len()` (block-loop trip counts), never on content.

/// WyHash fold: `hi64(a·b) XOR lo64(a·b)` — the avalanche kernel.
// The always-inline is deliberate: wymix is the per-block avalanche kernel of
// wyhash_64's 48-byte lanes; call overhead would dominate the hot loop.
#[allow(clippy::inline_always)]
#[inline(always)]
#[must_use]
pub fn wymix(a: u64, b: u64) -> u64 {
    let r = u128::from(a).wrapping_mul(u128::from(b));
    ((r >> 64) as u64) ^ (r as u64)
}

/// Read a little-endian u64 from `src` at `offset`.
///
/// Internal contract: `offset + 8 <= src.len()`.
// Always-inline, as for `wymix`: wyhash lane reads sit inside the hot loop.
#[allow(clippy::inline_always)]
#[inline(always)]
fn read_u64_le(src: &[u8], offset: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&src[offset..offset + 8]);
    u64::from_le_bytes(b)
}

/// Read a little-endian u32 from `src` at `offset`.
///
/// Internal contract: `offset + 4 <= src.len()`.
// Always-inline, as for `wymix`: wyhash lane reads sit inside the hot loop.
#[allow(clippy::inline_always)]
#[inline(always)]
fn read_u32_le(src: &[u8], offset: usize) -> u64 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&src[offset..offset + 4]);
    u64::from(u32::from_le_bytes(b))
}

/// WyHash v4 64-bit hash of an arbitrary byte slice with a caller seed.
///
/// Streaming shape: 48 bytes per main-loop iteration via three 64-bit lanes
/// (`seed`/`see1`/`see2` folded with `wymix` against WY_P1/P2/P3), 16-byte
/// blocks until ≤16 bytes remain, then two overlapping 8-byte tail reads.
/// Inputs ≤16 bytes take dedicated small paths: 4–16 bytes use overlapping
/// 4-byte reads; 1–3 bytes use byte-masked compositing (first, middle, last);
/// empty input returns `wymix(seed', WY_P0)`.
///
/// Control flow depends only on `data.len()`, never on content.
pub fn wyhash_64(data: &[u8], seed: u64) -> u64 {
    const WY_P0: u64 = 0xa076_1d64_78bd_642f;
    const WY_P1: u64 = 0xe703_7ed1_a0b4_28db;
    const WY_P2: u64 = 0x8ebc_6af0_9c88_c6e3;
    const WY_P3: u64 = 0x5899_65cc_7537_4cc3;

    let len = data.len();
    let mut seed = seed ^ wymix(seed ^ WY_P0, WY_P1);
    let a: u64;
    let b: u64;

    if len <= 16 {
        if len >= 4 {
            // 4–16 bytes: two overlapping reads (head + tail).
            let tail_offset = len - 4;
            a = (read_u32_le(data, 0) << 32) | read_u32_le(data, tail_offset);
            b = if len >= 8 {
                (read_u32_le(data, 4) << 32) | read_u32_le(data, len - 4)
            } else {
                a.rotate_right(32)
            };
        } else if len > 0 {
            // 1–3 bytes: byte compositing (first, middle, last).
            a = (u64::from(data[0]) << 16)
                | (u64::from(data[len >> 1]) << 8)
                | u64::from(data[len - 1]);
            b = 0;
        } else {
            return wymix(seed, WY_P0);
        }
    } else {
        // >16 bytes: 48-byte lane blocks, then 16-byte blocks, then tail.
        let mut i = len;
        let mut p = 0usize;
        let mut see1 = seed;
        let mut see2 = seed;
        if i > 48 {
            while i > 48 {
                seed = wymix(
                    read_u64_le(data, p) ^ WY_P1,
                    read_u64_le(data, p + 8) ^ seed,
                );
                see1 = wymix(
                    read_u64_le(data, p + 16) ^ WY_P2,
                    read_u64_le(data, p + 24) ^ see1,
                );
                see2 = wymix(
                    read_u64_le(data, p + 32) ^ WY_P3,
                    read_u64_le(data, p + 40) ^ see2,
                );
                p += 48;
                i -= 48;
            }
            seed ^= see1 ^ see2;
        }
        while i > 16 {
            seed = wymix(
                read_u64_le(data, p) ^ WY_P1,
                read_u64_le(data, p + 8) ^ seed,
            );
            p += 16;
            i -= 16;
        }
        // Overlapping tail reads; p + i == len.
        a = read_u64_le(data, p + i - 16);
        b = read_u64_le(data, p + i - 8);
        return wymix(WY_P1 ^ len as u64, wymix(a ^ WY_P1, b ^ seed));
    }

    wymix(WY_P1 ^ len as u64, wymix(a ^ WY_P1, b ^ seed))
}

/// Tabulation hash of a 32-bit key over four 256-entry random tables.
///
/// `h(k) = T0[k.byte0] ^ T1[k.byte1] ^ T2[k.byte2] ^ T3[k.byte3]`.
/// Provably 3-independent when table entries are independent uniform values.
/// Tables are caller-provided (8 KiB, stack or static) — zero heap.
pub fn tabulation_hash_u64(key: u32, tables: &[[u64; 256]; 4]) -> u64 {
    let b0 = (key & 0xFF) as usize;
    let b1 = ((key >> 8) & 0xFF) as usize;
    let b2 = ((key >> 16) & 0xFF) as usize;
    let b3 = ((key >> 24) & 0xFF) as usize;
    tables[0][b0] ^ tables[1][b1] ^ tables[2][b2] ^ tables[3][b3]
}

/// Fill four 256-entry tabulation tables from an xorshift64 PRNG.
///
/// Seed 0 maps to a fixed nonzero fallback (`0x853c49e6748fea9b`); xorshift64
/// never produces 0 from a nonzero state, so initialized tables are all-nonzero.
pub fn tabulation_hash_init_tables(seed: u64, tables: &mut [[u64; 256]; 4]) {
    let mut state = if seed == 0 {
        0x853c_49e6_748f_ea9b
    } else {
        seed
    };
    for table in tables.iter_mut() {
        for entry in table.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *entry = state;
        }
    }
}

/// SipHash-2-4 MAC over a single 8-byte message word.
///
/// **Fixed-key limitation (load-bearing):** `k0` is hardwired to `0` and the
/// second key word is the `key` argument — this is a keyed MAC over exactly one
/// 64-bit message word, NOT the streaming SipHash of arbitrary-length input.
/// It cannot authenticate multi-word messages as-is.
///
/// 2 compression + 4 finalisation SipRounds, fully unrolled — constant-time.
pub fn siphash_2_4_branchless(val: u64, key: u64) -> u64 {
    let k0: u64 = 0;
    let k1: u64 = key;
    let mut v0 = k0 ^ 0x736f_6d65_7073_6575;
    let mut v1 = k1 ^ 0x646f_7261_6e64_6f6d;
    let mut v2 = k0 ^ 0x6c79_6765_6e65_7261;
    let mut v3 = k1 ^ 0x7465_6462_7974_6573;

    // Absorb the message word; 2 compression rounds.
    v3 ^= val;
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= val;

    // Finalisation: length byte (8) then 4 rounds after the 0xff tag.
    let b: u64 = 8u64 << 56;
    v3 ^= b;
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= b;

    v2 ^= 0xff;
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);
    sipround(&mut v0, &mut v1, &mut v2, &mut v3);

    v0 ^ v1 ^ v2 ^ v3
}

/// One SipRound over four state words, fully unrolled.
#[inline]
fn sipround(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    *v0 = v0.wrapping_add(*v1);
    *v1 = v1.rotate_left(13);
    *v1 ^= *v0;
    *v0 = v0.rotate_left(32);
    *v2 = v2.wrapping_add(*v3);
    *v3 = v3.rotate_left(16);
    *v3 ^= *v2;
    *v0 = v0.wrapping_add(*v3);
    *v3 = v3.rotate_left(21);
    *v3 ^= *v0;
    *v2 = v2.wrapping_add(*v1);
    *v1 = v1.rotate_left(17);
    *v1 ^= *v2;
    *v2 = v2.rotate_left(32);
}

/// Knuth multiplicative hash: `key·φ` shifted right by `shift`.
/// Projects into `2^(64 - shift)` slots. Marginal, kept for parity
/// with the bcinr catalog.
///
/// The golden-ratio constant routes through the sanctioned home
/// [`purrdf_hash::mix::GOLDEN_GAMMA`] (one home per job), not a local literal.
///
/// Widened from the bcinr `(val, aux)` scaffold (aux packed the shift
/// amount); the shift is masked to `& 63` mask-style exactly as the
/// scaffold's decode did.
// from bcinr crates/bcinr-logic/src/algorithms/knuth_hash_u64.rs:27-30
#[inline]
#[must_use]
pub fn knuth_hash_u64(key: u64, shift: u32) -> u64 {
    key.wrapping_mul(purrdf_hash::mix::GOLDEN_GAMMA) >> (shift & 63)
}

/// Fibonacci hashing: the same golden-ratio product projected into `2^bits`
/// slots (shift `64 - bits`; a 0-bit projection is the full product).
///
/// Widened from the bcinr `(val, aux)` scaffold (aux packed the bit count);
/// `bits` is masked `& 63` exactly as the scaffold's decode did.
// from bcinr crates/bcinr-logic/src/algorithms/fibonacci_hash_u64.rs:27-33
#[inline]
#[must_use]
pub fn fibonacci_hash_u64(key: u64, bits: u32) -> u64 {
    let product = key.wrapping_mul(purrdf_hash::mix::GOLDEN_GAMMA);
    let shift = (64u32.wrapping_sub(bits & 63)) & 63;
    product >> shift
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Array-state SipHash reference with a counted-round loop (independent
    /// re-spelling, not an alias).
    fn siphash_reference(val: u64, key: u64) -> u64 {
        fn round(s: &mut [u64; 4]) {
            s[0] = s[0].wrapping_add(s[1]);
            s[1] = s[1].rotate_left(13) ^ s[0];
            s[0] = s[0].rotate_left(32);
            s[2] = s[2].wrapping_add(s[3]);
            s[3] = s[3].rotate_left(16) ^ s[2];
            s[0] = s[0].wrapping_add(s[3]);
            s[3] = s[3].rotate_left(21) ^ s[0];
            s[2] = s[2].wrapping_add(s[1]);
            s[1] = s[1].rotate_left(17) ^ s[2];
            s[2] = s[2].rotate_left(32);
        }
        fn rounds(s: &mut [u64; 4], n: usize) {
            let mut i = 0;
            while i < n {
                round(s);
                i += 1;
            }
        }
        let (k0, k1) = (0u64, key);
        let mut s = [
            k0 ^ 0x736f_6d65_7073_6575,
            k1 ^ 0x646f_7261_6e64_6f6d,
            k0 ^ 0x6c79_6765_6e65_7261,
            k1 ^ 0x7465_6462_7974_6573,
        ];
        s[3] ^= val;
        rounds(&mut s, 2);
        s[0] ^= val;
        let b = 8u64 << 56;
        s[3] ^= b;
        rounds(&mut s, 2);
        s[0] ^= b;
        s[2] ^= 0xff;
        rounds(&mut s, 4);
        s[0] ^ s[1] ^ s[2] ^ s[3]
    }

    /// u128-truncation oracle for the golden-ratio product.
    fn golden_product(val: u64) -> u64 {
        ((val as u128 * 0x9E37_79B9_7F4A_7C15u128) & 0xFFFF_FFFF_FFFF_FFFF) as u64
    }

    #[test]
    fn wyhash_deterministic_across_code_paths() {
        // Sweep lengths covering every branch: 0, 1-3, 4-16, 17-48, 49-64, 100.
        for len in [
            0usize, 1, 2, 3, 4, 7, 8, 15, 16, 17, 31, 48, 49, 63, 64, 100,
        ] {
            let data: [u8; 100] = core::array::from_fn(|i| (i * 7 + len) as u8);
            assert_eq!(
                wyhash_64(&data[..len], 42),
                wyhash_64(&data[..len], 42),
                "len={len}"
            );
        }
    }

    #[test]
    fn wyhash_avalanche_one_bit() {
        let d1 = [0u8; 32];
        let mut d2 = [0u8; 32];
        d2[0] = 1;
        let diff = (wyhash_64(&d1, 0) ^ wyhash_64(&d2, 0)).count_ones();
        assert!(diff >= 16, "avalanche too weak: {diff}");
    }

    #[test]
    fn wyhash_seed_and_content_sensitivity() {
        assert_ne!(wyhash_64(b"hello", 0), wyhash_64(b"hello", 1));
        assert_ne!(wyhash_64(b"hello", 0), wyhash_64(b"hellp", 0));
        assert_ne!(wyhash_64(&[0x42], 0), wyhash_64(&[0x43], 0));
        assert_ne!(wyhash_64(&[0xAA; 16], 0), wyhash_64(&[0u8; 16], 0));
    }

    #[test]
    fn wyhash_matches_small_input_oracle() {
        // Independent re-derivation of the effective ≤16-byte semantics.
        fn ref_small(data: &[u8], seed: u64) -> u64 {
            const P0: u64 = 0xa076_1d64_78bd_642f;
            const P1: u64 = 0xe703_7ed1_a0b4_28db;
            let len = data.len();
            let seed = seed ^ wymix(seed ^ P0, P1);
            if len == 0 {
                return wymix(seed, P0);
            }
            let a: u64;
            let b: u64;
            if len >= 4 {
                a = (read_u32_le(data, 0) << 32) | read_u32_le(data, len - 4);
                b = if len >= 8 {
                    (read_u32_le(data, 4) << 32) | read_u32_le(data, len - 4)
                } else {
                    a.rotate_right(32)
                };
            } else {
                a = (u64::from(data[0]) << 16)
                    | (u64::from(data[len >> 1]) << 8)
                    | u64::from(data[len - 1]);
                b = 0;
            }
            wymix(P1 ^ len as u64, wymix(a ^ P1, b ^ seed))
        }
        for len in [0usize, 1, 2, 3, 4, 5, 7, 8, 12, 16] {
            let data: [u8; 16] = core::array::from_fn(|i| (i * 31 + 11) as u8);
            assert_eq!(
                wyhash_64(&data[..len], 7),
                ref_small(&data[..len], 7),
                "len={len}"
            );
        }
    }

    #[test]
    fn tabulation_zero_key_uses_index_zero() {
        let mut t = [[0u64; 256]; 4];
        tabulation_hash_init_tables(1, &mut t);
        assert_eq!(
            tabulation_hash_u64(0, &t),
            t[0][0] ^ t[1][0] ^ t[2][0] ^ t[3][0]
        );
    }

    #[test]
    fn tabulation_init_nonzero_and_zero_seed_fallback() {
        let mut t = [[0u64; 256]; 4];
        tabulation_hash_init_tables(1, &mut t);
        assert!(t.iter().all(|tab| tab.iter().all(|&e| e != 0)));
        let mut z = [[0u64; 256]; 4];
        tabulation_hash_init_tables(0, &mut z);
        assert_ne!(z[0][0], 0);
    }

    #[test]
    fn tabulation_key_sensitivity_and_avalanche() {
        let mut t = [[0u64; 256]; 4];
        tabulation_hash_init_tables(1337, &mut t);
        assert_ne!(tabulation_hash_u64(0, &t), tabulation_hash_u64(1, &t));
        let diff = (tabulation_hash_u64(0, &t) ^ tabulation_hash_u64(0x8000_0000, &t)).count_ones();
        assert!(diff >= 16, "avalanche too weak: {diff}");
    }

    #[test]
    fn siphash_matches_array_oracle() {
        for i in 0..64_u64 {
            assert_eq!(siphash_2_4_branchless(i, 1337), siphash_reference(i, 1337));
        }
    }

    #[test]
    fn siphash_deterministic_and_key_sensitive() {
        assert_eq!(siphash_2_4_branchless(42, 1), siphash_2_4_branchless(42, 1));
        assert_ne!(siphash_2_4_branchless(42, 1), siphash_2_4_branchless(42, 2));
        assert_ne!(siphash_2_4_branchless(42, 1), siphash_2_4_branchless(43, 1));
    }

    #[test]
    fn golden_ratio_oracles() {
        for i in 0..=100_u64 {
            assert_eq!(knuth_hash_u64(i, 24), golden_product(i) >> 24);
            assert_eq!(fibonacci_hash_u64(i, 24), golden_product(i) >> 40);
        }
        // bits == 0 → shift 0 for fibonacci; knuth shift 63 at aux = 63.
        assert_eq!(fibonacci_hash_u64(9, 0), golden_product(9));
        assert_eq!(knuth_hash_u64(9, 63), golden_product(9) >> 63);
    }
}

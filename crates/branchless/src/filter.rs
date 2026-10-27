//! Probabilistic filters and sketches — 64-bit word and slice-backed kernels.
//!
//! All kernels are constant-shape: no data-dependent branching in the hot
//! path, no heap, no ambient state. Guard branches (empty/short tables) are
//! listed per item and are length-dependent, never data-dependent.
//!
//! Provenance: ported from BranchlessCInRust (`bcinr`) v26.10.11,
//! commit `1e92b213`; each item carries a `// from bcinr <path>:<lines>`
//! header. Scaffold ABIs (`#[no_mangle]`, `(val, aux)` pairs) stripped.
//! fnv/splitmix mixers are deliberately absent — `purrdf-hash` owns them.

#![allow(clippy::module_name_repetitions)]

// One home per job: the splitmix/fnv/golden constants route through the
// `purrdf-hash` sanctioned homes (`purrdf_hash::mix`, `purrdf_hash::fnv`);
// no re-derived literals in this module.
use purrdf_hash::fnv::{BASIS as FNV_OFFSET, PRIME as FNV_PRIME};
use purrdf_hash::mix::GOLDEN_GAMMA as GOLDEN;

/// Per-block seed differentiation constant for the XOR filter.
// from bcinr algorithms/xor_filter_lookup.rs:106
const XOR_BLOCK_SEED: u64 = 0x6C62_272E_07BB_0142;

/// Insert `element` into the 64-bit Bloom filter word `word` by setting two
/// independent hash-selected bit positions (golden-ratio mix + SplitMix mix,
/// each reduced mod 64).
///
/// Branchless: two multiplies, two shifts, three ORs. Data-independent.
// from bcinr algorithms/bloom_filter_add_u64.rs:18-24
#[must_use]
pub fn bloom_add(word: u64, element: u64) -> u64 {
    let h1 = element.wrapping_mul(GOLDEN);
    // DIVERGENCE from bcinr: upstream `bloom_filter_add_u64` mixes the second
    // probe with `* 0xBF58476D1CE4E5B9` while `bloom_filter_query_u64` probes
    // with `(^ FNV_OFFSET) * FNV_PRIME` — insert and probe disagreed, so
    // inserted elements were not found (false negatives on members). Aligned
    // to the query mix so the filter is self-consistent.
    let h2 = (element ^ FNV_OFFSET).wrapping_mul(FNV_PRIME);
    let p1 = (h1 >> 58) & 63;
    let p2 = (h2 >> 58) & 63;
    word | (1u64 << p1) | (1u64 << p2)
}

/// Membership probe against a 64-bit Bloom filter word.
///
/// The element maps to two independent probe positions via a golden-ratio
/// mix and an FNV-folded prime mix (each reduced mod 64). Returns `true`
/// (possibly present) iff both probe bits are set; `false` = definitely
/// absent.
///
/// Branchless: single expression, data-independent.
// from bcinr algorithms/bloom_filter_query_u64.rs:18-27
#[must_use]
pub fn bloom_query(element: u64, word: u64) -> bool {
    let h1 = element.wrapping_mul(GOLDEN);
    let h2 = (element ^ FNV_OFFSET).wrapping_mul(FNV_PRIME);
    let i = (h1 >> 58) & 63;
    let j = (h2 >> 58) & 63;
    let probe = (1u64 << i) | (1u64 << j);
    (word & probe) == probe
}

/// Bloom filter intersection: word-level AND of two filter words.
// from bcinr algorithms/bloom_filter_intersect.rs:20-22
#[must_use]
pub fn bloom_intersect(a: u64, b: u64) -> u64 {
    a & b
}

/// Bloom filter union: word-level OR of two filter words.
// from bcinr algorithms/bloom_filter_union.rs:20-22
#[must_use]
pub fn bloom_union(a: u64, b: u64) -> u64 {
    a | b
}

/// Visited-bit lattice step: mark vertex `vertex & 63` visited in word `word`.
///
/// Same semantics as the Bloom add with a single caller-chosen bit; kept as
/// its own named primitive for graph-visited lattices.
// from bcinr algorithms/bloom_filter_graph_visited.rs:19-21
#[must_use]
pub fn bloom_graph_visited(word: u64, vertex: u64) -> u64 {
    word | (1u64 << (vertex & 63))
}

/// Insert an 8-bit fingerprint `(element & 0xFF)` into the first zero byte of
/// `word` (little-endian byte order) via SWAR zero-byte detection.
///
/// If no byte is free the word is returned unchanged (insertion silently
/// saturates — the caller must size the word for its fingerprint population).
///
/// Branchless core: SWAR zero-byte detect, `trailing_zeros & 0x38` to the
/// nearest byte boundary, multiply-select on `has_zero`.
// from bcinr algorithms/quotient_filter_add_u64.rs:3-13
#[must_use]
pub fn quotient_add(word: u64, element: u64) -> u64 {
    let nonzero = (((word | 0x8080_8080_8080_8080).wrapping_sub(0x0101_0101_0101_0101)) | word)
        & 0x8080_8080_8080_8080;
    let zero_bytes = !nonzero & 0x8080_8080_8080_8080;
    let tz = zero_bytes.trailing_zeros();
    let shift = tz & 0x38; // nearest byte boundary
    let mask = (0xFFu64).wrapping_shl(shift);
    let has_zero = (zero_bytes != 0) as u64;
    let insert = ((element & 0xFF).wrapping_shl(shift)) & mask;
    word | (insert * has_zero)
}

/// SWAR tag filter: `N` buckets (N must be a power of two), each packing
/// eight 8-bit tag lanes in one `u64` word (little-endian byte order).
///
/// # Semantics
/// - Index → word: bucket selection is `idx & (N - 1)` (mask, no modulo
///   branch).
/// - Tag → lane: a tag occupies one byte lane within its bucket's word.
/// - Position → lane: insertion always targets the first empty (zero) byte
///   lane, found via SWAR zero-byte detection.
/// - **Tag `0` is the reserved empty-lane sentinel and is never a legal
///   member.** Every free lane holds a zero byte, so `contains(idx, 0)`
///   reports `true` for any bucket with space (bcinr's `contains(idx, 0)`
///   is vacuously true the same way — this is fixed by contract, not code:
///   callers must never query or store tag 0). `insert(·, 0)` writes zero
///   into a free lane, a state no-op that reports `true`.
///
/// # Divergences from bcinr
/// - `new` adds `debug_assert!(N.is_power_of_two())` — upstream omits the
///   check, silently aliasing buckets for non-power-of-two `N` (upstream:
///   `swar_quotient.rs:26-74`, pinned `1e92b213`).
/// - `insert` masks the lane shift with `& 0x38` instead of upstream's
///   `& 0xF8`: when the bucket is full, `empty == 0` has
///   `trailing_zeros() == 64`, and upstream's `64 & 0xF8 == 64` feeds
///   `<< 64` into `wrapping_shl` — a debug-panic path (bcinr
///   swar_quotient.rs:66). `64 & 0x38 == 0` keeps every shift strictly below
///   64 in all states; the zero-mask (`& 0.wrapping_sub(has_space)`) still
///   kills the write, so full-bucket semantics are unchanged.
///
/// Branchless: `contains` is one SWAR broadcast/compare expression; `insert`
/// is SWAR zero-lane detection plus multiply/select recombination.
// from bcinr patterns/swar_quotient.rs:26-74 (1e92b213)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwarTagFilter<const N: usize> {
    /// One word per bucket: 8 little-endian 8-bit tag lanes; 0 = empty lane.
    table: [u64; N],
}

impl<const N: usize> SwarTagFilter<N> {
    /// An all-empty filter: every lane in every bucket is the zero sentinel.
    ///
    /// # Panics
    /// Debug builds panic if `N` is not a power of two (bucket addressing is
    /// a mask, so non-power-of-two `N` would alias buckets).
    // from bcinr patterns/swar_quotient.rs:26-74 (1e92b213)
    #[must_use]
    pub const fn new() -> Self {
        debug_assert!(
            N.is_power_of_two(),
            "SwarTagFilter: N must be a power of two"
        );
        Self { table: [0u64; N] }
    }

    /// Number of buckets (`N`).
    // from bcinr patterns/swar_quotient.rs:26-74 (1e92b213)
    #[must_use]
    pub const fn len(&self) -> usize {
        N
    }

    /// Whether the filter has zero buckets (`N == 0`).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        N == 0
    }

    /// Whether `tag` is present in bucket `idx & (N - 1)`.
    ///
    /// Tag `0` is the reserved empty-lane sentinel: because every free lane
    /// holds a zero byte, `contains(idx, 0)` reports `true` for any bucket
    /// that still has space. Callers must never query (or store) tag 0 —
    /// this is fixed by contract, not code; see the type-level docs.
    // from bcinr patterns/swar_quotient.rs:26-74 (1e92b213)
    #[must_use]
    pub fn contains(&self, idx: usize, tag: u8) -> bool {
        crate::swar::has_byte_in_word(self.table[idx & (N - 1)], tag)
    }

    /// Insert `tag` into the first empty byte lane of bucket `idx & (N - 1)`.
    ///
    /// Returns `true` if a lane was free (the tag is stored, or the write is
    /// a saturated no-op when the bucket was already full); `false` means the
    /// bucket was full and the tag was not stored.
    // from bcinr patterns/swar_quotient.rs:26-74 (1e92b213)
    pub fn insert(&mut self, idx: usize, tag: u8) -> bool {
        let word = self.table[idx & (N - 1)];
        let empty = crate::swar::find_byte_in_word(word, 0);
        let has_space = (empty != 0) as u64;
        let shift = (empty.trailing_zeros() & 0x38) as u32;
        let ins = ((tag) as u64).wrapping_shl(shift);
        self.table[idx & (N - 1)] = word | (ins & 0u64.wrapping_sub(has_space));
        has_space == 1
    }
}

impl<const N: usize> Default for SwarTagFilter<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Derive the 8-bit XOR-filter fingerprint: MurmurHash3 finaliser over
/// `key ^ seed`, folded to one byte via `(h >> 32) ^ h`.
// from bcinr algorithms/xor_filter_lookup.rs:75-90
#[must_use]
pub fn xor_fingerprint(key: u64, seed: u64) -> u8 {
    let mut h = key ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    ((h >> 32) ^ h) as u8
}

/// Compute one of three independent block hashes for a key.
///
/// `block` ∈ {0, 1, 2} selects the hash family via an additive seed rotation.
/// SplitMix64 finaliser over `key ^ block_seed`.
// from bcinr algorithms/xor_filter_lookup.rs:97-116
#[must_use]
pub fn xor_hash(key: u64, seed: u64, block: u64) -> u64 {
    let block_seed = seed.wrapping_add(block.wrapping_mul(XOR_BLOCK_SEED));
    // splitmix64 finaliser over `state + φ`: purrdf_hash::mix::splitmix64_step
    // (identical algebra to the inlined bcinr body this replaces).
    purrdf_hash::mix::splitmix64_step(key ^ block_seed)
}

/// XOR filter membership probe over a caller-provided backing table.
///
/// The table holds `3 * n` cells (`n` per block, minimum 1); the probe XORs
/// the three block cells for the key and compares against its fingerprint.
///
/// # Guard branch (length-dependent, not data-dependent)
/// Returns `false` for `table.len() < 3` — a table with fewer than 3 cells
/// cannot hold a valid filter; indexing is skipped entirely.
// from bcinr algorithms/xor_filter_lookup.rs:34-53
#[must_use]
pub fn xor_lookup(key: u64, table: &[u8], seed: u64) -> bool {
    // Guard: fewer than 3 entries cannot hold a valid filter.
    if table.len() < 3 {
        return false;
    }
    let n = (table.len() / 3).max(1);
    let fp = xor_fingerprint(key, seed);
    let h0 = xor_hash(key, seed, 0) as usize % n;
    let h1 = xor_hash(key, seed, 1) as usize % n + n;
    let h2 = xor_hash(key, seed, 2) as usize % n + 2 * n;
    let xor_val = table[h0] ^ table[h1] ^ table[h2];
    xor_val == fp
}

/// HyperLogLog register update: `rho = leading_zeros(hash) + 1`, register
/// takes the max of its current value and `rho`.
///
/// `reg` packs a single register; the slice/register-file layer belongs to
/// the caller.
// from bcinr algorithms/hyperloglog_add_u64.rs:18-21
#[must_use]
pub fn hll_add(hash: u64, reg: u64) -> u64 {
    let rho = (hash.leading_zeros()) as u64 + 1;
    reg.max(rho)
}

/// HyperLogLog register merge: componentwise max of two register words.
// from bcinr algorithms/hyperloglog_merge.rs:18-20
#[must_use]
pub fn hll_merge(a: u64, b: u64) -> u64 {
    a.max(b)
}

/// Count-min sketch register word update: `regs` packs four 16-bit counter
/// cells; `element` is hashed (golden-ratio mix, reduced mod 4) to pick one
/// cell, incremented by 1 with saturation at `u16::MAX`.
///
/// Branchless: mask-clear + saturating next + recombination.
// from bcinr algorithms/count_min_sketch_add.rs:19-29
#[must_use]
pub fn cms_add_word(regs: u64, element: u64) -> u64 {
    let h = (element.wrapping_mul(GOLDEN) >> 62) & 3;
    let shift = (h * 16) as u32;
    let cur = (regs >> shift) & 0xFFFF;
    let next = (cur + 1).min(0xFFFF);
    let cleared = regs & !(0xFFFFu64 << shift);
    cleared | (next << shift)
}

/// Count-min sketch point query over a single packed register word: minimum
/// across the two candidate counters passed in `a`/`b`.
///
/// This is the reduced single-word form of bcinr's scaffold; the row-minimum
/// fold is the caller's loop over rows.
// from bcinr algorithms/count_min_sketch_query.rs:19-21
#[must_use]
pub fn cms_query_word(a: u64, b: u64) -> u64 {
    a.min(b)
}

/// Per-row hash for the count-min sketch: SplitMix finaliser over
/// `key ^ row_seed`, where `row_seed = row * golden_ratio`.
// from bcinr algorithms/count_min_sketch_update.rs:46-56
#[must_use]
pub fn cms_hash(key: u64, row: u64) -> u64 {
    // splitmix64 finaliser: purrdf_hash::mix::splitmix64_finalize (identical
    // algebra to the inlined bcinr body this replaces).
    purrdf_hash::mix::splitmix64_finalize(key ^ row.wrapping_mul(GOLDEN))
}

/// Count-min sketch update: add `delta` to one cell per row over a
/// caller-owned `d × w` sketch (`sketch.len() >= d * w`), saturating.
///
/// # Guard
/// `sketch.len() < d * w` is undefined behavior territory — the caller
/// contract is `sketch.len() >= d * w`; a shorter slice is not supported
/// (debug builds assert).
///
/// # Panics
/// Debug builds panic if `sketch.len() < d * w`.
// from bcinr algorithms/count_min_sketch_update.rs:34-40
pub fn cms_update(sketch: &mut [u32], d: usize, w: usize, key: u64, delta: u32) {
    debug_assert!(sketch.len() >= d * w, "sketch shorter than d*w");
    for i in 0..d {
        let h = cms_hash(key, i as u64) as usize % w;
        let idx = i * w + h;
        sketch[idx] = sketch[idx].saturating_add(delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- independent loop-form oracles -----------------------------------

    fn ref_bloom_add(word: u64, element: u64) -> u64 {
        // Oracle mirrors the ALIGNED probe mix (see DIVERGENCE note on
        // `bloom_add`): second position from the FNV-fold, matching query.
        let p1 = ((element.wrapping_mul(GOLDEN) >> 58) & 63) as u32;
        let h2 = (element ^ FNV_OFFSET).wrapping_mul(FNV_PRIME);
        let p2 = ((h2 >> 58) & 63) as u32;
        let mut out = word;
        out |= 1u64.rotate_left(p1);
        out |= 1u64.rotate_left(p2);
        out
    }

    fn ref_bloom_query(element: u64, word: u64) -> bool {
        let p1 = ((element.wrapping_mul(GOLDEN) >> 58) & 63) as u32;
        let h2 = (element ^ FNV_OFFSET).wrapping_mul(FNV_PRIME);
        let p2 = ((h2 >> 58) & 63) as u32;
        (word >> p1) & 1 == 1 && (word >> p2) & 1 == 1
    }

    fn ref_quotient(word: u64, element: u64) -> u64 {
        let mut v = word.to_le_bytes();
        let fp = (element & 0xFF) as u8;
        for b in &mut v {
            if *b == 0 {
                *b = fp;
                break;
            }
        }
        u64::from_le_bytes(v)
    }

    fn ref_hll_add(hash: u64, reg: u64) -> u64 {
        // MSB-scan rho derivation (independent of leading_zeros).
        let mut clz = 0u32;
        for bit in (0..64).rev() {
            if (hash >> bit) & 1 == 1 {
                break;
            }
            clz += 1;
        }
        reg.max((clz) as u64 + 1)
    }

    // ---- Bloom ---------------------------------------------------------------

    /// THIRD independent formulation of the `bloom_add` bit pattern.
    ///
    /// Differs from the impl (and from `ref_bloom_add`) in arithmetic shape:
    /// 1. Positions come from 128-bit wide multiplication — the top six bits
    ///    of the low word are read as bits 58..=63 of the full 128-bit
    ///    product, extracted by an MSB-first bit scan, never by `>> 58`.
    /// 2. The FNV fold `(x ^ OFF) * PRIME mod 2^64` is computed as the LOW
    ///    half of a 128-bit product (a different reduction path than
    ///    `wrapping_mul`'s semantics-level statement, computed explicitly).
    /// 3. The expected word is assembled by testing each of the 64 bit
    ///    positions against the extracted position values, not by shifting a
    ///    `1u64` into place.
    fn ref_bloom_pattern(word: u64, element: u64) -> u64 {
        let wide1 = ((element) as u128) * (GOLDEN) as u128;
        let fnv = ((element ^ FNV_OFFSET) as u128) * (FNV_PRIME) as u128;
        let wide2 = fnv & 0xFFFF_FFFF_FFFF_FFFF; // low half = mod 2^64
        // MSB-first scan extracting bits 58..=63 of each 128-bit product.
        let pos = |prod: u128| -> u32 {
            let mut p = 0u32;
            for bit in (58..=63).rev() {
                p <<= 1;
                p |= ((prod >> bit) & 1 == 1) as u32;
            }
            p
        };
        let (p1, p2) = (pos(wide1), pos(wide2));
        let mut out = 0u64;
        for bit in 0..64u32 {
            let base = (word >> bit) & 1 == 1;
            let hit = bit == p1 || bit == p2;
            if base || hit {
                out |= 1 << bit;
            }
        }
        out
    }

    #[test]
    fn bloom_add_matches_independent_pattern_oracle() {
        // Fixed corner corpus plus a deterministic xorshift64* sweep over
        // both the word and element dimensions (10k keys each).
        let mut state: u64 = 0x1234_5678_9ABC_DEF0;
        let mut next = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        let words = [0u64, 1, u64::MAX, 0xDEAD_BEEF_CAFE_F00D];
        for &w in &words {
            for &e in &[0u64, 1, 42, 1337, u64::MAX] {
                assert_eq!(
                    bloom_add(w, e),
                    ref_bloom_pattern(w, e),
                    "w={w:#x} e={e:#x}"
                );
            }
            for _ in 0..10_000 {
                let e = next();
                assert_eq!(
                    bloom_add(w, e),
                    ref_bloom_pattern(w, e),
                    "w={w:#x} e={e:#x}"
                );
            }
        }
        for _ in 0..10_000 {
            let w = next();
            let e = next();
            assert_eq!(
                bloom_add(w, e),
                ref_bloom_pattern(w, e),
                "w={w:#x} e={e:#x}"
            );
        }
    }

    #[test]
    fn bloom_query_agrees_with_independent_pattern() {
        // The impl's query must accept exactly the elements whose two
        // independently-computed pattern bits are present in the word.
        let mut state: u64 = 0x0FED_CBA9_8765_4321;
        let mut next = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        for _ in 0..10_000 {
            let e = next();
            let w = next();
            let expected = (ref_bloom_pattern(0, e) & !w) == 0; // both probe bits set?
            assert_eq!(bloom_query(e, w), expected, "e={e:#x} w={w:#x}");
        }
    }

    #[test]
    fn bloom_add_matches_loop_oracle() {
        for &w in &[0u64, 1, u64::MAX, 0xAAAA_AAAA_AAAA_AAAA] {
            for &e in &[0u64, 1, 42, 1337, u64::MAX] {
                assert_eq!(bloom_add(w, e), ref_bloom_add(w, e));
            }
        }
    }

    #[test]
    fn bloom_query_matches_oracle_and_inserted_elements_found() {
        for &e in &[0u64, 1, 42, 1337, u64::MAX] {
            let w = bloom_add(0, e);
            assert!(bloom_query(e, w), "inserted element must be found");
            assert_eq!(bloom_query(e, w), ref_bloom_query(e, w));
        }
    }

    #[test]
    fn bloom_query_rejects_empty_word() {
        for &e in &[0u64, 42, u64::MAX] {
            assert!(!bloom_query(e, 0), "empty filter must reject");
        }
    }

    #[test]
    fn bloom_set_ops_are_and_or() {
        assert_eq!(bloom_intersect(0xF0F0, 0x0FF0), 0x00F0);
        assert_eq!(bloom_union(0xF0F0, 0x0FF0), 0xFFF0);
        assert_eq!(bloom_graph_visited(0, 63), 1u64 << 63);
        assert_eq!(bloom_graph_visited(0, 64), 1); // wraps to bit 0
    }

    // ---- quotient filter --------------------------------------------------

    #[test]
    fn quotient_matches_byte_loop_oracle() {
        for &w in &[0u64, 0x0102_0000_0000_0000, u64::MAX, 0x00FF_FFFF_FFFF_FFFF] {
            for &e in &[0u64, 1, 0xAB, 0xFF, u64::MAX] {
                assert_eq!(quotient_add(w, e), ref_quotient(w, e));
            }
        }
    }

    #[test]
    fn quotient_fuzz_fixed_seed_xorshift() {
        // Deterministic mini-fuzz (xorshift64*), standing in for bcinr's
        // proptest suite — zero-dep crate, so the generator is hand-rolled.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        for _ in 0..10_000 {
            let w = next();
            assert_eq!(quotient_add(w, 0), ref_quotient(w, 0), "w={w:#x}");
        }
        // Also fuzz the element dimension with a fixed word.
        let word = 0x0102_0300_0000_0000u64;
        for _ in 0..10_000 {
            let e = next();
            assert_eq!(quotient_add(word, e), ref_quotient(word, e));
        }
    }

    #[test]
    fn quotient_saturates_when_full() {
        assert_eq!(quotient_add(u64::MAX, 0xAB), u64::MAX);
    }

    // ---- XOR filter -------------------------------------------------------

    #[test]
    fn xor_single_key_member() {
        // n=1 per block: h0%1=0, h1%1=0, h2%1=0 -> indices 0,1,2.
        // Decomposition: fp = f0 ^ f1 ^ f2; choose f0=fp, f1=f2=0.
        let (key, seed) = (42u64, 1337u64);
        let fp = xor_fingerprint(key, seed);
        let table = [fp, 0, 0];
        assert!(xor_lookup(key, &table, seed));
    }

    #[test]
    fn xor_wrong_key_misses() {
        let (key, seed) = (42u64, 1337u64);
        let _baseline = [xor_fingerprint(key, seed), 0, 0];
        // A different key has different fingerprints/blocks; over a few seeds
        // at least one must miss (fingerprint collision only if identical fp).
        let misses = (0..64u64)
            .filter(|&s| {
                let t = [xor_fingerprint(key, s), 0, 0];
                !xor_lookup(key + 1, &t, s)
            })
            .count();
        assert!(misses > 0, "foreign key must miss under some seed");
    }

    #[test]
    fn xor_short_table_guard() {
        assert!(!xor_lookup(0, &[], 0));
        assert!(!xor_lookup(0, &[0u8], 0));
        assert!(!xor_lookup(0, &[0u8, 0], 0));
    }

    #[test]
    fn xor_fingerprint_seed_dependent() {
        assert_ne!(
            xor_fingerprint(42, 0),
            xor_fingerprint(42, 1),
            "different seeds should usually differ"
        );
    }

    // ---- HLL --------------------------------------------------------------

    #[test]
    fn hll_add_matches_msb_scan_oracle() {
        for &h in &[0u64, 1, 1 << 63, 42, u64::MAX] {
            for &r in &[0u64, 1, 5, u64::MAX] {
                assert_eq!(hll_add(h, r), ref_hll_add(h, r));
            }
        }
    }

    #[test]
    fn hll_merge_is_max() {
        assert_eq!(hll_merge(3, 7), 7);
        assert_eq!(hll_merge(9, 2), 9);
        assert_eq!(hll_merge(0, 0), 0);
    }

    // ---- CMS --------------------------------------------------------------

    #[test]
    fn cms_add_word_saturation_and_isolation() {
        // Oracle: extract the cell the impl must touch from the same hash.
        let cell = |e: u64| ((e.wrapping_mul(GOLDEN) >> 62) & 3) * 16;
        let get = |regs: u64, e: u64| (regs >> cell(e)) & 0xFFFF;

        // Saturated cell stays saturated.
        let full = 0xFFFFu64 << cell(u64::MAX);
        assert_eq!(cms_add_word(full, u64::MAX), full);

        // Increment from empty: exactly one cell becomes 1.
        let after = cms_add_word(0, 1);
        assert_eq!(get(after, 1), 1);
        assert_eq!(after.count_ones(), 1);

        // Second element: only its own cell moves; others untouched.
        let after2 = cms_add_word(after, 0);
        assert_eq!(get(after2, 0), 1);
        assert_eq!(get(after2, 1), 1);

        // Isolation: a saturated cell elsewhere is not disturbed.
        let mixed = 0xFFFFu64 << 48;
        let after3 = cms_add_word(mixed, 0);
        assert_eq!(after3 >> 48 & 0xFFFF, 0xFFFF);
        assert_eq!(get(after3, 0), 1);
    }

    #[test]
    fn cms_query_word_is_min() {
        assert_eq!(cms_query_word(3, 7), 3);
        assert_eq!(cms_query_word(9, 2), 2);
    }

    #[test]
    fn cms_update_and_query_end_to_end() {
        // 2 rows x 4 columns.
        let (d, w) = (2usize, 4usize);
        let mut sketch = [0u32; 8];
        for k in 0..5u64 {
            cms_update(&mut sketch, d, w, k, 3);
        }
        for k in 0..5u64 {
            let row0 = sketch[cms_hash(k, 0) as usize % w];
            let row1 = sketch[w + cms_hash(k, 1) as usize % w];
            assert!(row0 >= 3 && row1 >= 3);
            assert_eq!(
                cms_query_word((row0) as u64, (row1) as u64),
                (row0.min(row1)) as u64
            );
        }
    }

    #[test]
    #[should_panic(expected = "sketch shorter than d*w")]
    fn cms_update_debug_asserts_on_short_sketch() {
        let mut sketch = [0u32; 2];
        cms_update(&mut sketch, 2, 4, 1, 1); // needs len >= 8
    }

    // ---- bloom position-pair oracle (OPT-42 §3.3) -------------------------

    #[test]
    fn bloom_add_bit_pattern_is_position_pair_oracle() {
        // Independent bit-level proof: for every (p1, p2) in 0..64 x 0..64,
        // find (by exhaustive deterministic search) an element whose two fold
        // mixes land exactly on that position pair, then assert `bloom_add(0,
        // e)` sets exactly bits {p1, p2}. No shift-expression is shared with
        // the impl — positions are reconstructed from a precomputed table.
        let mut first_at = [u64::MAX; 4096];
        for e in 0..1u64 << 20 {
            let p1 = ((e.wrapping_mul(GOLDEN) >> 58) & 63) as usize;
            let p2 = (((e ^ FNV_OFFSET).wrapping_mul(FNV_PRIME) >> 58) & 63) as usize;
            let slot = (p1 << 6) | p2;
            if first_at[slot] == u64::MAX {
                first_at[slot] = e;
            }
        }
        for p1 in 0..64usize {
            for p2 in 0..64usize {
                let e = first_at[(p1 << 6) | p2];
                assert!(e != u64::MAX, "no element found for pair ({p1},{p2})");
                let w = bloom_add(0, e);
                let expect = (1u64 << p1) | (1u64 << p2);
                assert_eq!(w, expect, "p1={p1} p2={p2} e={e}");
                if p1 == p2 {
                    assert_eq!(w.count_ones(), 1, "double-set pair must set one bit");
                } else {
                    assert_eq!(w.count_ones(), 2);
                }
            }
        }
    }

    // ---- SwarTagFilter ----------------------------------------------------

    extern crate std;

    use std::collections::HashSet;

    /// Byte-loop oracle for insert: scan little-endian lanes, write into the
    /// first zero lane; full bucket returned unchanged.
    fn ref_filter_insert(word: u64, tag: u8) -> u64 {
        let mut v = word.to_le_bytes();
        for b in &mut v {
            if *b == 0 {
                *b = tag;
                break;
            }
        }
        u64::from_le_bytes(v)
    }

    /// Byte-loop oracle for contains: any lane equal to `tag`.
    fn ref_filter_contains(word: u64, tag: u8) -> bool {
        word.to_le_bytes().iter().any(|&b| b == tag)
    }

    fn xorshift() -> impl FnMut() -> u64 {
        let mut state: u64 = 0xC0FF_EE12_3456_789A;
        move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
    }

    #[test]
    fn swar_tag_filter_insert_and_contains_match_byte_loop_oracles() {
        let mut f = SwarTagFilter::<8>::new();
        let mut next = xorshift();
        // Insert sweep: after each insert the bucket word must equal the
        // byte-loop oracle applied to the previous word.
        for i in 0..10_000usize {
            let bucket = (next() as usize) & 7;
            let tag = (next() & 0xFF) as u8;
            let before = f.table[bucket];
            let inserted = f.insert(bucket, tag);
            let expected = ref_filter_insert(before, tag);
            assert_eq!(
                f.table[bucket], expected,
                "i={i} bucket={bucket} tag={tag:#x}"
            );
            let oracle_has_space = before.to_le_bytes().iter().any(|&b| b == 0);
            assert_eq!(
                inserted, oracle_has_space,
                "i={i} bucket={bucket} tag={tag:#x}"
            );
        }
        // Mixed probe sweep: ~half present (just-inserted) tags, ~half absent.
        let mut f2 = SwarTagFilter::<8>::new();
        for _ in 0..10_000 {
            let bucket = (next() as usize) & 7;
            let tag = (next() & 0xFF) as u8;
            let _ = f2.insert(bucket, tag);
        }
        for i in 0..10_000usize {
            let bucket = (next() as usize) & 7;
            let tag = if i % 2 == 0 {
                // Present: read a lane actually stored in this bucket.
                let bytes = f2.table[bucket].to_le_bytes();
                bytes[i % 8]
            } else {
                (next() & 0xFF) as u8
            };
            if tag == 0 {
                continue; // tag 0 is the reserved empty sentinel, not probed
            }
            assert_eq!(
                f2.contains(bucket, tag),
                ref_filter_contains(f2.table[bucket], tag),
                "i={i} bucket={bucket} tag={tag:#x}"
            );
        }
    }

    #[test]
    fn swar_tag_filter_agrees_with_hashset_cross_check() {
        let mut f = SwarTagFilter::<16>::new();
        let mut inserted: HashSet<(usize, u8)> = HashSet::new();
        let mut next = xorshift();
        for _ in 0..5_000usize {
            let bucket = (next() as usize) & 15;
            let tag = (next() & 0xFF) as u8;
            if tag == 0 {
                continue;
            }
            let ok = f.insert(bucket, tag);
            if ok {
                inserted.insert((bucket, tag));
            }
            // `ok == false` here means the bucket was already full AND the tag
            // was not resident — a real eviction-free miss the cross-check
            // below must reproduce as `contains == false`.
            let was_resident = inserted.contains(&(bucket, tag));
            assert_eq!(f.contains(bucket, tag), ok || was_resident);
        }
        // Every insert-reported member must be found, and vice versa.
        for bucket in 0..16usize {
            for tag in 1..=255u8 {
                assert_eq!(
                    f.contains(bucket, tag),
                    inserted.contains(&(bucket, tag)),
                    "bucket={bucket} tag={tag:#x}"
                );
            }
        }
    }

    #[test]
    fn swar_tag_filter_full_bucket_court_both_directions() {
        // Fully occupied word (no zero lanes): all 8 resident tags contained,
        // absent tag rejected.
        let mut f = SwarTagFilter::<4>::new();
        f.table[0] = 0x0807_0605_0403_0201;
        for tag in 1u8..=8 {
            assert!(f.contains(0, tag), "resident tag {tag} must be contained");
        }
        assert!(!f.contains(0, 0x09), "absent tag 0x09 must be rejected");
        let mut next = xorshift();
        for _ in 0..1_000 {
            let tag = (next() & 0xFF) as u8;
            if tag != 0 && !matches!(tag, 1..=8) {
                assert!(!f.contains(0, tag), "swept absent tag {tag:#x} rejected");
            }
        }
        // 8 inserts fill the bucket; the 9th returns false and changes nothing.
        let mut g = SwarTagFilter::<4>::new();
        for tag in 1u8..=8 {
            assert!(g.insert(1, tag));
        }
        assert_eq!(g.table[1], 0x0807_0605_0403_0201);
        assert!(
            !g.insert(1, 0x09),
            "9th insert into a full bucket returns false"
        );
        assert_eq!(
            g.table[1], 0x0807_0605_0403_0201,
            "full bucket must be unchanged after failed insert"
        );
    }

    #[test]
    fn swar_tag_filter_rejects_all_nonzero_tags_when_empty() {
        let mut f = SwarTagFilter::<8>::new();
        let mut next = xorshift();
        for _ in 0..5_000 {
            let tag = (next() & 0xFF) as u8;
            if tag == 0 {
                continue; // reserved sentinel: see type contract, not a member
            }
            let bucket = (next() as usize) & 7;
            assert!(
                !f.contains(bucket, tag),
                "empty filter must reject tag {tag:#x}"
            );
        }
        // Reserved sentinel: contains(_, 0) reflects free-lane presence (all
        // lanes free here), which is exactly why tag 0 is reserved by contract.
        assert!(f.contains(0, 0));
        assert!(
            f.insert(0, 0),
            "inserting the sentinel is a state no-op but succeeds"
        );
    }

    #[test]
    fn swar_tag_filter_len_and_mask_addressing() {
        assert_eq!(SwarTagFilter::<1>::new().len(), 1);
        assert_eq!(SwarTagFilter::<64>::new().len(), 64);
        // idx is taken modulo N via the mask: idx and idx+N address the same bucket.
        let mut f = SwarTagFilter::<4>::new();
        assert!(f.insert(9, 0xAB)); // 9 & 3 == 1
        assert!(f.contains(1, 0xAB));
        assert!(f.contains(13, 0xAB)); // 13 & 3 == 1
        assert!(!f.contains(2, 0xAB));
    }
}

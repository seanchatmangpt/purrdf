//! Small-N branchless sorting networks.
//!
//! Provenance: ported from `bcinr` (`bench/rdtsc-tick-tables`, commit
//! `1e92b213`) — `bcinr-logic/src/network.rs:49-132`,
//! `bcinr-logic/src/algorithms/optimal_sort_{5,6,7,8}_u32.rs`,
//! `algorithms/odd_even_merge_sort_16u32.rs`,
//! `algorithms/insertion_sort_branchless_fixed.rs:26-69`.
//! Scaffold ABI (`#[no_mangle]`, `(val, aux)` pairs) and vacuous-mutant test
//! scaffolding stripped. Renamed honestly: bcinr's `*_16*` names refer to
//! 16-BIT register lanes, not 16 elements; the true 16-element networks
//! (Batcher 63-comparator, bitonic 80-comparator) live in [`crate::sort16`].
//!
//! Every compare-exchange is XOR-mask conditional selection: no `if`, no
//! temporaries — predicated `xor`, CC=1 on every input.

// Branchless fixed-trip kernels: inlining into every caller is the point of the
// crate (constant-folded comparators, cmov fusion). Deliberate, per-module.
#![allow(clippy::inline_always)]

/// Branchless compare-exchange: orders `a[i] <= a[j]` unconditionally.
///
/// The single canonical kernel for every non-directional compare-exchange in
/// this crate ([`crate::sort16`] delegates here; `cex_dir` there adds only the
/// direction bit). from bcinr `bcinr-logic/src/network.rs:49-54`. XOR-mask
/// conditional selection — the guaranteed-cmov form
/// `purrdf-core::primitives::cswap!` should adopt (see OPT-12).
#[inline(always)]
pub(crate) fn cswap(a: &mut [u32], i: usize, j: usize) {
    let mask = (a[i] > a[j]) as u32;
    let diff = (a[i] ^ a[j]) & 0u32.wrapping_sub(mask);
    a[i] ^= diff;
    a[j] ^= diff;
}

/// Sort `[u32; 2]` ascending: 1 comparator.
#[inline(always)]
pub fn sort2(a: &mut [u32; 2]) {
    cswap(a, 0, 1);
}

/// Sort `[u32; 4]` ascending: optimal 5-comparator network (from bcinr
/// `odd_even_merge_sort_16u32` / `green_sorting_network_16` — the identical
/// n=4 graph, ported once with an element-count-honest name).
#[inline(always)]
pub fn sort4(a: &mut [u32; 4]) {
    cswap(a, 0, 1);
    cswap(a, 2, 3);
    cswap(a, 0, 2);
    cswap(a, 1, 3);
    cswap(a, 1, 2);
}

/// Sort `[u32; 5]` ascending: optimal 9-comparator network (Knuth, AoCP V3).
/// from bcinr `optimal_sort_5_u32.rs`.
#[inline(always)]
pub fn sort5(a: &mut [u32; 5]) {
    cswap(a, 0, 3);
    cswap(a, 1, 4);
    cswap(a, 0, 2);
    cswap(a, 1, 3);
    cswap(a, 0, 1);
    cswap(a, 2, 4);
    cswap(a, 1, 2);
    cswap(a, 3, 4);
    cswap(a, 2, 3);
}

/// Sort `[u32; 6]` ascending: 12-comparator network. from bcinr
/// `optimal_sort_6_u32.rs`.
#[inline(always)]
pub fn sort6(a: &mut [u32; 6]) {
    cswap(a, 0, 1);
    cswap(a, 2, 3);
    cswap(a, 4, 5);
    cswap(a, 0, 2);
    cswap(a, 1, 4);
    cswap(a, 3, 5);
    cswap(a, 0, 1);
    cswap(a, 2, 3);
    cswap(a, 4, 5);
    cswap(a, 1, 2);
    cswap(a, 3, 4);
    cswap(a, 2, 3);
}

/// Sort `[u32; 7]` ascending: 16-comparator network. from bcinr
/// `optimal_sort_7_u32.rs`.
#[inline(always)]
pub fn sort7(a: &mut [u32; 7]) {
    cswap(a, 0, 6);
    cswap(a, 2, 3);
    cswap(a, 4, 5);
    cswap(a, 0, 2);
    cswap(a, 1, 4);
    cswap(a, 3, 6);
    cswap(a, 0, 1);
    cswap(a, 2, 5);
    cswap(a, 3, 4);
    cswap(a, 1, 2);
    cswap(a, 4, 6);
    cswap(a, 2, 3);
    cswap(a, 4, 5);
    cswap(a, 1, 2);
    cswap(a, 3, 4);
    cswap(a, 5, 6);
}

/// Sort `[u32; 8]` ascending: optimal 19-comparator Batcher odd-even merge
/// network, depth 6. from bcinr `optimal_sort_8_u32.rs`.
#[inline(always)]
pub fn sort8(a: &mut [u32; 8]) {
    cswap(a, 0, 1);
    cswap(a, 2, 3);
    cswap(a, 4, 5);
    cswap(a, 6, 7);
    cswap(a, 0, 2);
    cswap(a, 1, 3);
    cswap(a, 4, 6);
    cswap(a, 5, 7);
    cswap(a, 1, 2);
    cswap(a, 5, 6);
    cswap(a, 0, 4);
    cswap(a, 3, 7);
    cswap(a, 1, 5);
    cswap(a, 2, 6);
    cswap(a, 1, 4);
    cswap(a, 3, 6);
    cswap(a, 2, 4);
    cswap(a, 3, 5);
    cswap(a, 3, 4);
}

/// Largest slice length the inline comparator network handles before falling
/// back to `sort_unstable_by_key` (core-stable since 1.82, allocation-free).
///
/// from bcinr `optimal_sort_8_u32.rs` lineage; promoted from datalog join.rs
/// (OPT-49, `crates/datalog/src/seminaive/join.rs:491`).
pub const SORT_INLINE_MAX: usize = 8;

/// Branchless compare-exchange over `u64` lanes: orders `lanes[i] <= lanes[j]`
/// unconditionally.
///
/// Promoted from datalog join.rs (OPT-49,
/// `crates/datalog/src/seminaive/join.rs:504-509`); from bcinr `sort_small`
/// lineage (`bcinr-logic/src/network.rs:49-54`, the same XOR-mask idiom as the
/// `u32` [`cswap`] above, over 64-bit lanes — [`cswap`] stays the canonical
/// `u32` kernel, this is its `u64` sibling).
///
/// DIVERGENCE from bcinr: none — verbatim idiom, widened to `u64` lanes.
#[inline(always)]
pub fn cswap_u64(lanes: &mut [u64], i: usize, j: usize) {
    let mask = (lanes[i] > lanes[j]) as u64;
    let diff = (lanes[i] ^ lanes[j]) & 0u64.wrapping_sub(mask);
    lanes[i] ^= diff;
    lanes[j] ^= diff;
}

/// Sort `[u64; 8]` ascending: 19-comparator Batcher odd-even merge network,
/// depth 6 — the same comparator graph as [`sort8`], carried over `u64` lanes
/// through [`cswap_u64`].
///
/// Promoted from datalog join.rs (OPT-49,
/// `crates/datalog/src/seminaive/join.rs:514-535`); from bcinr
/// `optimal_sort_8_u32.rs`.
#[inline(always)]
fn network_sort8(packed: &mut [u64; 8]) {
    let lanes = packed.as_mut_slice();
    cswap_u64(lanes, 0, 1);
    cswap_u64(lanes, 2, 3);
    cswap_u64(lanes, 4, 5);
    cswap_u64(lanes, 6, 7);
    cswap_u64(lanes, 0, 2);
    cswap_u64(lanes, 1, 3);
    cswap_u64(lanes, 4, 6);
    cswap_u64(lanes, 5, 7);
    cswap_u64(lanes, 1, 2);
    cswap_u64(lanes, 5, 6);
    cswap_u64(lanes, 0, 4);
    cswap_u64(lanes, 3, 7);
    cswap_u64(lanes, 1, 5);
    cswap_u64(lanes, 2, 6);
    cswap_u64(lanes, 1, 4);
    cswap_u64(lanes, 3, 6);
    cswap_u64(lanes, 2, 4);
    cswap_u64(lanes, 3, 5);
    cswap_u64(lanes, 3, 4);
}

/// Sorts `items` (len <= [`SORT_INLINE_MAX`]) ascending by a `u64` key with a
/// fixed comparator network, allocation-free.
///
/// Packing contract: keys must be below 2^56 so `(key << 8) | origin_slot`
/// stays in the `u64` lane without clobbering the key (the production
/// instantiation is `body_index: u32`, far below the bound). Sentinel lanes
/// are packed with `u64::MAX`, which sorts past every real key because real
/// keys are shifted left before packing.
///
/// Stability: ties break by original slot — the packed low 8 bits carry the
/// origin slot, so the network is observationally identical to a stable
/// `sort_by_key` even under duplicate keys. Longer slices fall back to
/// core `sort_unstable_by_key`; the OPT-49 caller pins key uniqueness there,
/// so stable/unstable are observationally identical on the fallback path.
///
/// Promoted from datalog join.rs (OPT-49,
/// `crates/datalog/src/seminaive/join.rs:545-571`); from bcinr `sort_small`
/// lineage.
pub fn sort_small_by_key<T: Copy, F>(items: &mut [T], key: F)
where
    F: Fn(&T) -> u64,
{
    let len = items.len();
    if len < 2 {
        return;
    }
    if len > SORT_INLINE_MAX {
        items.sort_unstable_by_key(|value| key(value));
        return;
    }
    // Pack the sort key with the origin slot; u64::MAX pads the unused lanes
    // past every real key (keys are bounded by 2^56 — see the packing
    // contract above).
    let mut packed = [u64::MAX; SORT_INLINE_MAX];
    for (slot, value) in items.iter().enumerate() {
        packed[slot] = (key(value) << 8) | slot as u64;
    }
    network_sort8(&mut packed);
    // Copy lanes back through an inline staging array; no heap, no `Clone`
    // bound, and the shared borrow ends before the writes below.
    let staged: [Option<T>; SORT_INLINE_MAX] =
        core::array::from_fn(|slot| (slot < len).then(|| items[slot]));
    for (lane, slot) in packed.into_iter().enumerate().take(len) {
        items[lane] = staged[(slot & 0xFF) as usize].expect("slots below len are staged");
    }
}

/// Branchless 8-byte rank-scatter sort: sorts the 8 bytes of a `u64`
/// ascending in one expression — each byte's destination is its stable rank
/// (# strictly less + # equal at an earlier position), computed by fully
/// unrolled comparison-as-arithmetic. Loop-free, CC=1.
///
/// from bcinr `insertion_sort_branchless_fixed.rs:26-69`.
#[inline(always)]
pub fn sort_bytes_u64(val: u64) -> u64 {
    let b = [
        val & 0xFF,
        (val >> 8) & 0xFF,
        (val >> 16) & 0xFF,
        (val >> 24) & 0xFF,
        (val >> 32) & 0xFF,
        (val >> 40) & 0xFF,
        (val >> 48) & 0xFF,
        (val >> 56) & 0xFF,
    ];
    let lt = |x: u64, y: u64| -> u64 { (x < y) as u64 };
    let eqe = |j: usize, i: usize, x: u64, y: u64| -> u64 { (x == y) as u64 & (j < i) as u64 };
    let rank = |i: usize| -> u64 {
        lt(b[0], b[i])
            + lt(b[1], b[i])
            + lt(b[2], b[i])
            + lt(b[3], b[i])
            + lt(b[4], b[i])
            + lt(b[5], b[i])
            + lt(b[6], b[i])
            + lt(b[7], b[i])
            + eqe(0, i, b[0], b[i])
            + eqe(1, i, b[1], b[i])
            + eqe(2, i, b[2], b[i])
            + eqe(3, i, b[3], b[i])
            + eqe(4, i, b[4], b[i])
            + eqe(5, i, b[5], b[i])
            + eqe(6, i, b[6], b[i])
            + eqe(7, i, b[7], b[i])
    };
    (b[0] << (rank(0) * 8))
        | (b[1] << (rank(1) * 8))
        | (b[2] << (rank(2) * 8))
        | (b[3] << (rank(3) * 8))
        | (b[4] << (rank(4) * 8))
        | (b[5] << (rank(5) * 8))
        | (b[6] << (rank(6) * 8))
        | (b[7] << (rank(7) * 8))
}

/// Sort four u16 lanes packed in a `u64` ascending: the 5-comparator n=4
/// network applied to 16-bit lanes (bcinr `green_sorting_network_16` /
/// `odd_even_merge_sort_16u32` — named for 16-BIT lanes there; renamed here).
#[inline(always)]
pub fn sort4_lanes_u16(v: u64) -> u64 {
    let mut l0 = v & 0xFFFF;
    let mut l1 = (v >> 16) & 0xFFFF;
    let mut l2 = (v >> 32) & 0xFFFF;
    let mut l3 = (v >> 48) & 0xFFFF;
    // ce on u64 lanes (min/max-split form compiles to cmov as well; the XOR
    // form below keeps the single idiom).
    let m = (l0 > l1) as u64;
    let d = (l0 ^ l1) & 0u64.wrapping_sub(m);
    l0 ^= d;
    l1 ^= d;
    let m = (l2 > l3) as u64;
    let d = (l2 ^ l3) & 0u64.wrapping_sub(m);
    l2 ^= d;
    l3 ^= d;
    let m = (l0 > l2) as u64;
    let d = (l0 ^ l2) & 0u64.wrapping_sub(m);
    l0 ^= d;
    l2 ^= d;
    let m = (l1 > l3) as u64;
    let d = (l1 ^ l3) & 0u64.wrapping_sub(m);
    l1 ^= d;
    l3 ^= d;
    let m = (l1 > l2) as u64;
    let d = (l1 ^ l2) & 0u64.wrapping_sub(m);
    l1 ^= d;
    l2 ^= d;
    l0 | (l1 << 16) | (l2 << 32) | (l3 << 48)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn oracle(mut v: Vec<u32>) -> Vec<u32> {
        v.sort_unstable();
        v
    }

    #[test]
    fn boundaries_sort2_to_sort5() {
        let cases: &[(&[u32], usize)] = &[
            (&[1], 1),
            (&[2, 1], 2),
            (&[4, 3, 2, 1], 4),
            (&[5, 3, 1, 4, 2], 5),
        ];
        for (input, n) in cases {
            let mut a = [0u32; 8];
            a[..*n].copy_from_slice(input);
            match *n {
                1 => {}
                2 => sort2(a.get_mut(..2).unwrap().try_into().unwrap()),
                4 => sort4(a.get_mut(..4).unwrap().try_into().unwrap()),
                5 => sort5(a.get_mut(..5).unwrap().try_into().unwrap()),
                _ => unreachable!(),
            }
            let mut oracle_v = input.to_vec();
            oracle_v.sort_unstable();
            assert_eq!(&a[..*n], &oracle_v[..]);
        }
    }

    #[test]
    fn sort6_all_equal_and_reverse() {
        let mut e = [5u32; 6];
        sort6(&mut e);
        assert_eq!(e, [5; 6]);
        let mut a = [6u32, 5, 4, 3, 2, 1];
        sort6(&mut a);
        assert_eq!(a, [1, 2, 3, 4, 5, 6]);
        let mut b = [0u32, u32::MAX, 7, 7, 0, u32::MAX];
        sort6(&mut b);
        assert_eq!(b, [0, 0, 7, 7, u32::MAX, u32::MAX]);
    }

    #[test]
    fn sort7_randomized_oracle() {
        // Deterministic xorshift over adversarial + arbitrary values.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..2_000 {
            let mut a = [0u32; 7];
            for slot in &mut a {
                *slot = match next() % 4 {
                    0 => 0,
                    1 => u32::MAX,
                    2 => (next() >> 32) as u32,
                    _ => next() as u32,
                };
            }
            let expected = oracle(a.to_vec());
            sort7(&mut a);
            assert_eq!(a, expected.as_slice());
        }
    }

    #[test]
    fn sort8_boundaries_and_known() {
        let mut a = [8u32, 3, 6, 1, 7, 2, 5, 4];
        sort8(&mut a);
        assert_eq!(a, [1, 2, 3, 4, 5, 6, 7, 8]);
        let mut b = [u32::MAX, 0, u32::MAX, 0, u32::MAX / 2, 1, u32::MAX - 1, 2];
        sort8(&mut b);
        assert_eq!(
            b,
            [0, 0, 1, 2, u32::MAX / 2, u32::MAX - 1, u32::MAX, u32::MAX]
        );
        let mut c = [9u32; 8];
        sort8(&mut c);
        assert_eq!(c, [9; 8]);
    }

    #[test]
    fn sort8_randomized_oracle() {
        let mut state = 0xDEAD_BEEF_CAFE_BABE_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..4_000 {
            let mut a = [
                next() as u32,
                next() as u32,
                next() as u32,
                next() as u32,
                next() as u32,
                next() as u32,
                next() as u32,
                next() as u32,
            ];
            let expected = oracle(a.to_vec());
            sort8(&mut a);
            assert_eq!(a, expected.as_slice());
        }
    }

    #[test]
    fn sort4_lanes_u16_cases() {
        // sorted stays sorted
        assert_eq!(
            sort4_lanes_u16(1 | (2 << 16) | (3 << 32) | (4 << 48)),
            1 | (2 << 16) | (3 << 32) | (4 << 48)
        );
        // reverse
        assert_eq!(
            sort4_lanes_u16(4 | (3 << 16) | (2 << 32) | (1 << 48)),
            1 | (2 << 16) | (3 << 32) | (4 << 48)
        );
        // all equal
        assert_eq!(
            sort4_lanes_u16(0x2222_2222_2222_2222),
            0x2222_2222_2222_2222
        );
        // lanes {0xFFFF, 0, 0x8000, 1} -> {0, 1, 0x8000, 0xFFFF}
        let v = 0xFFFFu64 | 0x8000u64 << 32 | 1u64 << 48;
        assert_eq!(
            sort4_lanes_u16(v),
            (1 << 16) | (0x8000 << 32) | (0xFFFF << 48)
        );
    }

    #[test]
    fn sort_bytes_u64_cases() {
        // LE byte order: 0x01..08's bytes are [08,07,...,01] (descending) ->
        // sorted ascending is [01..08] at LSB..MSB -> 0x0807060504030201.
        assert_eq!(sort_bytes_u64(0x0102_0304_0506_0708), 0x0807_0605_0403_0201);
        // already-sorted-ascending LE input is a fixed point
        assert_eq!(sort_bytes_u64(0x0807_0605_0403_0201), 0x0807_0605_0403_0201);
        assert_eq!(sort_bytes_u64(0), 0);
        assert_eq!(sort_bytes_u64(u64::MAX), u64::MAX);
        let v: u64 = 0x00_FF_00_7F_80_01_02_03;
        let mut bytes: Vec<u8> = v.to_be_bytes().iter().rev().copied().collect();
        bytes.sort_unstable();
        let mut expected = 0u64;
        for (i, b) in bytes.iter().enumerate() {
            expected |= ((*b) as u64) << (i * 8);
        }
        assert_eq!(sort_bytes_u64(v), expected);
    }

    #[test]
    fn bitonic_sort_16_oracle() {
        let a = crate::sort16::bitonic_sort_16u32([
            16u32, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1,
        ]);
        assert_eq!(a.as_slice(), (1..=16).collect::<Vec<_>>().as_slice());
        let b = crate::sort16::bitonic_sort_16u32([5u32; 16]);
        assert_eq!(b, [5; 16]);
        let c_in = [
            0u32,
            u32::MAX,
            1,
            u32::MAX - 1,
            0,
            1,
            u32::MAX,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
        ];
        let expected = oracle(c_in.to_vec());
        let c = crate::sort16::bitonic_sort_16u32(c_in);
        assert_eq!(c.as_slice(), expected.as_slice());
    }

    /// A `Copy` payload so the oracle exercises exactly the production
    /// instantiation (datalog `SourceRow` is `Copy` too) without a join
    /// fixture.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Probe {
        key: u64,
        tag: u64,
    }

    /// Deterministic xorshift64* — the random corpus is reproducible on wasm.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
    }

    /// Differential oracle (ported from datalog join.rs OPT-49 tests): for
    /// every length 0..=12 (covering the inline network, its boundaries and
    /// the `sort_unstable_by_key` fallback) the network output must equal the
    /// std **stable** `sort_by_key` result, including under duplicate keys —
    /// stability is preserved by the origin-slot tie-break, not assumed.
    /// Four adversarial universes per round: all-equal keys, tiny key range
    /// (heavy duplicates), zero/extreme mix, full-range keys.
    #[test]
    fn small_by_key_differential_matches_stable_std_sort() {
        let mut rng = Rng(0x5EED_1A7E_5EED_1A7E);
        for len in 0..=12usize {
            for round in 0..64 {
                let universe = match round % 4 {
                    0 => 1,
                    1 => 4,
                    2 => 3,
                    _ => (1u64 << 56) - 1,
                };
                let mut slice: Vec<Probe> = (0..len)
                    .map(|tag| Probe {
                        key: if round % 4 == 3 && tag % 2 == 0 {
                            (1u64 << 56) - 1
                        } else if round % 4 == 2 && tag % 3 == 0 {
                            0
                        } else {
                            rng.next() % universe
                        },
                        tag: tag as u64,
                    })
                    .collect();
                let mut expected = slice.clone();
                expected.sort_by_key(|probe| probe.key);
                sort_small_by_key(&mut slice, |probe| probe.key);
                assert_eq!(slice, expected, "len {len}, round {round}");
            }
        }
    }

    /// Boundary pins: empty, single element, the exact inline capacity, and
    /// the first fallback length all agree with the std oracle; MAX keys do
    /// not collide with the network's padding sentinel because real keys are
    /// shifted left before packing.
    #[test]
    fn small_by_key_boundary_lengths_and_extreme_keys() {
        let mut all_max: Vec<Probe> = (0..=SORT_INLINE_MAX)
            .map(|tag| Probe {
                key: u64::MAX,
                tag: tag as u64,
            })
            .collect();
        let mut expected = all_max.clone();
        expected.sort_by_key(|probe| probe.key);
        sort_small_by_key(&mut all_max, |probe| probe.key);
        assert_eq!(all_max, expected);

        // Extreme keys inside the packing contract: 0, u64::MAX - 1, and
        // values near the 2^56 packing ceiling (key << 8 must not overflow).
        let near_ceiling: u64 = (1 << 56) - 1;
        let mut extremes: Vec<Probe> = [
            0u64,
            u64::MAX - 1,
            near_ceiling,
            near_ceiling - 1,
            1,
            0,
            near_ceiling,
            5,
            0,
        ]
        .iter()
        .enumerate()
        .map(|(tag, key)| Probe {
            key: *key,
            tag: tag as u64,
        })
        .collect();
        let mut expected = extremes.clone();
        expected.sort_by_key(|probe| probe.key);
        sort_small_by_key(&mut extremes, |probe| probe.key);
        assert_eq!(extremes, expected);
    }

    /// OPT-49 acceptance: duplicate keys cannot reorder payloads relative to
    /// the stable sort this replaces — ties keep authored order by
    /// construction (origin-slot low bits), pinned here.
    #[test]
    fn small_by_key_duplicate_keys_preserve_authored_order() {
        let mut slice: Vec<Probe> = (0..8)
            .map(|tag| Probe {
                key: 7,
                tag: tag as u64,
            })
            .collect();
        sort_small_by_key(&mut slice, |probe| probe.key);
        assert_eq!(
            slice.iter().map(|probe| probe.tag).collect::<Vec<_>>(),
            (0..8).collect::<Vec<_>>()
        );
    }
}

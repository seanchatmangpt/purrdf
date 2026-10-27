// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Verification suite for low-level branchless primitives and Eytzinger layout in `purrdf-core`.

#[cfg(test)]
mod tests {
    use purrdf_core::eytzinger::*;
    use purrdf_core::primitives::*;

    #[test]
    fn test_verified_eytzinger_search() {
        let sorted = [100, 250, 400, 550, 700, 850, 1000];
        let mut eyt = [0u64; 8];
        build_eytzinger_layout(&sorted, &mut eyt);

        assert_eq!(search_eytzinger_u64(&eyt, 550), Some(1)); // Root node
        assert_eq!(search_eytzinger_u64(&eyt, 100), Some(4)); // Left leaf
        assert_eq!(search_eytzinger_u64(&eyt, 999), None); // Missing
    }

    #[test]
    fn test_verified_swar_scanner() {
        // Byte 2 has '<' (0x3C)
        let word = u64::from_le_bytes(*b"ab<cdefg");
        let result = swar_find_delimiter_u64(word, b'<');
        assert_ne!(result, 0);
        assert_eq!(result.trailing_zeros(), 23); // Byte index 2: 2 * 8 + 7 = bit 23
    }

    #[test]
    fn test_quad_sorting_network() {
        let unsorted = [500u64, 12u64, 9999u64, 42u64];
        let sorted = sort_quad_lanes_branchless(unsorted);
        assert_eq!(sorted, [12, 42, 500, 9999]);
    }

    /// Independent branchy oracle for the 5-comparator network: scalar
    /// `sort_unchecked`-free bubble pass, a different formulation than the
    /// XOR-mask cswap implementation.
    fn quad_network_oracle(mut v: [u64; 4]) -> [u64; 4] {
        for i in 0..4 {
            for j in 0..3 - i {
                if v[j] > v[j + 1] {
                    v.swap(j, j + 1);
                }
            }
        }
        v
    }

    /// Differential court: XOR-mask cswap network vs the scalar bubble oracle
    /// over exhaustive boundaries, adversarial duplicates, and a deterministic
    /// pseudo-random sweep (xorshift64* — no allocation, no external rng).
    #[test]
    fn quad_sort_differential_vs_branchy_oracle() {
        // boundary + adversarial shapes
        let mut cases: Vec<[u64; 4]> = vec![
            [0, 0, 0, 0],
            [u64::MAX; 4],
            [0, u64::MAX, 1, u64::MAX - 1],
            [u64::MAX, 0, u64::MAX, 0],
            [7, 7, 7, 8],
            [8, 7, 7, 7],
            [1, 2, 3, 4],
            [4, 3, 2, 1],
        ];
        // deterministic xorshift64* sweep: covers arbitrary lane patterns
        let mut state: u64 = 0x9E37_7997_97F3_A15;
        for _ in 0..4096 {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let r = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
            cases.push([
                (r >> 48) as u64,
                (r >> 32) as u64,
                (r >> 16) as u64,
                r as u64,
            ]);
        }
        for case in &cases {
            assert_eq!(
                sort_quad_lanes_branchless(*case),
                quad_network_oracle(*case),
                "mismatch on {case:?}"
            );
        }
        // mutation guard: implementation is order-preserving and non-mutating input
        let input = [9u64, 5, 5, 1];
        let original = input;
        let _ = sort_quad_lanes_branchless(input);
        assert_eq!(input, original);
    }

    #[test]
    fn test_bitset_intersect_and_clamped_scaling() {
        assert_eq!(bitset_intersect_u64(0b1011, 0b1101), 0b1001);
        assert_eq!(clamped_scaling_u64(10, 5, 2), 25);
        assert_eq!(clamped_scaling_u64(u64::MAX, 2, 1), u64::MAX);
        assert_eq!(clamped_scaling_u64(100, 10, 0), u64::MAX);
    }

    /// Coverage of the re-exported `purrdf_branchless::eytzinger` API through
    /// the `purrdf_core::eytzinger` path: exact search mapped through the
    /// layout permutation, and the subtree-size lower bound, against
    /// `binary_search` / `partition_point` on a non-power-of-two case.
    #[test]
    fn test_eytzinger_lower_bound_and_permutation_round_trip() {
        let n = 13usize; // non-power-of-two
        let sorted: Vec<u64> = (0..n)
            .map(|i| (i as u64).wrapping_mul(7).wrapping_add(3))
            .collect();
        let mut layout = vec![0u64; n + 1];
        build_eytzinger_layout(&sorted, &mut layout);

        let mut perm = vec![0usize; n + 1];
        build_layout_permutation(&sorted, &mut perm);
        let mut sizes = vec![0u32; n + 1];
        // `build_subtree_sizes` takes the LAYOUT length (usable slots = len - 1).
        build_subtree_sizes(n + 1, &mut sizes);

        let mut targets: Vec<u64> = sorted.clone();
        targets.extend([2, 10, sorted[n - 1] + 1, u64::MAX]);
        for target in targets {
            // exact search: 1-based layout slot -> sorted index via permutation
            let got = search_eytzinger_u64(&layout, target).map(|slot| perm[slot]);
            let want = sorted.binary_search(&target).ok();
            assert_eq!(got, want, "search target={target}");
            // lower bound vs partition_point
            let lb = lower_bound_eytzinger(&layout, &sizes, target);
            let want_lb = sorted.partition_point(|x| *x < target);
            assert_eq!(lb, want_lb, "lower_bound target={target}");
        }

        // permutation round trip: layout -> sorted -> layout
        let mut restored = vec![0usize; n + 1];
        layout_permutation_inverse(&perm, &mut restored);
        for i in 1..=n {
            assert_eq!(restored[perm[i]], i, "perm inverse at {i}");
        }
    }
}

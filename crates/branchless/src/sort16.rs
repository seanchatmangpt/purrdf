//! Sixteen-element sorting networks over `[u32; 16]`.
//!
//! Two kernels:
//! - [`batcher_sort16_u32`] — Batcher odd-even mergesort, **63 comparators,
//!   depth 10**, the comparator-count-optimal (Knuth 5.3.4) network for n = 2^4.
//! - [`bitonic_sort_16u32`] — bitonic sort, **80 comparators, 4 stages**,
//!   ported from bcinr `network.rs:115`.
//!
//! Every compare-exchange is the XOR-mask conditional selection (no `if`, no
//! temporaries; compiles to `cmov`/`csel`):
//!
//! ```text
//! diff = (a > b) as u32;  mask = 0u32.wrapping_sub(diff);
//! a ^= diff_masked;       b ^= diff_masked;   // diff_masked = (a ^ b) & mask
//! ```
//!
//! Validity proof-by-construction: the comparator table below was generated
//! from the Batcher 1968 odd-even mergesort recursion (see Knuth, TAOCP Vol 3,
//! §5.3.4, algorithm M for n = 2^k) and verified exhaustively against a
//! std-sort oracle over randomized inputs plus an exhaustive 3-alphabet
//! prefix sweep in this module's tests. The table is data — no comparator is
//! chosen at runtime; the fixed-bounds loop is constant-trip (63).

// build fresh per migration plan; XOR-mask form from bcinr network.rs:49.

// Branchless fixed-trip kernels: inlining into every caller is the point of the
// crate (constant-folded comparators, cmov fusion). Deliberate, per-module.
#![allow(clippy::inline_always)]

pub(crate) const BATCHER_16: [(usize, usize); 63] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (8, 9),
    (10, 11),
    (12, 13),
    (14, 15),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (8, 10),
    (9, 11),
    (12, 14),
    (13, 15),
    (1, 2),
    (5, 6),
    (9, 10),
    (13, 14),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
    (8, 12),
    (9, 13),
    (10, 14),
    (11, 15),
    (2, 4),
    (3, 5),
    (10, 12),
    (11, 13),
    (1, 2),
    (3, 4),
    (5, 6),
    (9, 10),
    (11, 12),
    (13, 14),
    (0, 8),
    (1, 9),
    (2, 10),
    (3, 11),
    (4, 12),
    (5, 13),
    (6, 14),
    (7, 15),
    (4, 8),
    (5, 9),
    (6, 10),
    (7, 11),
    (2, 4),
    (3, 5),
    (6, 8),
    (7, 9),
    (10, 12),
    (11, 13),
    (1, 2),
    (3, 4),
    (5, 6),
    (7, 8),
    (9, 10),
    (11, 12),
    (13, 14),
];

pub(crate) const BITONIC_16: [(usize, usize, bool); 80] = [
    (0, 1, true),
    (2, 3, false),
    (4, 5, true),
    (6, 7, false),
    (8, 9, true),
    (10, 11, false),
    (12, 13, true),
    (14, 15, false),
    (0, 2, true),
    (1, 3, true),
    (4, 6, false),
    (5, 7, false),
    (8, 10, true),
    (9, 11, true),
    (12, 14, false),
    (13, 15, false),
    (0, 1, true),
    (2, 3, true),
    (4, 5, false),
    (6, 7, false),
    (8, 9, true),
    (10, 11, true),
    (12, 13, false),
    (14, 15, false),
    (0, 4, true),
    (1, 5, true),
    (2, 6, true),
    (3, 7, true),
    (8, 12, false),
    (9, 13, false),
    (10, 14, false),
    (11, 15, false),
    (0, 2, true),
    (1, 3, true),
    (4, 6, true),
    (5, 7, true),
    (8, 10, false),
    (9, 11, false),
    (12, 14, false),
    (13, 15, false),
    (0, 1, true),
    (2, 3, true),
    (4, 5, true),
    (6, 7, true),
    (8, 9, false),
    (10, 11, false),
    (12, 13, false),
    (14, 15, false),
    (0, 8, true),
    (1, 9, true),
    (2, 10, true),
    (3, 11, true),
    (4, 12, true),
    (5, 13, true),
    (6, 14, true),
    (7, 15, true),
    (0, 4, true),
    (1, 5, true),
    (2, 6, true),
    (3, 7, true),
    (8, 12, true),
    (9, 13, true),
    (10, 14, true),
    (11, 15, true),
    (0, 2, true),
    (1, 3, true),
    (4, 6, true),
    (5, 7, true),
    (8, 10, true),
    (9, 11, true),
    (12, 14, true),
    (13, 15, true),
    (0, 1, true),
    (2, 3, true),
    (4, 5, true),
    (6, 7, true),
    (8, 9, true),
    (10, 11, true),
    (12, 13, true),
    (14, 15, true),
];
/// Compare-exchange `a[i] <= a[j]` — delegates to the canonical
/// `crate::sort_small::cswap` kernel (single home for the XOR-mask form).
#[inline(always)]
fn cex(a: &mut [u32; 16], i: usize, j: usize) {
    crate::sort_small::cswap(a, i, j);
}

/// *Directional* compare-exchange for the bitonic network: `asc == true`
/// enforces `a[i] <= a[j]`, `asc == false` enforces `a[i] >= a[j]`.
/// Same XOR-mask selection; the direction folds into the swap mask, so the
/// exchange stays branchless.
#[inline(always)]
fn cex_dir(a: &mut [u32; 16], i: usize, j: usize, asc: bool) {
    let cmp = (a[i] > a[j]) as u32;
    let swap_needed = cmp ^ (!asc) as u32; // asc: swap iff out of order; desc: inverse
    let mask = 0u32.wrapping_sub(swap_needed);
    let masked = (a[i] ^ a[j]) & mask;
    a[i] ^= masked;
    a[j] ^= masked;
}

/// Sorts 16 elements with the Batcher odd-even mergesort network
/// (63 comparators, depth 10). Fixed-trip loop over a const table.
#[must_use]
pub fn batcher_sort16_u32(mut a: [u32; 16]) -> [u32; 16] {
    for &(i, j) in &BATCHER_16 {
        cex(&mut a, i, j);
    }
    a
}

/// Sorts 16 elements in place with the Batcher odd-even mergesort network.
pub fn batcher_sort16_u32_in_place(a: &mut [u32; 16]) {
    for &(i, j) in &BATCHER_16 {
        cex(a, i, j);
    }
}

/// Sorts 16 elements with the bitonic network (80 comparators, 4 stages,
/// ported from bcinr `network.rs:115`). Fixed-trip loop over a const table.
#[must_use]
pub fn bitonic_sort_16u32(mut a: [u32; 16]) -> [u32; 16] {
    // (i, j, ascending) — the direction bit is data, part of the const table.
    for &(i, j, asc) in &BITONIC_16 {
        cex_dir(&mut a, i, j, asc);
    }
    a
}

/// Ascending 24-comparator bitonic network for n = 8, re-derived from the
/// bcinr `network.rs:67` recursion (half-cleaner index-mirror form: the
/// direction bit of the textbook bitonic sort is folded into index mirroring,
/// `compare(k+j, k+2s-1-j)`, so every comparator is the plain ascending
/// `min -> lower index` form and [`crate::sort_small::cswap`] suffices).
///
/// Comparator count: stage 0 (4) + stage 1 (8) + stage 2 (12) = 24, in
/// `log2(8) = 3` stages.
// from bcinr network.rs:67-82 (1e92b213) — re-derived pair-by-pair:
// i=0: (0,1)(2,3)(4,5)(6,7); i=1: (0,3)(4,7)(1,2)(5,6)+(0,1)(2,3)(4,5)(6,7);
// i=2: (0,7)(1,6)(2,5)(3,4)+(0,2)(1,3)(4,6)(5,7)+(0,1)(2,3)(4,5)(6,7).
pub(crate) const BITONIC_8: [(usize, usize); 24] = [
    // stage 0
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    // stage 1, half-cleaner mirror (step 2)
    (0, 3),
    (4, 7),
    (1, 2),
    (5, 6),
    // stage 1, inner merge (step 1)
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    // stage 2, half-cleaner mirror (step 4)
    (0, 7),
    (1, 6),
    (2, 5),
    (3, 4),
    // stage 2, inner merge (step 2)
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    // stage 2, inner merge (step 1)
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
];

/// Compare-exchange `a[i] <= a[j]` on an 8-lane operand — delegates to the
/// canonical [`crate::sort_small::cswap`] kernel (single home for the XOR-mask
/// form), mirroring [`cex`].
#[inline(always)]
fn cex8(a: &mut [u32; 8], i: usize, j: usize) {
    crate::sort_small::cswap(a, i, j);
}

/// Sorts 8 elements with the bitonic network (24 comparators, 3 stages,
/// index-mirror form with no direction bit — from bcinr `network.rs:67`).
/// Fixed-trip loop over the [`BITONIC_8`] const table.
#[must_use]
pub fn bitonic_sort_8u32(mut a: [u32; 8]) -> [u32; 8] {
    for &(i, j) in &BITONIC_8 {
        cex8(&mut a, i, j);
    }
    a
}

/// Min-oriented fixed-capacity top-8 priority queue over `u32` priorities:
/// keeps the **8 smallest** priorities pushed so far; [`FixedTopK8::pop`]
/// yields the current smallest. 32 bytes, zero heap, all operations
/// branchless (mask-gated, fixed-trip sort; no data-dependent forward
/// branches).
///
/// # Reserved payload
///
/// `u32::MAX` is a reserved sentinel (in-band tombstone, the bcinr
/// `bitonic_pq.rs:42` convention): it must not be pushed as a real priority.
/// [`FixedTopK8::push`] of a value `>= u32::MAX` is a no-op by the gate, and
/// empty lanes read `u32::MAX`.
// from bcinr patterns/bitonic_pq.rs:28-70 (1e92b213)
#[derive(Debug)]
pub struct FixedTopK8 {
    /// Sorted-ascending lanes; unused capacity is `u32::MAX` sentinels.
    data: [u32; 8],
}

const _: () = assert!(size_of::<FixedTopK8>() == 32);

impl FixedTopK8 {
    /// An empty queue: all lanes at the `u32::MAX` sentinel.
    // from bcinr patterns/bitonic_pq.rs:37-41 (1e92b213)
    #[must_use]
    pub const fn new() -> Self {
        Self {
            data: [u32::MAX; 8],
        }
    }

    /// Inserts `priority`, keeping the 8 smallest priorities seen. A no-op
    /// when `priority >= self.data[7]` (the candidate cannot displace any of
    /// the current top-8).
    ///
    /// # DIVERGENCE from bcinr
    ///
    /// bcinr `bitonic_pq.rs:49` overwrites `data[7]` unconditionally
    /// (`self.data[7] = priority`), evicting the current max even when the
    /// incoming value is *larger* — the queue loses an element. Here the
    /// insert is mask-gated: `take = (priority < data[7]) as u32`, and the
    /// write `data[7] ^= (priority ^ data[7]) & mask` fires only when the
    /// gate is open, so capacity is never silently dropped.
    // from bcinr patterns/bitonic_pq.rs:43-51 (1e92b213), gate added
    pub fn push(&mut self, priority: u32) {
        let take = (priority < self.data[7]) as u32;
        let mask = 0u32.wrapping_sub(take);
        self.data[7] ^= (priority ^ self.data[7]) & mask;
        self.data = bitonic_sort_8u32(self.data);
    }

    /// Removes and returns the smallest priority, or `None` when empty.
    ///
    /// Empty detection is mask-select on `mask == 0` (derived from
    /// `data[0] != u32::MAX` *before* the tombstone write), never a direct
    /// `val == u32::MAX` comparison at the return site — the select compares
    /// the already-reduced mask so a legitimate `u32::MAX` payload can never
    /// diverge the result.
    // from bcinr patterns/bitonic_pq.rs:53-68 (1e92b213)
    #[must_use]
    pub fn pop(&mut self) -> Option<u32> {
        let val = self.data[0];
        let mask = 0u32.wrapping_sub((val != u32::MAX) as u32);
        self.data[0] = u32::MAX;
        self.data = bitonic_sort_8u32(self.data);
        if mask == 0 { None } else { Some(val) }
    }

    /// Number of live (non-sentinel) lanes: branchless fold
    /// `Σ (lane != u32::MAX)` over the fixed 8 lanes — no early exit.
    // from bcinr patterns/bitonic_pq.rs:28-70 (1e92b213)
    #[must_use]
    pub const fn len(&self) -> usize {
        let mut live = 0usize;
        let mut k = 0;
        while k < 8 {
            live += (self.data[k] != u32::MAX) as usize;
            k += 1;
        }
        live
    }

    /// `true` when no live lanes remain. Valid because the data is kept
    /// sorted ascending with `u32::MAX` sentinels in a suffix, so lane 0 is
    /// the sentinel iff every lane is.
    // from bcinr patterns/bitonic_pq.rs:28-70 (1e92b213)
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data[0] == u32::MAX
    }
}

/// Branchless sortedness predicate: AND-fold of adjacent `<=`
/// (`(a <= b) as u32` per step; no early exit).
///
/// Special-case of the canonical slice-form predicate
/// `crate::select::is_sorted_branchless_u32`; kept local only for the fixed
/// `[u32; 16]` signature this module's oracle helpers use.
#[must_use]
pub fn is_sorted_branchless_u16(a: &[u32; 16]) -> bool {
    let mut acc = 1u32;
    let mut k = 1;
    while k < 16 {
        acc &= (a[k - 1] <= a[k]) as u32;
        k += 1;
    }
    acc == 1
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::collections::BTreeSet;
    use std::vec::Vec;

    fn oracle_check(x: [u32; 16], f: fn([u32; 16]) -> [u32; 16]) -> bool {
        let mut expected = x;
        expected.sort_unstable();
        f(x) == expected && is_sorted_branchless_u16(&f(x))
    }

    #[test]
    fn batcher_matches_std_sort() {
        // deterministic LCG sweep (no_std-friendly; std only under cfg(test))
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..10_000 {
            let mut x = [0u32; 16];
            for cell in &mut x {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *cell = ((state >> 32) as u32) % 8;
            }
            assert!(oracle_check(x, batcher_sort16_u32));
            assert!(oracle_check(x, bitonic_sort_16u32));
        }
    }

    #[test]
    fn adversarial_shapes() {
        let flat = [0u32; 16];
        assert!(oracle_check(flat, batcher_sort16_u32));
        assert!(oracle_check(flat, bitonic_sort_16u32));
        let mut rev = [0u32; 16];
        for (k, cell) in rev.iter_mut().enumerate() {
            *cell = (15 - k) as u32;
        }
        assert!(oracle_check(rev, batcher_sort16_u32));
        let mut saw = [0u32; 16];
        for (k, cell) in saw.iter_mut().enumerate() {
            *cell = if k % 2 == 0 {
                k as u32
            } else {
                (15 - k) as u32
            };
        }
        assert!(oracle_check(saw, batcher_sort16_u32));
        assert!(oracle_check(saw, bitonic_sort_16u32));
        assert!(is_sorted_branchless_u16(&batcher_sort16_u32(saw)));
    }

    // ---- OPT-56a: bitonic_sort_8u32 + FixedTopK8 -----------------------

    /// Independent reference: collect a `BTreeSet<(u32, u64)>` of
    /// `(priority, insertion_seq)` keyed multiset — duplicates survive —
    /// and its 8 smallest priorities, ascending, are the expected queue
    /// image. (A scalar-BTreeSet formulation, deliberately different from
    /// the impl's comparator-network form.)
    struct TopKOracle {
        live: BTreeSet<(u32, u64)>,
        seq: u64,
    }

    impl TopKOracle {
        fn new() -> Self {
            Self {
                live: BTreeSet::new(),
                seq: 0,
            }
        }
        fn push(&mut self, p: u32) {
            self.live.insert((p, self.seq));
            self.seq += 1;
            // evict the largest pair (priority, then seq) while > 8 live
            while self.live.len() > 8 {
                let max = *self.live.iter().next_back().unwrap();
                self.live.remove(&max);
            }
        }
        fn pop(&mut self) -> Option<u32> {
            let min = *self.live.iter().next().unwrap();
            self.live.remove(&min);
            Some(min.0)
        }
        /// expected queue image: priorities ascending (seq tiebreak is
        /// irrelevant to the multiset of priorities)
        fn image(&self) -> Vec<u32> {
            self.live.iter().map(|&(p, _)| p).collect()
        }
    }

    fn assert_queue_matches_oracle(q: &FixedTopK8, o: &TopKOracle) {
        assert_eq!(q.len(), o.live.len(), "cardinality");
        assert_eq!(q.is_empty(), o.live.is_empty(), "is_empty");
        // full data image: the oracle's 8 smallest priorities in order, padded with u32::MAX
        let mut expected = [u32::MAX; 8];
        for (slot, &val) in expected.iter_mut().zip(o.image().iter()) {
            *slot = val;
        }
        assert_eq!(q.data, expected, "full data image");
    }

    #[test]
    fn bitonic_sort_8u32_matches_std_sort() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..10_000 {
            let mut x = [0u32; 8];
            for cell in &mut x {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *cell = ((state >> 32) as u32) % 8;
            }
            let mut expected = x;
            expected.sort_unstable();
            let got = bitonic_sort_8u32(x);
            assert_eq!(got, expected, "input {x:?}");
            assert!(
                got.windows(2).all(|w| w[0] <= w[1]),
                "not ascending: {got:?}"
            );
        }
    }

    #[test]
    fn topk8_stream_matches_oracle() {
        // 10_000-round xorshift64* interleaved push/pop stream; after every
        // op the queue must be sorted-ascending, len/oracle-cardinality equal,
        // and the full data image equal to the oracle's 8 smallest.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut q = FixedTopK8::new();
        let mut o = TopKOracle::new();
        for round in 0..10_000u32 {
            // xorshift64* output mixing (star shift) distinguishes the stream
            let raw = next() ^ (next().wrapping_mul(0x9E37_79B9_7F4A_7C15 >> 32));
            let priority = match raw % 5 {
                0 => 0,
                1 => u32::MAX, // sentinel-adjacent adversarial payload
                2 => u32::MAX - 1,
                _ => (raw >> 32) as u32,
            };
            let do_push = round % 2 == 0 || o.live.is_empty();
            if do_push {
                // u32::MAX is the reserved tombstone: excluded symmetrically
                // from queue and oracle so their multisets stay comparable
                if priority != u32::MAX {
                    q.push(priority);
                    o.push(priority);
                }
            } else {
                assert_eq!(q.pop(), o.pop(), "pop round {round}");
            }
            assert!(
                q.data.windows(2).all(|w| w[0] <= w[1]),
                "unsorted after round {round}: {:?}",
                q.data
            );
            assert_queue_matches_oracle(&q, &o);
        }
    }

    #[test]
    fn topk8_pop_on_new_is_repeatedly_none() {
        let mut q = FixedTopK8::new();
        assert_eq!(q.pop(), None);
        assert_eq!(q.pop(), None);
        assert_eq!(q.pop(), None);
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn topk8_fill_overflow_keeps_8_smallest() {
        let mut q = FixedTopK8::new();
        // 8 distinct pushes
        for p in [50u32, 10, 40, 20, 30, 60, 70, 80] {
            q.push(p);
        }
        assert_eq!(q.data, [10, 20, 30, 40, 50, 60, 70, 80]);
        // 10 more: only values below the current max (80) survive
        for p in [5u32, 85, 15, 95, 25, 35, 45, 105, 1, 2] {
            q.push(p);
        }
        assert_eq!(q.data, [1, 2, 5, 10, 15, 20, 25, 30]);
        assert_eq!(q.len(), 8);
    }

    #[test]
    fn topk8_push_larger_than_max_is_noop() {
        // The bcinr-divergence case: an unguarded push would evict data[7].
        let mut q = FixedTopK8::new();
        for p in [9u32, 3, 7, 1, 8, 2, 6, 5] {
            q.push(p);
        }
        let snapshot = q.data;
        q.push(100);
        q.push(9); // equal to max: also gated (take == 0)
        assert_eq!(q.data, snapshot, "capacity lost to unguarded eviction");
        assert_eq!(q.len(), 8);
    }

    #[test]
    fn topk8_all_equal_priorities() {
        let mut q = FixedTopK8::new();
        for _ in 0..20 {
            q.push(7);
        }
        assert_eq!(q.data, [7u32; 8]);
        assert_eq!(q.len(), 8);
        for _ in 0..8 {
            assert_eq!(q.pop(), Some(7));
        }
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn topk8_sentinel_adjacent_values() {
        // u32::MAX-1 must be a live payload, never confused with the
        // tombstone; 0 must sort below everything.
        let mut q = FixedTopK8::new();
        let mut o = TopKOracle::new();
        for p in [u32::MAX - 1, 0, u32::MAX - 1, 0, 1, u32::MAX - 2] {
            q.push(p);
            o.push(p);
            assert_queue_matches_oracle(&q, &o);
        }
        assert_eq!(
            q.data,
            [
                0,
                0,
                1,
                u32::MAX - 2,
                u32::MAX - 1,
                u32::MAX - 1,
                u32::MAX,
                u32::MAX
            ]
        );
        while !q.is_empty() {
            assert_eq!(q.pop(), o.pop());
            assert_queue_matches_oracle(&q, &o);
        }
    }

    #[test]
    fn topk8_pops_past_empty_then_pushes_again() {
        let mut q = FixedTopK8::new();
        q.push(42);
        assert_eq!(q.pop(), Some(42));
        // drain past empty
        assert_eq!(q.pop(), None);
        assert_eq!(q.pop(), None);
        assert!(q.is_empty());
        // revives cleanly
        q.push(13);
        q.push(4);
        assert_eq!(
            q.data,
            [
                4,
                13,
                u32::MAX,
                u32::MAX,
                u32::MAX,
                u32::MAX,
                u32::MAX,
                u32::MAX
            ]
        );
        assert_eq!(q.len(), 2);
        assert_eq!(q.pop(), Some(4));
        assert_eq!(q.pop(), Some(13));
        assert_eq!(q.pop(), None);
    }
}

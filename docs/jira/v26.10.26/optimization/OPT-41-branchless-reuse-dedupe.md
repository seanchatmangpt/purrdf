# [OPT-41] Branchless Crate Reuse & Internal Dedupe Receipt

## 1. TARGET CONTEXT
- **Crate & File:** `crates/branchless/src/*` (18 modules, ~150 pub fns);
  cross-checks vs `crates/hash/src`, `crates/lex/src/scan.rs`,
  `crates/rdf-core/src/{eytzinger,primitives}.rs`, `ir/pack/bits.rs`
- **Subsystem:** Memory Arena / Relational Sieve (kernel primitives)
- **Inspection Agent:** 3 reuse-audit lanes (vs purrdf-hash/lex, vs rdf-core,
  internal dedupe) over the bcinr migration

## 2. MECHANICAL PATHOLOGY
- **Cross-crate verdicts (mandate check):** ZERO duplicates vs purrdf-hash /
  purrdf-lex — fnv/splitmix exclusion held; wyhash, tabulation, SipHash,
  knuth/fibonacci have no existing purrdf home. Word-altitude SWAR kernels
  COMPLEMENT (not duplicate) `lex::scan`'s 16-byte SIMD chunks.
- **Verbatim cross-crate copy (RESOLVED):** `rdf-core/src/eytzinger.rs` was a
  51-line byte-equivalent copy of the branchless module. Switchover LANDED:
  rdf-core now `pub use purrdf_branchless::eytzinger::*;` (dep via
  `[workspace.dependencies]`); existing `purrdf_core::eytzinger` paths
  resolve unchanged; test file unaffected.
- **Internal duplicate families: 17** (worst offenders):
  1. `sort_small::compare_exchange` ≡ `sort_small::ce` — byte-identical,
     same file (sort_small.rs:22 vs :30)
  2. `saturation::clamp_u64` ≡ `clamp_slice_u64` — byte-identical intra-file
     (saturation.rs:81 vs :103)
  3. `char_class` re-implemented swar kernels it was told to reuse:
     `classify_bytes`, `parse_8_decimal_digits`, `swar_is_in_range` (private
     clone of pub `byte_lane_in_range`)
  4. ASCII case-fold twins: `swar::to_lower/upper_ascii_word` (range-mask)
     vs `char_class::ascii_to_lower/uppercase_word` (sign-bit product) —
     same contract, divergent formulations
  5. 3-way per-word rank/select: `bitset::rank_u64` vs `bitmap::rank_word`;
     `bitset::select_bit_u64` vs `bitmap::select_word` (+ bits.rs:530
     `select_in_word` at container altitude, deliberately faster LUT)
  6. Triplicated first-byte match: `swar::first_byte_position` (O(1) tz),
     `search::first_match_byte_u64` (8-iter walk), `swar::find_first_of_word`
  7. pdep/pext twins: `simd_fallback::pdep_u64` ≡ `packing::deposit_bits`
     (both 64-iter loops, different style)
  8. min/max mask-family spread: select.rs pub fns, saturation.rs private
     byte-identical clones, simd_fallback ct_min/max wrap-add form
  9. Select kernel in 3 widths/3 modules: select_u128, ct_select_u64/i64,
     CanonicalMask::select_u32/i32 — one idea, three hand-rolls
  10. Compare-exchange across files: sort_small vs sort16 `cex` (same body)
  11. `sort16::bitonic_sort_16u32` (const table) vs `sort_small::bitonic_sort_16`
      (generated loop) — same 80-comparator network, divergent codegen
  12. is_sorted predicate in select.rs + sort16.rs
  13. 5 copies of the "chunks_exact(8) + tail" sweep skeleton (swar slice
      kernels + structural masks using per-byte closures instead of masks)
  14. Char-equality kernel: sub-form `find_byte_in_word` vs carry-safe
      `find_byte_mask_word` (strictly better)
  15. Clamp family per width: cost_math, simd_fallback, saturation each
      define their own
  16. circuit_breaker mask predicates vs ct_eq/lt vs const_lt/eq — thin
      one-liners, low priority
  17. helpers-ledger INVISIBILITY: zero `[[job]]` rows with home under
      `purrdf_branchless` — the census gate could not see the crate, which is
      exactly why 17 families accumulated unchecked

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Canonical homes (audit recommendation):**
  - `swar.rs`: byte_lane_in_range, classify_bytes, parse_8_decimal_digits,
    find_byte_mask_word (carry-safe form canonical), one sweep helper,
    per-word rank/select (from bitmap)
  - `select.rs`: min/max mask family (absorb saturation's private clones),
    is_sorted
  - `saturation.rs`: clamp family, generic over width
  - `simd_fallback.rs`: pdep/pext (packing delegates), shared ct_select kernel
  - `sort16.rs`: 16-wide networks (delete sort_small's loop-form bitonic_16);
    one shared `cswap`
- **helpers-ledger rows:** add one `[[job]]` per canonical family with the
  dup list as entry points so the census gate sees the crate going forward
- **Follow-up ticket list:**
  - T-A: char_class → swar mechanical dedupe (families 3, 4, 13 — family 4
    needs a bench call to pick the faster case-fold formulation first)
  - T-B: const-generic macro for the select/clamp/min-max width families
  - T-C: straddling `field_extract2`/`field_deposit2` (k ≤ 64) in packing,
    then refactor `IntVector::get/push` onto them
  - T-D: doc-only — annotate `bits.rs:530 select_in_word` as deliberately
    container-local (LUT speed, altitude) to close audit noise
  - T-E: unify the two bitset/bitmap per-word selects (convention adapter:
    inclusive/exclusive, Option/sentinel)

- **Follow-up status (OPT-63, verified on disk 2026-10-26):** T-E rank/
  select unification LANDED (bitmap = adapter over bitset canonical);
  T-A bench home does not exist yet (no OPT-20/39 primitives bench
  target landed; no case-fold bench hits); T-C straddling kernels do
  not exist; T-D annotation absent at bits.rs side; T-B macro absent.
  Full evidence: `OPT-63-opt41-followups.md` (same directory).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (dedupe ticket) — surviving kernels keep
  their cmov/SWAR forms; no consolidation may pick the branchier variant.
- **Allocation Invariant:** dedupe must not introduce alloc (crate stays
  no_std, zero-alloc; census gate extended with the new [[job]] rows).
- **Benchmark Target:** family 4 (case-fold) decided by the OPT-20 bench
  suite, not assertion; all other consolidations are semantically identical
  by construction.
- **Layer Boundary:** `check-layers.py` green post-switchover (271 edges /
  52 members at switchover time); `cargo check -p purrdf-core --lib` clean
  after the re-export landed.

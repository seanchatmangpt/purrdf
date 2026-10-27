# [OPT-63] OPT-41 Follow-ups T-A..T-E — Verified Status & Landing Plan

## 1. TARGET CONTEXT
- **Crate & File:** `crates/branchless/src/{swar,char_class,select,saturation,
  simd_fallback,packing,bitset,bitmap}.rs`; `crates/rdf-core/src/ir/pack/bits.rs:530`
- **Subsystem:** Relational Sieve / Memory Arena (kernel primitives)
- **Inspection Agent:** OPT-42 ticket-writer lane (verify-on-disk pass over
  OPT-41 §3 follow-up list T-A..T-E)

## 2. MECHANICAL PATHOLOGY (per item, verified on disk 2026-10-26)
- **T-A — case-fold twins + missing bench call:**
  - **Current Code Construct:** two divergent ASCII case-fold
    formulations: `swar::to_lower_ascii_word`/`to_upper_ascii_word`
    (`crates/branchless/src/swar.rs:90,99`, bcinr swar_str.rs:240/268
    provenance, range-mask form) vs
    `char_class::ascii_to_lowercase_word`/`ascii_to_uppercase_word`
    (`crates/branchless/src/char_class.rs:28,43`, sign-bit product form).
    Same contract, divergent codegen.
  - **Bench-call status:** OPT-41 §4 says family 4 is "decided by the
    OPT-20 bench suite" — verified: **that bench does not exist yet.**
    OPT-39 proposes a primitives suite under `crates/rdf-core/benches/`;
    `ls crates/rdf-core/benches/` shows 23 targets, none referencing
    `to_lower_ascii_word` or either case-fold kernel (grep: 0 hits).
    The only SWAR bench in the workspace is the local reimplementation in
    `crates/duckdb/benches/sieve_bench.rs:21-61` (OPT-39's finding,
    unchanged). There is no `[[bench]]` in
    `crates/branchless/Cargo.toml`.
  - **Hardware-Level Impact:** unresolved dedupe keeps two hot-path
    formulations live with no measured winner; consumers cannot pick the
    cache/mul-cheaper form without data.
  - **Baseline Cost:** 0 measured baselines for either case-fold form.

- **T-B — const-generic width macro for select/clamp/min-max:**
  - **Current Code Construct:** one select/clamp idea, hand-rolled per
    width and per module: `select.rs` `min_u32`/`max_u32`/`min_u64`/
    `max_u64` (select.rs:21,28,35,42) + `select_u128` (select.rs:155);
    `saturation.rs` `clamp_i64`/`clamp_u64`/`clamp_slice_u64`
    (saturation.rs:72,81,103 — last two byte-identical); `CanonicalMask::
    select_u32`/`select_i32` (saturation.rs:305,312); `simd_fallback::
    ct_select_u64/i64` (simd_fallback.rs:178,189) and `ct_clamp_u32`
    (simd_fallback.rs:324). No macro or generic exists.
  - **Feasibility (verified against the bodies):** all are mask-form
    (`(a & m) | (b & !m)`, clamp = two mask mins). Unsigned widths unify
    trivially. Signed widths already share the ordered-pair helper
    (saturation.rs:391-397 `ordered`/`ordered_u64`); the i32/i64 forms
    reuse it via the existing sign-fold. A `macro_rules!` over
    `(type, suffix)` tuples is the right tool — a true `const N: usize`
    const-generic adds nothing (widths are types, not lengths). Const
    callability is preserved by generating `const fn` bodies.
  - **Hardware-Level Impact:** N hand-rolls drift O(N²); each hand-roll
    is an independent review/mutation-test surface with zero codegen
    benefit (same cmov chains).
  - **Baseline Cost:** 0 (identical codegen by construction; gain is
    review/maintenance surface, not cycles).

- **T-C — straddling field kernels:**
  - **Current Code Construct:** `packing.rs` has single-word
    `field_deposit`/`field_extract` (packing.rs:25,36) bounded to one u64;
    `IntVector::get/push` in rdf-core `ir/pack/` does its own multi-word
    packing today. No `field_extract2`/`field_deposit2` (k ≤ 64 crossing
    a word boundary) exists anywhere in the tree (grep: 0 hits).
  - `(word_lo, word_hi)` = `(word, word >> offset)` pair, deposit is its
    inverse via the same `mask_low(k)` helper already used in packing.rs
    tests (packing.rs:328).
  - **Hardware-Level Impact:** each consumer re-rolls the
    shift-or-shift dance; a straddling bug class (offset + k > 64 edge)
    is re-opened per implementation.
  - **Baseline Cost:** n/a (new kernel, not a dedupe).

- **T-D — bits.rs:530 altitude doc:**
  - **Current Code Construct:** `bits.rs:530` `fn select_in_word(word:
    u64, r: usize) -> u32` — the multiply-based per-byte prefix-sum LUT
    form, deliberately container-local. The bitset side already carries
    the cross-reference (bitset.rs:26-29 states bitset is canonical and
    bitmap is adapters); the bits.rs side carries **no** deliberate-
    locality annotation, so audits keep re-flagging it (OPT-41 family 5).
  - **Hardware-Level Impact:** none (doc-only); the cost is recurring
    audit noise.
  - **Baseline Cost:** one audit flag per reuse pass.

- **T-E — bitset/bitmap select unification check:**
  - **Current Code Construct:** verified **already unified.**
    `bitmap::select_word` (bitmap.rs:123-127) is a sentinel-64 adapter
    over canonical `bitset::select_bit_u64` (bitset.rs:142); the rank
    side is likewise unified: `bitmap::rank_word` is the exclusive
    adapter over inclusive `bitset::rank_u64`/`rank_exclusive_u64`
    (bitmap.rs:81-87, bitset.rs:50-62), with the convention documented in
    both module headers (bitset.rs:14-29, bitmap.rs:5-7). No unassigned
    residue: the only remaining independent select is the deliberately
    container-local `bits.rs:530` kernel, which is T-D's doc item, not a
    dedupe target.
  - **Hardware-Level Impact:** none — the adapter is const, branchless,
    and inlines away.
  - **Baseline Cost:** 0; item is a status flip, not work.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **T-A:** first land the bench home, then dedupe. Add one bench target
  (e.g. `crates/rdf-core/benches/branchless_casefold.rs` — rdf-core is
  where `make bench` already sweeps) sweeping both formulations of
  `to_lower_ascii_word` ≡ `ascii_to_lowercase_word` (and upper twin) over
  word patterns (all-ASCII, mixed, high-bit-heavy, all-ones) x
  `Throughput::Elements(8)`. Record the winner, then collapse char_class
  to delegate to the winner (or delete and keep one name) and add the
  `[[job]]` row to helpers-ledger.toml (one-home-per-job; ledger already
  has the branchless section at helpers-ledger.toml:3866+ — extend, do
  not fork a second ledger section). Then execute the remaining T-A
  scope from OPT-41 families 3 and 13 (char_class→swar mechanical dedupe,
  one chunks_exact(8) sweep helper).
  - **Hard Bounds Applied:** zero-alloc, no_std preserved; winner picked
    by bench, never assertion.
  - **Proposed Type Layout & Signatures:** one survivor
    `pub fn to_lower_ascii_word(word: u64) -> u64` (same signature both
    sides already carry), upper twin likewise.
- **T-B:** one `macro_rules!` at `select.rs` generating const mask-form
  min/max/select/clamp per `(type, suffix)`:
  `min! { u32, u64, i32, i64, u128 }` — bodies are the existing single
  expressions; saturation.rs's private byte-identical clones
  (clamp_u64/clamp_slice_u64 :81/:103, private min/max at :367-:385) are
  deleted and callers re-pointed to the select.rs home; `CanonicalMask::
  select_*` stays as the typed method (delegating body) since it is a
  public API; simd_fallback's ct_select/ct_clamp keep names, bodies
  delegate. Feasibility verified: every body is a one-expression mask
  form; const-callable; no behavior change.
  - **Hard Bounds Applied:** no codegen change (same cmov chains);
    remaining hand-rolls after the macro: 0 for min/max/select/clamp.
  - **Proposed Type Layout & Signatures:**
    `pub const fn min<T: Copy>(a: T, b: T) -> T` — NO. Keep concrete
    `pub const fn min_u32(a: u32, b: u32) -> u32` etc. generated by the
    macro; concrete names preserved (no trait bound, no vtable hop,
    no monomorphization explosion; crate stays no_std).
  - **Proposed Type Layout & Signatures:** unchanged public names;
    dedupe is internal.
  - **T-C:** add to `packing.rs`:
    ```rust
    pub const fn field_extract2(lo: u64, hi: u64, k: u32, offset: u32)
        -> u64
    ```
    and `field_deposit2(word_lo, word_hi, value, k, offset) ->
    (u64, u64)`; k ≤ 64, offset < 64; extract =
    ```
    let raw = (lo >> offset) | (hi << (64 - offset));
    raw & mask_low(k)  // k==64 => all-ones, no shift by 64
    ```
    **LANDED-FORM DEVIATION (recorded 2026-10-26):** the ticket's
    `hi << (64 - offset)` snippet has a shift-by-64 hole at `offset == 0`
    (`64 - 0 == 64` overflows the u64 shift). The landed
    `field_extract2`/`field_deposit2` (`crates/branchless/src/packing.rs:253`
    onward) use the u128 window
    `((u128::from(hi) << 64) | u128::from(lo)) >> offset` instead — the
    128-bit window absorbs `offset <= 63` with no mask branch and no UB.
    Landed, UNVERIFIED (no-build machine).
  - **Hard Bounds Applied:** const, branchless (mask, no branch on k),
    zero-alloc.
  - **Proposed Type Layout & Signatures:** as above; then refactor
    `IntVector::get/push` onto them per OPT-41.
- **T-D:** doc-only. Annotate `bits.rs:530` with an altitude note:
  deliberately container-local, multiply-prefix LUT form chosen for
  directory-bounded callsites, canonical per-word kernel is
  `purrdf_branchless::bitset::select_bit_u64` (mirrors the existing
  pointer at bitset.rs:26-29). No behavior change; closes the recurring
  audit flag.
- **T-E:** status flip only. Record: rank/select unification LANDED
  (bitmap = adapter layer over bitset canonical, both module headers
  document the convention). Close the item; the only live follow-up is
  T-D's doc note on the third, deliberately separate kernel.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** survivors keep cmov/SWAR forms; T-B macro
  bodies are the exact existing expressions (differential test: macro
  output vs old fns on boundary tuples). T-C: roundtrip property test
  extract(deposit2(v)) == v for offset 0..=63, k in {1, 31, 32, 33, 63,
  64} including the k==64 no-shift-by-64 edge, plus a
  valid-neighbor check that a k=64 field at offset 0 (full-word field)
  still extracts — over-refusal is the mirror bug here too: the kernel
  must not clamp k to 63.
  - **Allocation Invariant:** crate stays no_std, zero-alloc; no new
    [[job]] fork — helpers-ledger.toml:3866+ branchless section is
    extended, never duplicated.
  - **Benchmark Target:** T-A case-fold decided by a new bench target
    (OPT-39's proposed rdf-core benches home), winner recorded in the
    dedupe receipt; all other items are semantically identical by
    construction and need no bench.
  - **Layer Boundary:** `python3 scripts/check-layers.py` stays green
    (baseline "OK: 270 first-party normal edges across 51 members");
    `cargo check -p purrdf-branchless --lib` clean after each item.

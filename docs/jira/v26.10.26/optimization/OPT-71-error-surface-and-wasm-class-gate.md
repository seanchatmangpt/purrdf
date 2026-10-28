# OPT-71: Error-surface deep items + wasm class parity gate

**Status:** OPEN (filed 2026-10-27, W4 error-surface + wasm-parity lanes)

## 1. Error surface (crates/arrow, crates/sparql-results)

Mechanical message fixes landed in 1900f7d5. Remaining structural items:

- **storage_overflow() is value-free** (canon_arrow.rs:129-131, 8 call
  sites): every overflow refusal discards the offending quantity (token
  length / lexical total / row count / slab arithmetic / payload sum).
  Fix: carry the failing quantity as a field (or a `which: &'static str`
  tag) through `StorageError::SizeOverflow` or a local wrapper.
- **frame_total() expects instead of refusing** (canon_arrow.rs:135-140):
  `checked_add(...).expect(...)` panics on a caller-supplied payload
  ≥ u64::MAX-192; the file's own policy (error.rs:9-10) says typed
  refusal. Fix: return Result and thread storage_overflow() through the
  two emit paths.
- **BadTotalLen dual-use** (arrow header.rs:154-165): the
  sub-overhead refusal (total < SIZE+64) reports "inconsistent lengths
  payload=…, total=…" when no payload comparison happened. Fix: a
  distinct variant (e.g. `BelowFrameOverhead { total_byte_len }`) or a
  dedicated message before the checked_sub chain.
- **BadBatchCount under-reports**: carries only the count; the payload
  length is in scope at both raise sites (header.rs:144-148) and the
  sibling BadTotalLen carries both operands.

## 2. wasm class parity gate

`scripts/check-wasm-js-exports.py` gates free functions only; classes
are where every observed gap lives (ProvenanceInfo re-export fixed in
1900f7d5; AsyncJob/AsyncJobOptions/ShaclJobRequest/AsyncEffect/
AsyncEvidence have no d.ts class at all; raw begin* job APIs and
Dataset.visual*Json are undocumented). Fix: extend the gate to compare
`#[wasm_bindgen] pub struct` exports against d.ts class declarations,
with an explicit allowlist for deliberately-internalized plumbing.

## Verification invariant

1: a refusal message names the value that overflowed. 2: no expect/panic
reachable from caller input in canon_arrow.rs. Gate: the extended
check-wasm-js-exports.py exits 1 when a public class goes missing from
d.ts or mjs.

> UPDATE (2026-10-27, wave-6): the ProvenanceInfo gap is CLOSED — mjs
> import+export repaired and tests/class-reexport.test.mjs added
> (1ec93da2). check-wasm-js-exports.py has class reverse-drift + d.ts
> parsing landed; the open residue is only the forward class-to-d.ts
> gate with an internalization allowlist.

> WAVE-10 SEMANTIC ADDITIONS (arrow header, 2026-10-27):
> - header.rs:141-149 — batch_count=u32::MAX with a 1-byte payload PASSES
>   header validation (only empty/nonempty consistency is checked); the
>   batch×row product is never validated anywhere in the reader path.
> - header.rs:111-112/172-175 — digests cover only the payload region;
>   flags and batch_count are protected by no digest, so a hostile frame
>   can flip them without verification failing.
> - header.rs:102-114 — to_bytes never writes `reserved`, so a non-zero
>   reserved struct serializes to a frame from_bytes would refuse
>   (write/read asymmetry).
> - header.rs:176-178 — reserved-zero check ordered after length checks;
>   BadTotalLen can mask ReservedNotZero (classification only).

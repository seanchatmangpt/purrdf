# [OPT-37] Inline BLAKE3 Receipt Minting: `sink::Blake3` + Drain-Parameterized Canonicalization

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf/src/gts_certify.rs:192-201`; `crates/rdf-core/src/ir/canon.rs:415,433,472-712`; `crates/rdf-core/src/sink.rs:305-355,361`; `crates/rdf/src/gts_compose.rs:1039-1042`; `crates/gts/src/writer.rs:389,956-958,1231,1349`; `crates/gts/src/wire.rs:50-99`; `crates/rdf-core/src/content_store.rs:43-47`
- **Subsystem:** Attestation
- **Inspection Agent:** Agent 18 (Streaming BLAKE3 State Receipts)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:** the streaming engine already exists in purrdf-hash — `Streaming<const BUFFER>` with zero-copy `update` for caller buffers ≥ BUFFER (`hash/src/blake3.rs:439-650,518,556-575`, "neither streaming nor one-shot hashing allocates"), an `io::Write` impl (:630-647), `RecordHasher = Streaming<1024>` for framed record identities, and framing helpers `frame_le_into`/`frame_le_display_into`/`frame_length_into` (`hash/src/frame.rs:138-215`). Yet the digest sites materialize-then-hash: `let canonical = try_canonicalize_with(...); let digest = Sha256::digest(canonical.nquads.as_bytes());` (gts_certify.rs:192-201) — a full N-Quads `String` built purely to be read once (`Canonicalized.nquads: String`, canon.rs:415,433); snapshot ids materialize full CBOR then one-shot hash (gts_compose.rs:1039-1042; writer.rs:389,956-958,1231,1349); `sink::Digest` wraps `sha2::Sha256` directly (sink.rs:305-355) even though its own doc names the problem ("every caller that only wants a digest has had to materialize the whole thing first") and `Tee` (:361) already exists.
- **Hardware-Level Impact:** O(document) memory residency + a second full pass over the bytes, where the bytes exist as a stream by construction; loses the SIMD BLAKE3 batching (the 16 KiB `Hasher` batches tiny writes across lanes).
- **Baseline Cost:** 2× document passes + full-document allocation per digest.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** (1) **`sink::Blake3(ByteDrain)`** next to `sink::Digest` in `sink.rs`, wrapping `purrdf_hash::blake3::Hasher` (`ByteDrain::drain(&mut self, chunk)` maps 1:1 onto `Streaming::update`); pair with the existing `Tee` so an envelope is written and receipt-hashed in one emission pass: `Tee(WriterDrain(out), Blake3(Hasher::new()))`. Unify `sink::Digest`'s sha2-direct usage onto purrdf-hash (removes the second hash implementation in the receipt path). (2) **Drain-parameterized canonicalization**: `try_canonicalize_into(ds, …, &mut dyn ByteDrain)` emitting sorted N-Quads lines straight into the drain, so `gts_certify.rs:198` becomes an inline fold; the `String` stays available when the caller wants text; `frame_le_display_into`/`frame_length_into` cover line framing without intermediates. (3) **Snapshot/genesis ids**: replace `canonical(&payload)` → `digest_str` with `purdf_lex::cbor::write_canonical(&value, &mut hasher)` — the `hash_excluding` precedent (wire.rs:85-92) proves byte-identity since both drive the same encoder. (4) **Parallel lane preserved**: `wire::blake3_256` (one-shot/`hash_with_join`, wire.rs:50-70) stays for already-resident buffers; inline minting is for emitters only. (5) **Receipt shape**: `blake3:<hex>` via `digest_label` at finalize; domain-separate with `Domain`/`frame_le_labelled` rather than changing preimages — "a domain cannot be added without breaking every recorded id for an existing archive" (gts_compose.rs:1027-1037), so recorded ids stay stable.
- **Hard Bounds Applied:** zero-alloc hashing (in-tree Streaming guarantee, miri-testable with the bounded 54-entry stack); 16 KiB batching window; no format/domain change to existing archives.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct Blake3Drain(purrdf_hash::blake3::Hasher);            // impl ByteDrain
  fn try_canonicalize_into(ds: &Dataset, drain: &mut dyn ByteDrain) -> Result<Digest, CanonError>;
  ```

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A; SIMD backend selection unchanged (Portable→Avx512/Neon per availability, blake3.rs:26-124).
- **Allocation Invariant:** streaming path allocates 0 (in-tree guarantee); digest computed without materializing N-Quads/CBOR.
- **Benchmark Target:** ≥12 GB/s class throughput on resident buffers (SIMD BLAKE3 state); byte-identity: `write_canonical`-fed digest == one-shot `canonical()` digest on the golden/frozen-vector suites (purrdf-hash-conformance, `columnar/tests/golden_bytes.rs`).
- **Layer Boundary:** purrdf-hash stays the zero-dependency root; rdf-core consumes it (layering per `layers.toml`); no new hash implementation (`helpers-ledger.toml` one-home rule).

# [OPT-18] Inline-Mint Receipts: sink::Blake3 Drain + Drain-Parameterized Canonicalization

## 1. TARGET CONTEXT
- **Crate & File:** `crates/rdf/src/gts_certify.rs:192-201`; `crates/rdf/src/gts_compose.rs:1039-1042`; `crates/gts/src/writer.rs:389,956-958,1231,2171,2243`; `crates/rdf-core/src/sink.rs:305-355,361`; `crates/rdf-core/src/content_store.rs:43-47`
- **Subsystem:** Attestation
- **Inspection Agent:** Agent 18 (Streaming BLAKE3 State Receipts)

## 2. MECHANICAL PATHOLOGY
- **Current Code Construct:**
  - `gts_certify.rs:192-201` — `let canonical = try_canonicalize_with(&dataset, CanonHash::Sha256)...; let digest = Sha256::digest(canonical.nquads.as_bytes());` where `Canonicalized.nquads: String` (`crates/rdf-core/src/ir/canon.rs:415,433`) — every `try_canonicalize*` variant materializes the full N-Quads document before a one-shot hash.
  - `gts_compose.rs:1039-1042` — `let bytes = canonical(&self.snapshot_payload()); digest_str(&bytes)` — full CBOR `Vec<u8>` resident, then one-shot `blake3::hash`.
  - `gts/writer.rs:389,956-958,1231,2171,2243` — `canonical(&payload)` → hash at each site (note `:1322` `append_canonical` already streams frame emission — only the hash is not incremental).
  - `content_store.rs:43-47` — `ContentDigest::of(bytes)` = one-shot SHA-256 over fully resident bytes.
- **Hardware-Level Impact:** O(document) heap residency + allocator churn purely to feed a hasher that never needed the bytes; double pass over the whole canonical form (write, then read-once-into-hash); SHA-256 (non-SIMD in-house path) where an SIMD BLAKE3 streaming state exists.
- **Baseline Cost:** full N-Quads String build + BLAKE3/SHA-256 over it, per certify/snapshot; the codebase itself names the defect — `sink.rs:305-355` doc: "every caller that only wants a digest has had to materialize the whole thing first purely to hand it over."

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Architectural Proposal:** Capability already exists; this is an integration ticket, not a capability build.
  1. **`sink::Blake3(ByteDrain)`** in `crates/rdf-core/src/sink.rs` next to `sink::Digest(Sha256)` (`:309-355`), wrapping `purrdf_hash::blake3::Hasher` (= `Streaming<16384>`, `crates/hash/src/blake3.rs:445-450`). `ByteDrain::drain(&mut self, chunk)` maps 1:1 onto `Streaming::update` (`:518`), which consumes caller buffers ≥ BUFFER directly with zero copy (`:556-575`). Pair with the existing `sink::Tee` (`:361`): `Tee(WriterDrain(out), Blake3(Hasher::new()))` — write and receipt-digest in one emission pass.
  2. **Drain-parameterized canonicalization:** `try_canonicalize_into(ds, ..., &mut dyn ByteDrain)` emitting sorted N-Quads lines straight into the drain; the `String` variant stays for callers wanting text. `frame_le_display_into` / `frame_length_into` (`crates/hash/src/frame.rs:172-215`) cover line-length framing with no intermediate allocation.
  3. **Snapshot/genesis ids:** replace `canonical(&payload)` → `digest_str` (`gts_compose.rs:1041`, `writer.rs:389,956-958,1231`) with `purrdf_lex::cbor::write_canonical(&value, &mut hasher)` — the `hash_excluding` precedent (`gts/src/wire.rs:85-92`) proves byte-identity (same encoder), pinned by frozen suites (`purrdf-hash-conformance`, `crates/columnar/tests/golden_bytes.rs`).
  4. **Parallel lane preserved:** `wire::blake3_256` (`wire.rs:50-70`, one-shot / `hash_with_join` + rayon) remains the path for already-resident buffers; inline minting applies only to emitters where bytes exist as a stream by construction.
  5. **Unify `sink::Digest` onto purrdf-hash** — it currently wraps `sha2` directly (`sink.rs:309,349`) despite rdf-core already depending on purrdf-hash; removes the second hash implementation from the receipt path.
- **Hard Bounds Applied:** zero allocations in the streaming state (blake3.rs:12: "neither streaming nor one-shot hashing allocates"; bounded 54-entry chunk stack, `Streaming` `stack: [[u32; 8]; 54]`) — miri-clean by construction; 16 KiB `Hasher` batches tiny writes across SIMD lanes.
- **Proposed Type Layout & Signatures:**
  ```rust
  pub struct Blake3(purrdf_hash::blake3::Hasher);          // sink::Blake3
  impl ByteDrain for Blake3 { fn drain(&mut self, chunk: &[u8]) -> ... { self.0.update(chunk); ... } }
  fn try_canonicalize_into(ds: &Dataset, ..., drain: &mut dyn ByteDrain) -> Result<DigestReceptacle, ...>;
  ```
  Streaming backends already span Portable/Sse2/Ssse3/Avx2/Avx512/Neon/Wasm128 (`blake3.rs:26-46`); in-repo streaming precedents to copy: `copy_counted_and_hash` (`gts/src/files.rs:504-530`, 16 KiB Tee over I/O) and MMR `RecordHasher` (`gts/src/mmr.rs:281-295`).

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Branchless Assertion:** N/A (streaming/alloc ticket); the streaming path contains no per-line heap allocation (miri lane covers `Streaming`'s zero-alloc guarantee).
- **Allocation Invariant:** 0 heap allocations in hasher state; certify/snapshot paths no longer materialize the N-Quads String / CBOR Vec when only a digest is wanted.
- **Benchmark Target:** digest throughput ≥ 12 GB/s on AVX2-class hosts via the existing SIMD backend selection; egress receipts minted inline with emission (single pass).
- **Layer Boundary:** receipt seam lives in purrdf-hash (framing) + rdf-core (ByteDrain); `gts` consumes only. Zero cyclic or layer violations via `scripts/check-layers.py`.
- **Byte-identity guard:** recorded ids on existing archives stay stable — domain separation only via `Domain` / `frame_le_labelled`; never change preimages (`gts_compose.rs:1027-1037`: "a domain cannot be added without breaking every recorded id for an existing archive"). Receipt shape `blake3:<hex>` via `digest_label` (`wire.rs:79-84`).

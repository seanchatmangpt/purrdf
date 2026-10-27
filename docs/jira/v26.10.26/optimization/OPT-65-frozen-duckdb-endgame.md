# [OPT-65] Frozen-DuckDB Prebuilt Endgame — Linkage Wiring, 5s-Law Interaction, C18 Status

## 1. TARGET CONTEXT
- **Crate & File:** `crates/duckdb` (consumer surface `src/eval.rs`); root
  `Cargo.toml:126`; `Cargo.lock:856/879/894`; registry
  `frozen-duckdb-{,builder-,sys-}2.0.2`
  (`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`);
  upstream `~/frozen-duckdb/crates/frozen-duckdb-builder/src/lib.rs`;
  `~/.frozen-duckdb/cache/`.
- **Subsystem:** the embedded DuckDB execution lane (`purrdf-duckdb` over the
  prebuilt `frozen-duckdb` linkage) and the no-builds machine law that governs
  how its C++ linkage may ever be produced or verified locally.
- **Inspection Agent:** lane F1-frozen-duckdb-endgame, wave fan-out
  2026-10-27; static inspection + one bounded leaf probe (receipt in §2).

## 2. MECHANICAL PATHOLOGY
- **The linkage is already fully wired (static, ALIVE this session).**
  `Cargo.toml:126` pins `frozen-duckdb = { version = "2.0.2" }` (crates.io);
  `Cargo.lock:856-858/879-881/894-896` resolve `frozen-duckdb`,
  `frozen-duckdb-builder`, `frozen-duckdb-sys` all at 2.0.2 from the registry;
  `crates/duckdb/Cargo.toml:24` consumes it `workspace = true`. The consumer
  surface in `crates/duckdb/src/eval.rs` uses only the facade: `Connection`,
  `open_in_memory`, `execute_batch`, `prepare`/`query`, `ToSql`,
  `duckdb::types::ValueRef`, `DuckDBError` (eval.rs:49,139,145,147,218-236,266)
  — no sys-crate FFI appears in the leased crate.
- **The registry 2.0.2 sys crate drives the prebuilt cache, not a C++ build
  when warm.** `frozen-duckdb-sys-2.0.2/build.rs:47` calls
  `frozen_duckdb_builder::ensure_binary()`; `:55` emits
  `cargo:rustc-link-search=native={cache}`; `:58` emits
  `cargo:rustc-link-lib=dylib=duckdb`; `:64` emits
  `cargo:rustc-link-arg=-Wl,-rpath,{cache}` — the KD-15 absolute-install-ID
  law, so binaries need no DYLD vars at run time.
- **Version keying is decoupled from the crates.io version (gap).** The
  registry builder 2.0.2 pins `pub const VERSION: &str = "2.0.0-dev"`
  (builder `src/lib.rs:19`); the cache entry it probes is
  `$HOME/.frozen-duckdb/cache/v2.0.0-dev-{arch}/` (`:593`,
  `get_cache_dir` `:1571-1578`). "frozen-duckdb 2.0.2" therefore does NOT pin
  engine bytes — two machines on the same crate version can serve different
  engine binaries under the same label.
- **The cold path is the local-compile fallback, not a 66 MB download.**
  `frozen-duckdb-sys-2.0.2/build.rs:26` states the GitHub release download
  404s pre-tag, so `ensure_binary` falls to `compile_duckdb_locally`
  (builder `src/lib.rs:1636-1786`: full duckdb.git clone + cmake + make,
  hours). A post-tag 66 MB asset download breaches 5s too; the pre-tag
  compile branch is the harsher operative one today.
- **Session probe receipt (quoted, no retry):** `scripts/bounded cargo check
  -p purrdf-duckdb --locked` exited **124 at 5013 ms**, killed while compiling
  proc-macro2/quote/unicode-ident/libc/syn — the leaf's dependency closure is
  cold in the bounded target dir; the frozen-duckdb-sys link stage was never
  reached. The warm ~0.1s leaf class holds only over a warmed target dir
  (lane lease); a cold-closure leaf breaches 5s by construction.
- **Prebuilt dir is not the operative path.** `~/frozen-duckdb/prebuilt/`
  carries real headers (`duckdb.h` 290,195 B, `duckdb.hpp` 431 B) but its
  `libduckdb*.dylib` symlinks are DANGLING (target `libduckdb_arm64.dylib`
  absent, checked 2026-10-27). Irrelevant to purrdf: `check_prebuilt_binary`
  (builder `src/lib.rs:878-897`) reads `$CWD/prebuilt/` of the crate being
  built, not this manual-workflow directory.
- **Warm cache is present and structurally trusted.**
  `~/.frozen-duckdb/cache/v2.0.0-dev-arm64/`: `libduckdb_arm64.dylib`
  65,535,744 B; `ENGINE_VERSION` = `v2.0.0-dev` (byte-exact builder pin);
  `libduckdb_arm64.dylib.size` = 65535744 (matches actual length);
  `duckdb/` vendored headers; `libduckdb.dylib` -> absolute cache path.
  Admission law (`accept_cached_entry`, builder `src/lib.rs:759-822`):
  byte-exact marker, `.size` pin == real length, Mach-O slice magic +
  10 MiB floor, KD-15 install-ID stamp. Structural, not digest, identity.

## 3. PROPOSED SPEED-LIMIT REPLACEMENT
- **Classification of the linkage under the 5s law (no code change needed
  to stay lawful):** cold staging (download or local compile) is
  CI/async-only by definition — it can never fit the leaf tier; the only
  lawful local rung is the warm trusted-cache-hit class inside a lane lease
  (`scripts/lane-lease.sh exec <name> -- scripts/bounded cargo check -p
  purrdf-duckdb --locked`) after a warming run, or a CI receipt.
- **Warm the lease before probing, never inside the probe:** the cold dep
  closure (proc-macro2 -> syn -> ... -> frozen-duckdb-sys) is what breached
  5s here, not the DuckDB chain; a warm lease turns the probe into the
  documented ~0.1s leaf class.
- **C18 content-keying: NOT realized on this surface (status, not a
  defect to fix in this lane).** The cache key is `(builder VERSION pin,
  arch)`, integrity is marker + length pin + slice magic — no content
  digest anywhere in the key. Until C18 lands, standing vocabulary for any
  claim about engine-byte identity is "version-keyed,
  length-and-magic-verified", never "hermetic". C18's own falsifier (same
  input key => same digest) fails today by inspection: the same
  `v2.0.0-dev-arm64` label could serve different bytes on two hosts.
- **If engine-byte identity is ever required:** the C18-shaped fix is a
  content digest in the key (e.g. BLAKE3 of the dylib recorded in a
  lockfile-style sidecar and asserted by the admission law) — extend the
  existing admission law, never add a second admission body
  (helpers-ledger rule).
- **Version-pin hygiene note for the endgame ticket owner:** publishing a
  v2.0.0-dev GitHub release with the per-arch assets (TR7 asset law,
  builder `src/lib.rs:113-130`) converts the cold path from
  hours-long compile to a 66 MB download; both stay CI/async-only, but the
  download branch is the one the prebuilt README already advertises.

## 4. VERIFICATION INVARIANT & ACCEPTANCE CRITERIA
- **Linkage wiring is ALIVE as static proof only:** every edge above is
  cited file:line and was re-read this session. Link-time execution remains
  UNVERIFIED on this machine (5s law) until a warmed lane-lease probe or
  CI receipts it — never claim "it builds" from this receipt.
- **Bounded probe discipline:** the leaf probe runs ONCE; a 124 is data and
  is quoted with its tier diagnosis, never retried (OPT-64 grammar). A
  retry-with-no-new-hypothesis run of the cold probe is itself a violation.
- **Warm-path acceptance (when someone picks up the rung):**
  `scripts/lane-lease.sh exec <lane> -- scripts/bounded cargo check -p
  purrdf-duckdb --locked` exits 0 within the t3 budget (1.5s warm) over a
  lease warmed by one prior run; rc 124 on a cold closure is expected and
  routes to warming, not to a retry loop.
- **Cache-trust invariant holds regardless of C18:** any served dylib must
  pass `accept_cached_entry` (marker byte-exact, size pin, Mach-O slice,
  10 MiB floor); a poisoned entry aborts with `POISON_STATE_REFUSED` exit 1
  (builder `src/lib.rs:739-746`), never a silent restage.
- **Standing honesty:** `purrdf-duckdb` Rust gates stay UNVERIFIED locally
  per the no-builds law; the sanctioned local truth for this lane is the
  static wiring proof + the quoted 124 + the cache structural-trust
  inspection above.
- **Layer Boundary:** this lane touched only `crates/duckdb` (read) and
  this receipt file; no `lib/`-class edit, no generated/, vectors/,
  `.deficiencies` contact, no git mutation.

See Also: `docs/jira/v26.10.26/OPT-5s-bounded-ceiling.md` ·
`docs/jira/v26.10.26/optimization/OPT-64-feedback-loop-sla.md` ·
`~/frozen-duckdb/crates/frozen-duckdb-builder/src/lib.rs` ·
`docs/jira/v26.10.26/OPT-42-branchless-handoff.md`

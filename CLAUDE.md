# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code
in this repository.

**WE DO NOT CARE ABOUT BINARY SIZE OR WASM SIZE. IF YOU FIND YOURSELF OBSESSING
OVER BINARY SIZE - STOP.**

PurRDF is a Rust RDF 1.2 toolkit — parse, query, validate, reason, retrieve,
transport — one engine shared verbatim by the Rust, Python, JavaScript/WASM and
C surfaces. Consumers need stable Rust 1.98; development runs the pinned
floating nightly from `rust-toolchain.toml`.

Read [AGENTS.md](./AGENTS.md) — it is the canonical contract for this repository
(crate map, hard constraints, commands, performance discipline, brand rules).

Read [Rust engineering guidelines](./docs/agents/RUST_GUIDELINES.md), subject to
the repository-specific constraints in AGENTS.md.

**WASM test ownership:** ordinary semantic, refusal, ownership and conformance
tests run natively. WASM selections must exercise actual dispatch/SIMD,
shadow-stack or host-interface behavior, or an explicitly frozen native byte
identity. A 32-bit target or an existing selection is insufficient. Follow
[the exact WASM execution inventory](./docs/WASM_TESTING.md); do not run whole
mixed targets on WASM or extend them with ordinary native tests.

## Agents

Never dispatch the built-in generic agents (`general-purpose`, `claude`) — the
`ban_generic_agents` PreToolUse hook refuses them. Use read-only Explore/Plan;
specialist agents live in `.claude/agents/` (five defined). Subagent
prompts always carry the absolute repo path (`/Users/sac/purrdf`) and disjoint
lane file-ownership when fanning out. Enforced: `.claude/settings.json`.

## This machine: no builds, ever

Every `cargo build`/`test`/`clippy` over ~5s can hard-crash this machine.
`scripts/bounded` plus the PreToolUse hook
(`.claude/hooks/bounded_build_guard.py`) kill anything at 5s wall clock, no
exceptions — do not work around it. Whole-workspace runs — `cargo test`,
`cargo check`, `cargo clippy`, `cargo fmt`, `make test`, `make check` — are
**banned locally, CI-only** (`.github/workflows/bounded-gates.yml` runs the
same 5s-bounded stages; the Makefile guard flips via `PURRDF_BOUNDED_LOCAL=0`,
owned by CI).

The 5s figure is the tiered feedback-loop SLA, not a cold-build claim: the budgets
are the rung ceilings in `scripts/fb-budgets.json` (t1 static 0.5s, t2 oracle 0.8s,
t3 leaf probe 1.5s warm, ladder total 2.5s; entry point `make fb`, contract in
AGENTS.md §4). Cold builds are CI/async physics —
`.github/workflows/bounded-gates.yml` owns them.

**Bounded command surface** (the 5s limit is hard-coded inside — no seconds
argument):

```sh
scripts/bounded <cmd>                       # any command, killed at 5s
make b-<crate>                              # bounded check one crate
make bt-<crate> CARGO_TEST_FILTER=owned_term_walks PURRDF_LANE=lane-c20
bash scripts/lane-lease.sh exec <name> -- <cmd>   # CARGO_TARGET_DIR lease
bash scripts/lane-lease.sh list             # leases live in /tmp/purrdf-targets
bash scripts/target-hygiene.sh status --fast
bash scripts/target-hygiene.sh prune --dry-run
bash scripts/target-hygiene.sh ice <crate>  # per-crate ICE surgery
bash scripts/bounded-fmt.sh                 # cargo fmt, changed files only
bash scripts/test-bounded-guard.sh          # non-vacuity corpus for the guard
make fb                                     # the feedback-SLA ladder (AGENTS.md §4)
```

Hook gotchas (they will bite otherwise):

* The guard classifies heredoc bodies as shell segments — never inline text
  containing build verbs; write scripts to /tmp with the Write tool and run
  `python3 /tmp/<name>.py`
* Plain `bash <script>.sh` (no `-c`) is blocked as uninspectable — invoke via
  `scripts/bounded` or a `python3` launcher
* `lane-lease.sh` needs `flock` on PATH (macOS: `brew install util-linux`)

**Verification on this machine** is static, not the test ladder:

* Python gates: `scripts/check-*.py` (e.g. `check-no-std.py`,
  `check-layers.py --fast`)
* pytest oracles: `crates/branchless/oracles/` — budgets: 10ms per test,
  1s per session

Standing rules:

* `cargo check -p purrdf-branchless` — the only cheap compile probe (hash +
  branchless subtree only); `make b-<crate>` is the bounded form
* Exclude `purrdf-duckdb` / `frozen-duckdb` from any cargo invocation —
  heaviest subtree in the workspace
* `target/debug/incremental` is ~15 GB; on a rustc ICE,
  `bash scripts/target-hygiene.sh ice <crate>` deletes that crate's
  incremental dirs — never the whole tree
* Never run concurrent cargo on one tree — lane isolation via
  `bash scripts/lane-lease.sh exec <name> -- <cmd>` (`CARGO_TARGET_DIR`
  leases in `/tmp/purrdf-targets`)

## Commands

```sh
make check        # the full gate: fmt, clippy (-D warnings, pedantic+nursery),
                  # build, tests, hygiene. Needs node + pinned binaryen (130).
make test         # cargo test --workspace --locked
make fmt          # cargo fmt --all
make hooks        # install pre-commit/pre-merge hooks (staged-index hygiene subset)
make metadata     # regenerate + verify generated/ artifacts and license bundles
make conformance  # full W3C conformance matrix, one scoreboard (docs/CONFORMANCE.md)
make bench        # purrdf_testkit::bench suites (report-only, never a gate)
make wasm         # build all release crates for wasm32 (CI hard-fails; locally SKIPs
                  # without the target — `make doctor` explains what each SKIP means)
make wasm-test    # the named WASM dispatch/SIMD/shadow-stack selections in Node
make pytest       # build the PyO3 module + run the Python binding tests
make capi-check   # committed purrdf.h vs generated output + C smoke test
make miri         # SmallVec + BLAKE3 streaming under Miri (own lane)
```

### Test ladder

Cheapest rung that answers the question; escalate only on failure. After
fixing a failure, rerun the boundary rung first, then expand — never rerun an
unchanged failing command without a new hypothesis. (On this machine the
ladder stops at rung 1 — see "This machine: no builds, ever"; everything above
`cargo check -p purrdf-branchless` runs on CI or another box.)

1. `cargo check` — compile gate (fastest)
2. `cargo test -p <crate> --test <target> --locked` — one integration court
3. `cargo test -p <crate> --locked` — one crate's suite
4. `make test` — the workspace suite (needs node + binaryen)
5. `make check` — the full gate (fmt, clippy, tests, hygiene + `make wasm`)
6. `make conformance` — the W3C matrix; `make wasm-test` for the named WASM
   selections

Exit discipline: a loud SKIP with a typed reason is not a pass (`make doctor`
says what each SKIP means); a gate that cannot exit 1 is the failure. Locked
builds (`--locked`) everywhere: the lockfile is part of the contract.

Single tests:

```sh
cargo test -p purrdf-core --test model_traits --locked
cargo test -p purrdf-core --test model_traits --locked -- owned_term_walks
make test-shard SHARD=lib   # one CI shard: lib | doc | integration-1..5
```

Toolchain: `rust-toolchain.toml` pins a **floating nightly** for development and
CI gates — an analysis choice, not a licence. The source stays nightly-free (no
`#![feature]` anywhere); the consumer MSRV is stable **1.98**, enforced by a
separate `msrv` CI job; release lanes build on stable. Dependency versions live
only in the root `[workspace.dependencies]`; member crates use
`dep.workspace = true`.

## Architecture

PurRDF is one RDF 1.2 engine shared verbatim across languages: the language
surfaces — Python (PyO3, `bindings/python`), JavaScript/WebAssembly
(`crates/rdf-wasm` + `js/`), C (`crates/rdf-capi`, cargo-c) and the CLI
(`crates/cli`) — call the same Rust crates rather than reimplementing them.
Every published crate must stay `wasm32-unknown-unknown`-clean.

Layering is machine-checked (`layers.toml` via `scripts/check-layers.py` in
`make check`), bottom to top:

1. `purrdf-hash` — zero-dependency root every crate may use: BLAKE3, MD5,
   SHA-1, SHA-3, CRC-32; the fixed-key `FixedHasher` every map in an output or
   hot path uses (std `RandomState` is clippy-banned — use
   `purrdf_core::FastMap`/`FastSet`); hex/frame/fnv/mix kernels.
2. `purrdf-lex` — the one lexical layer: exact W3C terminal classes (scanners
   carry a character cursor and must call `purrdf_lex::terminals`, never retype
   a range table — the terminal ring-fence), Unicode normalization, and the
   JSON/YAML/CBOR/XML readers and writers that replace serde & co. (banned by
   `scripts/check-banned-deps.py`).
3. `purrdf-iri`, `purrdf-xsd`, `purrdf-events` — IRI value space, CURIEs, BCP 47
   and W3C vocab terms (`purrdf_iri::vocab`); XSD value spaces and exact
   numbers; the RDF 1.2 base direction.
4. `purrdf-core` — the interned IR kernel (`TermId` = niche-optimized
   `NonZeroU32`, string arena, store-once interner), datasets, diagnostics,
   provenance and segmented storage; ring-fenced from PyO3.
5. Above it: codecs (`purrdf-rdf`, incl. JSON-LD and canonicalization), SPARQL
   (`sparql-algebra` → `sparql-eval` → `sparql-results`), validation and
   reasoning (`shapes`/SHACL, `shex`, `entail`, `datalog`, `n3`), retrieval
   (`text`, `geo`/`geo-kernel`, `hnsw`, `retrieval`), document codecs and
   carriers (`markdown`, `json`, `toml`, `mime`, `columnar`, `gts`), all
   exposed through the `purrdf` facade crate.

**One home per job**: `helpers-ledger.toml` is authoritative — every job the
workspace implements once (hashing, JSON, escaping, goldens, benches, temp
paths...) has exactly one sanctioned home; call it, never write a second body
(`helper-census` + `scripts/check-shared-helpers.py` enforce this). To place
new shared code at the right altitude, run
`python3 scripts/check-layers.py --home-for A B …`.

Conformance corpora are the contract: the W3C suites under `vectors/` and the
first-party corpora under `crates/*/corpus/` and `tests/fixtures/` are frozen.
Never regenerate or "fix" the GTS vectors — they are shared byte-exact with the
authoritative GTS project, and the wire format is governed there. Harnesses
assert exact counts and enforce XPASS discipline on their xfail ledgers.

Releases: one version in `[workspace.package]`, lockstep across Cargo/PyPI/npm,
tag-driven trusted publishing (`rust-v*`/`py-v*`/`npm-v*`,
[docs/RELEASE.md](./docs/RELEASE.md)). `purrdf-alloc-probe`,
`purrdf-conformance-kit` and `purrdf-testkit` are dev-dependencies whose root
entries are path-only with **no `version`** — that is what lets `cargo publish`
verification resolve them.

## Generated artifacts (never hand-edit)

| Artifact | Regenerate |
|---|---|
| `generated/` projections + license bundles | `make metadata` |
| `crates/rdf-capi/include/purrdf.h` | `make capi-header` |
| book visualization samples (`docs/book/src/assets/visualization/`) | `make book-samples` |
| rule inventory / conformance scoreboard docs | `make metadata` (rule inventory) · `python3 scripts/conformance-matrix.py --write-doc` (matrix) |
| `CHANGELOG.md` | `make changelog` (pinned git-cliff; guard: `CHANGELOG_ALLOW_REGENERATE=1`) |
| `vectors/` (W3C corpora, GTS vectors) | never here — frozen; GTS wire format is governed upstream |

## Non-negotiables (details in AGENTS.md)

* **Gate before you claim done**: `make check` — on CI/another box, not here.
  On this machine the gate is the static Python checks plus Chicago oracles.
  Clippy runs the workspace pedantic/nursery lint table and must be warning-free.
* **Branchless-porter contract**: `docs/jira/v26.10.26/OPT-42-branchless-handoff.md`
  is the authoritative remaining-work list; don't re-derive scope from greps.
* **`branchless-*` helpers-ledger jobs are enforced**: new kernels extend
  `entry_points` on the existing job, never add a second body.
* **Chicago verification without cargo**: `crates/branchless/oracles/` (and
  `~/ggen-marketplace/packages/pm4pytest`) run Python oracles over shared JSON
  vectors — use them instead of `cargo test` on this machine.
* **Never**: add Cargo features (the sole exception is the empty
  `purrdf-capi:capi = []` marker cargo-c requires); hand-edit `generated/` or
  `vectors/`; pin a dependency version inside a member crate; introduce
  nondeterminism into serializers or the GTS writer; add PyO3 anywhere near
  `purrdf-core`; break the wasm32 build.
* **Naming**: the project is **PurRDF** in prose, `purrdf` in identifiers
  (`make check-brand-casing`).
* **PurRDF is NOT an ontology**: it mints no vocabulary IRIs. Vocabularies are
  caller-supplied configuration with no fabricated defaults (hard error or
  inactive feature when absent); test fixtures use `example.org`; W3C terms
  come from `purrdf_iri::vocab`, never string literals.
* **Perf changes need a bench**: extend the benches (`purrdf_testkit::bench`)
  rather than asserting a speedup.
* **A refusal is a claim too — prove it.** Over-refusal (rejecting input that is
  actually valid) is the mirror of the silent-drop bug, and it shows up exactly
  when you are tightening validation. It hides well: every test passes, and a
  refusal *looks* like correct strictness — nothing appears broken until a user
  writes the query that should work and doesn't. So for every refusal you add or
  tighten, execute two cases: the one you believe is invalid, and a neighbouring
  one that is valid. Confirm the second still succeeds. Cost and size estimators
  are the worst offenders, and they are not off by a little — one refused a query
  at an estimated 86,016 intermediate cells that actually produced 42.
* **`.deficiencies` is a last-resort emergency ledger, never an authorization
  surface.** Every entry below its marker is 100% unauthorized, 100% a bug, and
  means its originating issue or PR failed. It records critically
  undone work misrepresented to pass gates, misrepresented by an agent, or
  exposed when an agent is found fundamentally defective — literally a cry for
  help from a failing agent. The normal file contains only its notice and
  marker. Any entry blocks completion, PR creation, and merge of the work that
  produced it until the defect is verified and has a durable, visible
  remediation owner. Removing the entry does not resolve the bug or make the
  failed work successful.

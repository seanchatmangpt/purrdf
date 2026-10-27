<!--
SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
SPDX-License-Identifier: CC-BY-4.0
-->

# Full-tree verification lanes: async CI-class verification over the 5-second local law

Status: DESIGN (lane F3-fulltree-ci-design, wave 2026-10-27). Nothing in this
document is wired; the two lanes it names already exist and this document
specifies how the full-tree verification classes map onto them, plus the
caching and claim laws that make their results citable. The local host is
no-build (repo `CLAUDE.md`, "This machine: no builds, ever"), so every wall
class below is a planning number carried from the wave input, not a locally
measured receipt — the first real run of each lane is what converts it to a
measured class.

## The two verification classes

The 5s law (`AGENTS.md:450-473`, `scripts/bounded:25` `LIMIT=5`, hook
`.claude/hooks/bounded_build_guard.py`) makes the local host fast precisely by
refusing everything colder than a leaf probe. That refusal is only lawful if
something *else* executes the cold work. Two async lanes own it:

| Lane | Class | Home | Runs the full tree? |
|---|---|---|---|
| V1 — cold build+test | minutes | `ci.yaml` `test-shard` (7 shards) | yes, `--workspace --locked` |
| V2 — bounded mirror | 5s per stage | `bounded-gates.yml` | no; the law's CI twin |

### V1 — Full cold build+test (`cargo test --workspace --locked`)

`make test` (`Makefile:299`) is `cargo test --workspace --locked`. CI already
executes it as seven parallel shards (`ci.yaml:315-326`; shard list at
`ci.yaml:326`: `lib, doc, integration-1..5`), partitioned by
`scripts/check-test-shards.py` (wired at `ci.yaml:131-135`), with the proof
that the shards equal the old single command recorded at `ci.yaml:306-314`
(the one-job form compiled 25 minutes before its first test and was cancelled
at its timeout — the shard split is the measured fix, and the `test` job at
`ci.yaml:401-405` aggregates the matrix).

**DuckDB rides the frozen lever, never source.** `crates/duckdb` consumes the
crates.io `frozen-duckdb` workspace dependency (`Cargo.toml:126`, version
2.0.2, checksum-pinned at `Cargo.lock:856-859`). Source DuckDB is a 15-40 min
class build; frozen DuckDB is a one-time ~66 MB artifact download, then a
~0.11 s link-class step per lane afterwards (classes as carried from the wave
input; receipt on first V1 run). The exclusion of `purrdf-duckdb` /
`frozen-duckdb` from local cargo invocations stays absolute — V1 runs it in CI
because under the frozen lever it is a download+link, not a build. Unbanning
source DuckDB is not a lane anyone may create: "full cold 15-40 min" exists in
this design only as the class that is refused.

Lane contract:

```sh
# CI-only (ubuntu runner), never the local host:
make test-shard SHARD=<lib|doc|integration-1..5>   # Makefile:302, runs ci.yaml:393
```

Receipts: each shard step emits the shard name + result; the aggregation job
(`ci.yaml:401-405`) fails if any shard fails (`needs: test-shard`). This is
the only lane allowed to say "workspace suite is green" — and only while
naming the commit SHA it ran on.

### V2 — The 5s-bounded mirror (`bounded-gates.yml`)

`bounded-gates.yml:5-8` states the contract: a CI mirror of the local
5-second law, every stage under `timeout -k 1 5`, no rust cache and no build
beyond the leaf probe. Two jobs: `bounded` (`bounded-gates.yml:38-89`, stages
leaf-compile, no_std, python oracles, bounded-guard corpus) and `fb-ladder`
(`bounded-gates.yml:91-197`), whose `bounded()` helper prints
`RECEIPT tier=N stage=<name> status=PASS|TIMEOUT|FAIL ms=<ms> sla=<ms>`
(`bounded-gates.yml:123`) so a CI log reads as a receipt log. The local
counterparts: `scripts/bounded` (exit 124 on breach, `scripts/bounded:7,95-101`),
tier ceilings from `scripts/fb-budgets.json:3-6` — a downward-only ratchet
(`fb-budgets.json:2`: loosening a budget is a reviewed diff, never a knob).

V2 exists so the *law itself* is verified off-host: the toolchain-pin gate
(`bounded-gates.yml:126` runs `scripts/check-toolchain-pin.py`; the agreement
itself is stated at `rust-toolchain.toml:32-35` — every CI job must name the
channel `rust-toolchain.toml` names) and the guard corpus
(`bounded-gates.yml:196-197`, tier 3, 10s SLA for the 8.9s corpus) both run
where the 5s kill cannot crash the no-build host.

## Content-keyed caching: one digest per input key (C18)

The Rust subtree is 52 workspace members (root `Cargo.toml` `members`,
52 entries — `bindings/python` + 51 crates under `crates/`). Cold-compile cost
is the dominant term of V1; the cache law is the C18 pattern (composition
catalog: one digest per input key; unchanged key means *verification* — a
cache hit — not a rebuild).

**The cache key MUST include, and be derived mechanically by one script in one
home:**

1. **Resolved rustc identity, not the channel word.** `rust-toolchain.toml:41`
   pins `channel = "nightly"` — a *floating* pointer by design
   (`rust-toolchain.toml:11-15`: a date freezes the lint surface and stops
   findings). The key is therefore the resolved toolchain:
   `rustc -vV` commit-hash + host triple, plus the components/targets lines
   (`rust-toolchain.toml:42-43`: clippy, rustfmt; wasm32-unknown-unknown,
   aarch64-unknown-linux-gnu). Keying on the literal "nightly" would make a
   compiler bump invisible to the cache — exactly the drift C18 exists to
   kill.
2. **`Cargo.lock` content hash.** The lock is part of the contract (locked
   builds everywhere; `--locked` at `Makefile:299,302` and `ci.yaml:397`). Its
   hash covers dependency versions *and* the `frozen-duckdb 2.0.2` checksum
   (`Cargo.lock:859`) — the frozen artifact's identity rides the same key.
3. **Workspace `Cargo.toml` content hash.** Manifest-level feature/profile
   changes (e.g. `[profile.dev]` debug policy, which `ci.yaml:327-331` notes
   is manifest-owned and checked by `scripts/check-build-profiles.py`) move
   compile behavior without touching the lock.

Any compile cache for this workspace — sccache or the existing
`Swatinem/rust-cache` lane (`ci.yaml:370-374`, `shared-key: workspace-test`,
saved only by the `lib` shard at `ci.yaml:374`) — must key on that triple.
Swatinem deliberately never caches first-party crates (`ci.yaml:368-369`), so
every shard still compiles the code under test; a sccache layer keyed as above
is the complement for the *cold* lane's dependency graph, not a replacement
for shard correctness.

Corollary: `cache-hit` is a claim about the *key*, so a hit inherits the
frozen-artifact law below — a hit is evidence for the keyed inputs, never for
the edited tree.

## The frozen-artifact law (hard constraint)

**A frozen artifact proves its frozen inputs, never the edited tree.** The
`frozen-duckdb 2.0.2` checksum at `Cargo.lock:859` proves that artifact; a
green V1 shard proves the commit it checked out; a cache hit proves the key it
matched. None of them proves the bytes sitting edited in a working tree.

Therefore: **any local claim keyed to a frozen build must name the input key
it verified.** Minimum key: (resolved rustc identity, `Cargo.lock` hash,
workspace `Cargo.toml` hash, tree/commit digest of the frozen inputs). A claim
that cannot state its key is UNVERIFIED, whatever log it points at. This is
the same discipline the repo already applies to frozen corpora — vectors are
frozen and governed upstream (repo `CLAUDE.md`, generated-artifacts table), so
a conformance pass is a claim about the corpus + engine pair, citable only by
naming both.

## Wall classes (planning estimates, receipt on first run)

| Class | Estimate | Note |
|---|---|---|
| Local bounded ladder, total | ≤2.5s | `fb-budgets.json:6` (`total.budget_s`), ratchet-only |
| Leaf probe (t3) | ≤5s hard | `scripts/bounded:25`; exit 124 (`scripts/bounded:95-101`) |
| Warm single-crate check | tens of seconds | t3 ceiling is the local *law*; colder is async-only |
| Cold Rust-only subtree | 3-8 min | 52 members; `lib` shard is the long pole (`ci.yaml:333-336`) |
| Frozen-duckdb first acquisition | 7-10s | ~66 MB once, then ~0.11s link (`Cargo.lock:859`) |
| Full cold WITH source duckdb | 15-40 min | **banned class, not a lane** — frozen is the point |

Estimates are planning numbers from the wave input, marked UNVERIFIED until
the first V1/V2 run receipts them on a build-capable host; the local machine
can never measure them (next section).

## Local-machine boundary

None of these lanes may ever run on the no-build host. The host's lawful
verification surface is static only: the `scripts/check-*.py` gates and the
`crates/branchless/oracles` pytest suite (repo `CLAUDE.md`, "Verification on
this machine is static"; tier table `AGENTS.md:459-464`), plus the lane-lease
leaf probe (`scripts/lane-lease.sh:4-7` leases in `/tmp/purrdf-targets`,
`Makefile:1072-1077` `bt-%`/`b-%` wrap `scripts/bounded`). V1 and V2 are
defined in `.github/workflows/` only; a "quick full test here" is a contract
violation, not a slow command — the guard (`bounded_build_guard.py`) and the
5s kill are the mechanical backstop, and `OPT-5s-bounded-ceiling.md`
(`docs/jira/v26.10.26/`) is the enforcement ticket this design extends.

## See Also

- `docs/jira/v26.10.26/OPT-5s-bounded-ceiling.md` — the 5s enforcement ladder
- `docs/design/purrdf-bench-lane-laws.md` — what a benchmark lane's digest certifies
- `.github/workflows/bounded-gates.yml`, `.github/workflows/ci.yaml` — the two lanes
- `AGENTS.md` §4 — feedback-loop SLA and receipts

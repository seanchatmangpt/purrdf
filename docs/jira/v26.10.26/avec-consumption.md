# [OPT-?] avec Consumption — Publication Decision and Wiring Recipe

> STATUS (2026-10-29, revised same day): **SUPERSEDED by the fork posture.**
> purrdf is a diverged fork, never published; every member carries
> `publish = false`, and the crate registry lane is retired. The decision
> below (patch bridge) held only while publishing was on the table.

> CURRENT MODEL: purrdf-process consumes `avec-arrow` as a **git dependency
> pinned by rev** — `avec-arrow = { git = "https://github.com/seanchatmangpt/avec", rev = "965fcc5" }`
> in root `[workspace.dependencies]`. Unlike a `[patch]`, a git dependency
> travels with the workspace to any git-referencing consumer. Tickets T1-T4
> are landed at that rev (see `~/avec/docs/CONSUMER_REQUIREMENTS.md`), and
> purrdf-process's columnar OCEL projection
> (`crates/process/src/log/columnar.rs`) is the first consumer.
>
> The original patch-bridge decision and recipe follow, kept for the record.

## 1. Decision

purrdf consumes `avec-arrow` (and later `avec-parquet`) as an **external
dependency via the `[patch.crates-io]` path bridge** — the frozen-duckdb
precedent (root `Cargo.toml:384-387`): absolute sibling-checkout paths,
workspace-local, ignored by crates.io, and structurally invisible to the
crates.io record preflight and the dependency-ledger gate (a path-patched
package has no registry `source`; `scripts/check-banned-deps.py:766` counts
source-less packages as first-party). crates.io publishing of the two crates
is the destination, queued behind the operator's Trusted-Publisher bootstrap
(the same queue as `purrdf-process`/`purrdf-hooks`, currently in
`PURRDF_UNBOOTSTRAPPED_CRATES`).

Rationale: the patch bridge passes today's gates with zero operator work on
the critical path, and the version pin lines up (avec 26.10.29 == purrdf
26.10.29). The known cost — the build couples to `~/avec`'s canonical
checkout — is the same trade frozen-duckdb already makes, and the fleet
doctrine's one-canonical-checkout-per-repo law is what keeps that checkout
meaningful.

## 2. Wiring recipe (apply when T1 or T2 lands)

Root `Cargo.toml`:

```toml
[patch.crates-io]
avec-arrow = { path = "/Users/sac/avec/crates/avec-arrow" }
avec-parquet = { path = "/Users/sac/avec/crates/avec-parquet" }
```

Root `[workspace.dependencies]`:

```toml
avec-arrow = { version = "26.10.29" }
avec-parquet = { version = "26.10.29" }
```

Member crates declare `{ workspace = true }` with no features (the
`crates/duckdb` spelling for frozen-duckdb). Consumer candidates:
`purrdf-process` (batch layer for columnar OCEL logs) and the canon-arrow
egress arm (OPT-17 slab handoff via typed zero-copy views).

Ledger: a path-patched package needs no `dependency-ledger.toml` row; if
avec later publishes and the pin resolves from the registry, add the
`structured-zstd` five-key row
(`dependency-ledger.toml:591-595` is the form).

## 3. Consumer release contract

avec's version is ontology-driven: `ontology/avec.ttl` → `ggen sync run` →
`crates/avec-arrow/src/generated_version.rs`, pinned by
`scripts/release_check.py`. A consumer pins the calendar version and bumps
with avec's releases; purrdf's own date-version scheme keeps the two in
lockstep cheaply (26.10.29 == 26.10.29 today).

## 4. Requirements summary

The requirement tickets (T1 batch/table layer, T2 typed zero-copy ingest,
T3 timestamp `Scalar`, T4 wasm32-unknown-unknown receipt), their evidence,
and acceptance shapes live avec-side in `~/avec/docs/CONSUMER_REQUIREMENTS.md`
— the substrate stays the single home for those additions.

## 5. Standing

| element | standing |
|---|---|
| publication model | DECIDED (patch bridge; crates.io destination) |
| wiring recipe | DOCUMENTED (this doc, §2) |
| consumer code in purrdf | DEFERRED (trigger: T1 or T2 lands avec-side) |
| purrdf code/manifest changes this round | NONE |

Evidence pin: surveys against `~/avec@0995d4b` + `26fcb35` (kernel diff
`0995d4b..26fcb35` = `digest.rs` addition only) and purrdf `59c69a38`.

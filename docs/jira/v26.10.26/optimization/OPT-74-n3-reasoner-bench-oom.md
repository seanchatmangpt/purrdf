# OPT-74: n3 reasoner bench OOM — not ladder-bounded

**Status:** OPEN (filed 2026-10-27, requeue receipt)

## Evidence

Two independent SIGKILL (signal 9) kills at the same point:

1. phase-4b rc=101 283s — died after `n3_parse/1000` + `n3_parse/10000`
   rows, before the fixpoint group printed (ladder then 64/256).
2. requeue rc=101 172s — died at the same point with the DEFAULT ladder
   already shrunk to 64/128 (N3_FIXPOINT_LARGE unset). The 256 top rung
   is therefore NOT the cause.

Both runs measured the parse rows fine (~1 ms / ~10 ms, ~968/935 Kelem/s)
then the process was OOM-killed during/before the next group's fixture
construction (`reason_fixture` for chain/64 or the prose-contrast row).

## Hypotheses (owner to discriminate)

- `reason_fixture` at chain/64 materializes proof structures that retain
  the parse arena (fixture built at group entry, per n) — cumulative
  allocation across the earlier parse benches + fixtures exceeds 48 GB.
- A leak in `ReasonerResult`/closure retention (closure.len() computed
  then the whole result kept alive through `black_box`).
- The numeric_peer fixture at n=128 (2n*56-byte program text, deep
  recursion in the reasoner).

## Next action

Reproduce with a single-group run (`--bench reasoner` with parse groups
commented out vs fixpoint-only) on a build box; if fixpoint-only also
OOMs at 64, it is the fixture/reasoner retention, not cumulative.

## Bench disposition until fixed

n3's reasoner bench stays RED/report-only. The two parse rows from the
SIGKILL'd runs are citable only under a partial banner (~1.03 ms /
~968 Kelem/s at 1000; ~10.35 ms / ~935-968 Kelem/s at 10000).

> UPDATE (2026-10-27, requeue receipt): the E0277 experiment is DECIDED -
> the both-bounds helper COMPILES (bench profile finished clean, no
> E0277 in the requeue log). The failure is now purely runtime: the
> deep_nesting binary also died SIGKILL (rc=101 527s) at ~7.5-24 GB RSS.
> Both requeue reds are the same class: memory-bound bench rows OOM
> this 48 GB box. Retitle scope: purrdf-n3 reasoner + purrdf-sparql-eval
> deep_nesting benches need memory-bounded fixtures (shrink inputs,
> cap recursion depth, or mark the top rows env-gated capacity probes)
> before they can produce green rows here.

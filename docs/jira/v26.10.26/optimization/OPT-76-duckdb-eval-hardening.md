# OPT-76: purrdf-duckdb eval hardening (wave-8 per-crate audit)

**Status:** OPEN (filed 2026-10-27, wave-8 duckdb lane; all pre-existing)

Worst-first, file:line exact:

1. **SQL interpolation without escaping** — `crates/duckdb/src/eval.rs:163-174`
   (`register_parquet_view`): `table_name`/`parquet_path` are
   format!-interpolated into `execute_batch` with only quote-wrapping; a
   `"` in a table name or `'` in a path breaks out. Fix: reject
   non-conforming identifiers up front (typed `PathError`) or escape;
   never interpolate raw caller strings into SQL.
2. **Silent column drop** — `eval.rs:348`: the catch-all `_ => None`
   omits Decimal/Date/Blob/List/Struct/... columns from the projection
   with no error. Fix: typed "unsupported column type" refusal.
3. **Swallowed memory ceiling** — `eval.rs:147`: `let _ =
   execute_batch("PRAGMA max_memory...")` — the documented safety
   property is fire-and-forget. Fix: propagate the failure.
4. **"zero-copy" doc overclaim** — `lib.rs:4`/`eval.rs:4` vs the
   String-materializing projection (`eval.rs:265-349`). Reword.
5. **Malformed dateTime lexical form** — `eval.rs:340-347` emits
   `"<n> (Microsecond)"` as xsd:dateTime. Fix: proper XSD lexical form.
6. **Unportable workspace patch** — root `Cargo.toml:380-383`
   hardcodes `/Users/sac/frozen-duckdb/...`; a fresh clone cannot
   resolve the workspace at all. Fix: gate on path-exists with a clear
   error, or document the required sibling clone in a build script /
   README (fresh-box failure is currently an obscure early cargo error).
7. Minor: dead `_query_iri` param (`eval.rs:210`), `rows_emitted`
   counting all-seen rows (`eval.rs:37` doc drift).

## Verification invariant

1: a table name containing `"` is refused with a typed error, not
executed. 2: a Decimal column yields a typed refusal, not a silent
omission. 3: a failed PRAGMA fails the open.

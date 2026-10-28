# OPT-73: uninlined_format_args backlog (wave-5 L11 receipt)

**Status:** OPEN — READY (the drain has ended; the mid-drain deferral
no longer applies). All 84 sites verified still open, wave-6.

## Scope (84 sites, all mechanical positional -> inline-capture)

- n3: lexer.rs 2 (345, 459); parser.rs 10; printing.rs 4 remaining
  (322, 434, 610, 614); proof_check_n3.rs ~19; proof_check.rs 3 (166,
  168, 273); proof_writer.rs 4 (64, 567, 687, 706 — keep {{}} escapes);
  reasoner.rs 19; tests/n3_parse.rs ~21 (multiline field-access sites do
  NOT fire); tests/n3_proofs.rs 1.
- sql: ast.rs 2 (326, 328 PRECEDING/FOLLOWING); serializer.rs 2
  (271 LIMIT, 274 OFFSET).
- manual_let_else: n3 reasoner.rs:5998-6002 (parse_n3 match).
- needless_borrow: none.

Rule: `format!("... {} ...", ident)` -> `format!("... {ident} ...")`;
`{:?}` -> `{ident:?}`; `{:02x}` -> `{ident:02x}`. Field-access and
method-call args do not fire.

## Verification invariant

CI clippy green (`cargo clippy --workspace --all-targets --locked
-D warnings`); rustfmt --check on touched files on a build box.

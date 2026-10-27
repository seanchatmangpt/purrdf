# [OPT-51] N3 Lexer Digit Runs to SWAR Word-At-A-Time Scans

> **MERGED-SUPERSEDED by OPT-46.** This ticket was a duplicate
> inspection of the same `crates/n3/src/lexer.rs` `read_number` digit
> loops as OPT-46 in the same wave. All content — pathology, SWAR
> proposal, kernel-home citation (ledger row
> `branchless-swar-word-kernels`), valid-neighbor refusal-mirror
> court, and layer-edge note — is folded into
> `OPT-46-n3-lexer-digit-run-swar.md` with line refs corrected against
> the final tree (the `:261/:270/:277-280` refs here were stale; the
> loops actually live at `lexer.rs:429-431/:441-443/:452-455`, inside
> `read_number` at `:423-474`). Do not execute from this file.

#!/usr/bin/env python3
"""PreToolUse guard: refuse unbounded build/test commands (5s wall-clock ceiling).

Fails open on unreadable input (the JSON payload only); refuses with exit 2 +
{"decision": "block"} naming the exact bounded rewrite, mirroring
ban_generic_agents.py. Embedded shell commands (`sh -c`, `eval`, `env`, ...)
are classified as the embedded command would be; command-substitution bodies
are classified from the RAW text (quotes never protect an executed
substitution); anything unparseable inside the command string fails CLOSED.

Known residuals (documented, mechanically undetectable without a shell
parser): variable indirection (`C=cargo; $C build`), ANSI-C `$'...'` heads,
and interpreter payloads reading verbs from external files.

Decision receipts: every allow/block verdict appends one compact JSON line
{"ts","command","decision"} to bounded-guard-receipts.log (sibling of this
hook; env BOUNDED_GUARD_RECEIPT overrides). Logging is fail-open (an OSError
never flips a classification); classification is fail-closed.
"""
import json
import os
import re
import shlex
import sys
import time

_RECEIPT = os.environ.get(
    "BOUNDED_GUARD_RECEIPT",
    os.path.join(os.path.dirname(os.path.abspath(__file__)),
                 "bounded-guard-receipts.log"),
)


def log_receipt(command, decision):
    """Append one decision receipt. Fail-open: never flip a classification."""
    try:
        with open(_RECEIPT, "a", encoding="utf-8") as fh:
            fh.write(json.dumps({
                "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "command": command,
                "decision": decision,
            }, separators=(",", ":")) + "\n")
    except OSError:
        pass


BLOCK_VERBS = {"cargo", "rustc"}
CARGO_FAST = {"fmt", "metadata"}  # subcommands allowed bare (no compilation)
MAKE_FAST_TARGETS = {
    "help", "doctor", "hooks", "fmt",
    "check-issue-refs", "check-brand-casing", "check-spec-attribution",
}
BLOCK_MAKE = {"test", "check", "wasm", "pytest", "conformance"}
# Bounded forms: the repo wrapper, or an exact `-k 1 5` coreutils lease.
BOUNDED_HEADS = {"bounded", "./scripts/bounded", "scripts/bounded"}
BOUNDED_TIMEOUT = re.compile(r"^(g?timeout)\s+-k\s+1\s+5(\s|$)")
# Interpreters whose embedded string must be inspected (or refused).
SHELL_HEADS = {"sh", "bash", "zsh", "ksh", "dash"}
INTERP_PAYLOAD_FLAGS = ("-c", "-e", "-m")  # sh-family / perl / python
# Prefix words that do not change what command actually runs.
PASS_THROUGH_HEADS = {"env", "nohup", "xargs", "time", "nice", "stdbuf",
                      "command", "exec"}
MAX_DEPTH = 8
DELIM = re.compile(r";|&&|\|\||\||&|\n|`|\$\(|<\(")
BUILD_VERB = re.compile(r"\b(cargo|rustc)\b")


def basename(head):
    return head.rsplit("/", 1)[-1]


def mask_quotes(cmd):
    """Blank quoted spans with same-length spaces. POSIX: single quotes have
    no escapes; double quotes honor backslash escapes. Command substitution
    is handled separately from the RAW text (quotes do not protect it)."""
    masked = []
    i, n = 0, len(cmd)
    while i < n:
        c = cmd[i]
        if c in ("'", '"'):
            j = i + 1
            while j < n and cmd[j] != c:
                j += 2 if (c == '"' and cmd[j] == "\\" and j + 1 < n) else 1
            j = min(j, n)
            masked.append(" " * (j - i + 1))
            i = j + 1
        else:
            masked.append(c)
            i += 1
    return "".join(masked)


def segments(cmd):
    """Split on unquoted delimiters only (masked copy), slice the original."""
    masked = mask_quotes(cmd)
    out = []
    pos = 0
    for m in DELIM.finditer(masked):
        seg = cmd[pos:m.start()]
        if seg.strip():
            out.append(seg.strip())
        pos = m.end()
    tail = cmd[pos:]
    if tail.strip():
        out.append(tail.strip())
    return out


def subst_bodies(cmd):
    """Command-substitution bodies from the RAW text: `$(...)` (balanced) and
    `` `...` `` pairs. Quotes do not protect a substitution the shell runs."""
    bodies = []
    i, n = 0, len(cmd)
    while i < n:
        if cmd.startswith("$(", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if cmd[j] == "(":
                    depth += 1
                elif cmd[j] == ")":
                    depth -= 1
                j += 1
            bodies.append(cmd[i + 2:j - 1])
            i = j
        elif cmd[i] == "`":
            j = cmd.find("`", i + 1)
            if j < 0:
                break
            bodies.append(cmd[i + 1:j])
            i = j + 1
        else:
            i += 1
    return bodies


def strip_env_and_cd(words):
    i = 0
    while i < len(words) and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", words[i]):
        i += 1
    if i < len(words) and words[i] in ("cd", "pushd"):
        i += 2
    return words[i:]


def strip_flags(words, value_flags):
    """Drop leading flag tokens (and one value for value-taking flags)."""
    i = 0
    while i < len(words):
        w = words[i]
        if w in value_flags and i + 1 < len(words):
            i += 2
        elif w.startswith("-") and w != "-":
            i += 1
        else:
            break
    return words[i:]


PASS_THROUGH_VALUE_FLAGS = frozenset(
    {"-u", "-S", "-I", "-n", "-P", "-s", "-E", "-L", "-d", "-a"})


def verdict(cmd, depth=0):
    """Classify a full command line; worst verdict wins; empty verdicts (pure
    env-assignment/cd prefixes) fail CLOSED — a command that classifies to
    nothing is not proven safe."""
    if depth > MAX_DEPTH:
        return "block"
    for body in subst_bodies(cmd):
        if verdict(body, depth + 1) == "block":
            return "block"
    saw_any = False
    for seg in segments(cmd):
        v = classify(seg, depth + 1)
        if v == "block":
            return "block"
        if v is not None:
            saw_any = True
    # Every segment stripped to nothing (e.g. `X="$(...)"` where the body was
    # benign): fail closed unless the text is genuinely trivial.
    return "ok" if saw_any or not cmd.strip() else "block"


def classify(seg, depth=0):
    if depth > MAX_DEPTH:
        return "block"
    try:
        words = shlex.split(seg)
    except ValueError:
        return "block"  # fail closed on unparseable command text
    words = strip_env_and_cd(words)
    if not words:
        return None
    head = words[0]
    base = basename(head)
    if base == "bounded" or head in BOUNDED_HEADS:
        return "ok"
    if BOUNDED_TIMEOUT.match(seg):
        return "ok"
    if base in ("timeout", "gtimeout"):
        return "block"  # any lease other than exact `-k 1 5` is not bounded
    if base in SHELL_HEADS:
        rest = words[1:]
        ci = next((k for k, w in enumerate(rest)
                   if w == "-c" or re.fullmatch(r"-[a-zA-Z]*c", w)), None)
        if ci is None or ci + 1 >= len(rest):
            return "block"  # interactive/uninspectable shell
        embedded = " ".join(rest[ci + 1:])
        v = verdict(embedded, depth + 1)
        return "block" if v == "block" else "ok"
    if base == "eval":
        return verdict(" ".join(words[1:]), depth + 1)
    if base in PASS_THROUGH_HEADS:
        rest = strip_flags(words[1:], PASS_THROUGH_VALUE_FLAGS)
        if not rest:
            return "block"  # e.g. bare `env -i`: uninspectable payload
        return classify(" ".join(rest), depth + 1)
    if base in BLOCK_VERBS:
        if base == "cargo" and len(words) > 1 and words[1] in CARGO_FAST:
            if words[1] == "fmt" and "--check" not in words[2:]:
                return "block"  # bare cargo fmt rewrites files; refuse
            return "ok"
        return "block"
    if base in ("make", "gmake"):
        targets = [w for w in words[1:] if not w.startswith("-") and "=" not in w]
        if not targets or any(t in BLOCK_MAKE for t in targets):
            return "block"
        if all(t in MAKE_FAST_TARGETS for t in targets):
            return "ok"
        return "block"  # unknown target: fail closed
    if base.endswith(".sh"):
        return "block"  # opaque shell scripts must be bounded
    if base in ("python3", "python", "perl"):
        rest = words[1:]
        ci = next((k for k, w in enumerate(rest) if w in INTERP_PAYLOAD_FLAGS),
                  None)
        if ci is not None and ci + 1 < len(rest):
            v = verdict(" ".join(rest[ci + 1:]), depth + 1)
            return "block" if v == "block" else "ok"
        return "ok"  # script-file runs: uninspectable payload, documented residual
    # Unknown head: block when a build verb appears OUTSIDE quoted spans.
    if BUILD_VERB.search(mask_quotes(seg)):
        return "block"
    return "ok"


def main():
    try:
        payload = json.load(sys.stdin)
    except Exception:
        sys.exit(0)  # fail open ONLY on unreadable JSON payload
    if payload.get("tool_name") != "Bash":
        sys.exit(0)
    cmd = payload.get("tool_input", {}).get("command", "")
    v = verdict(cmd)
    if v == "block":
        log_receipt(cmd, "block")
        print(json.dumps({
            "decision": "block",
            "reason": "BOUNDED-BUILD-REFUSED: build/test commands must run under "
                      "scripts/bounded (5s wall-clock ceiling, no exceptions). "
                      "Re-run as: scripts/bounded " + cmd +
                      " — probe would breach the 5s feedback SLA: probe wrong, "
                      "not machine slow; revert to leaf probe",
        }))
        sys.exit(2)
    log_receipt(cmd, "allow")
    sys.exit(0)


if __name__ == "__main__":
    main()

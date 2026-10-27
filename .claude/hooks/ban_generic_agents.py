#!/usr/bin/env python3
# Why not Rust: this is a Claude Code session-hook script wired from
# .claude/settings.json — the harness executes it as a bare python3 file on
# every Agent dispatch, not as compiled workspace tooling.
"""PreToolUse guard: refuse the built-in generic agents (general-purpose, claude).

purrdf CLAUDE.md: read-only Explore/Plan only; specialist agents live in
.claude/agents/ when added. A generic-agent dispatch exits 2 (block) with a
typed refusal naming the allowed agent types.

Removed 2026-10-27 by operator decision: the same-session agent-dispatch
budget + lane-lease guard. The lease regex required a trailing slash, so a
bare repo-root lease ("You own EXACTLY /Users/sac/purrdf") never matched, and
the daily-rotating budget key had already been exhausted by an earlier wave —
the guard refused valid dispatches while looking like a lease error.
"""
import json
import sys

BANNED = {"general-purpose", "claude"}
ALLOWED = ["Explore", "Plan"]


def _refuse(reason: str) -> None:
    print(json.dumps({"decision": "block", "reason": reason}))
    sys.exit(2)


def main() -> None:
    try:
        payload = json.load(sys.stdin)
    except Exception:
        sys.exit(0)  # unreadable input: fail open, other guards own that case

    if payload.get("tool_name", "") != "Agent":
        sys.exit(0)

    subagent = payload.get("tool_input", {}).get("subagent_type", "")
    if subagent in BANNED and subagent not in ALLOWED:
        _refuse(
            f"GENERIC_AGENT_REFUSED: subagent_type '{subagent}' is banned in purrdf. "
            f"Allowed: {', '.join(ALLOWED)} (Explore/Plan + .claude/agents/). "
            "Re-dispatch with a project agent type."
        )
    sys.exit(0)


if __name__ == "__main__":
    main()

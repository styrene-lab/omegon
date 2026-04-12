#!/usr/bin/env python3
"""Extract token totals from Omegon session journal entries.

Usage:
  python3 scripts/extract_session_usage.py .omegon/agent-journal.md --last 1
  python3 scripts/extract_session_usage.py .omegon/agent-journal.md --contains "@ file selection"
  python3 scripts/extract_session_usage.py .omegon/agent-journal.md --header "2026-04-12 — main"

Important: journal entries may contain multiple sub-sessions. This extractor
chooses one top-level journal entry, then sums all parseable turn lines within it.
Use --header or --contains to target a specific run note when comparing scripted
interactive litmus sessions.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

ENTRY_SPLIT = re.compile(r"(?=^##\s+20)", re.MULTILINE)
TURN_RE = re.compile(
    r"turn\s+(?P<turn>\d+)\s+—\s+(?P<provider>[^/]+)\s*/\s*(?P<model>[^\s]+)\s+in:(?P<input>\d+)\s+out:(?P<output>\d+)\s+cache:(?P<cache>\d+)",
    re.IGNORECASE,
)
HEADER_RE = re.compile(r"^##\s+(?P<header>.+)$", re.MULTILINE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Extract session token totals from agent journal")
    parser.add_argument("journal", help="Path to .omegon/agent-journal.md")
    parser.add_argument("--last", type=int, default=1, help="Use the last N entries and report the newest one")
    parser.add_argument("--contains", help="Select the newest entry containing this substring")
    parser.add_argument("--header", help="Select the newest entry whose heading contains this substring")
    return parser.parse_args()


def split_entries(text: str) -> list[str]:
    return [entry.strip() for entry in ENTRY_SPLIT.split(text) if entry.strip().startswith("## ")]


def pick_entry(entries: list[str], contains: str | None, header: str | None, last: int) -> str:
    if header:
        matches = []
        for entry in entries:
            first_line = entry.splitlines()[0] if entry.splitlines() else ""
            if header in first_line:
                matches.append(entry)
        if not matches:
            raise SystemExit(f"No journal entry header contains substring: {header!r}")
        return matches[-1]
    if contains:
        matches = [entry for entry in entries if contains in entry]
        if not matches:
            raise SystemExit(f"No journal entry contains substring: {contains!r}")
        return matches[-1]
    if not entries:
        raise SystemExit("No journal entries found")
    idx = max(0, len(entries) - last)
    return entries[idx:][-1]


def summarize(entry: str) -> dict:
    header_match = HEADER_RE.search(entry)
    header = header_match.group("header") if header_match else "unknown"
    turns = []
    for match in TURN_RE.finditer(entry):
        turns.append(
            {
                "turn": int(match.group("turn")),
                "provider": match.group("provider").strip(),
                "model": match.group("model").strip(),
                "input": int(match.group("input")),
                "output": int(match.group("output")),
                "cache": int(match.group("cache")),
            }
        )
    if not turns:
        raise SystemExit("Selected entry contains no parseable turn usage lines")
    total_input = sum(t["input"] for t in turns)
    total_output = sum(t["output"] for t in turns)
    total_cache = sum(t["cache"] for t in turns)
    return {
        "header": header,
        "turn_count": len(turns),
        "provider": turns[-1]["provider"],
        "model": turns[-1]["model"],
        "input_tokens": total_input,
        "output_tokens": total_output,
        "cache_tokens": total_cache,
        "total_tokens": total_input + total_output + total_cache,
        "turns": turns,
    }


def main() -> int:
    args = parse_args()
    text = Path(args.journal).read_text()
    entries = split_entries(text)
    entry = pick_entry(entries, args.contains, args.header, args.last)
    print(json.dumps(summarize(entry), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

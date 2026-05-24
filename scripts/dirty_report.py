#!/usr/bin/env python3
"""Classify git working-tree dirt for safer scoped commits."""

from __future__ import annotations

import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass


@dataclass(frozen=True)
class Entry:
    status: str
    path: str
    category: str
    note: str


def run_git(args: list[str]) -> str:
    result = subprocess.run(
        ["git", *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout


def classify(path: str) -> tuple[str, str]:
    if path == "ai/lifecycle/state.json":
        return (
            "lifecycle-state",
            "tracked lifecycle/runtime state; commit separately only when intentional",
        )
    if path.startswith("ai/lifecycle/"):
        return "lifecycle", "lifecycle artifact; avoid mixing with source commits"
    if path.startswith("openspec/"):
        return "spec", "OpenSpec artifact; should usually pair with planned implementation"
    if path.startswith("docs/design/") or path.startswith("ai/design/"):
        return "design", "design artifact; avoid accidental runtime churn"
    if path == "CHANGELOG.md":
        return "release-memory", "required for behavior/operator-workflow changes"
    if path.endswith(".rs") and path.startswith("core/crates/"):
        return "rust-source", "Rust source; check for unrelated rustfmt drift"
    if path == "Cargo.lock" or path.endswith("Cargo.toml"):
        return "cargo", "Cargo manifest/lockfile"
    if path.startswith("scripts/") or path in {"Justfile", "justfile"}:
        return "tooling", "developer/release tooling"
    if path.startswith("skills/"):
        return "skills", "operator skill content"
    if path.startswith("site/"):
        return "site", "generated or authored site content"
    if path.startswith(".omegon/"):
        return "omegon-local", "repo-local agent state; normally should be ignored or separate"
    return "other", "inspect manually"


def parse_status() -> list[Entry]:
    raw = run_git(["status", "--porcelain=v1", "-z"])
    if not raw:
        return []
    parts = raw.split("\0")
    entries: list[Entry] = []
    i = 0
    while i < len(parts):
        item = parts[i]
        i += 1
        if not item:
            continue
        status = item[:2]
        path = item[3:]
        if status.startswith("R") or status.startswith("C"):
            # porcelain -z emits the destination then source for renames/copies.
            if i < len(parts) and parts[i]:
                source = parts[i]
                i += 1
                path = f"{source} -> {path}"
        category, note = classify(path.split(" -> ")[-1])
        entries.append(Entry(status=status, path=path, category=category, note=note))
    return entries


def staged_categories(entries: list[Entry]) -> set[str]:
    return {entry.category for entry in entries if entry.status[0] != " " and entry.status[0] != "?"}


def main() -> int:
    try:
        entries = parse_status()
    except subprocess.CalledProcessError as exc:
        sys.stderr.write(exc.stderr)
        return exc.returncode

    if not entries:
        print("Working tree clean.")
        return 0

    grouped: dict[str, list[Entry]] = defaultdict(list)
    for entry in entries:
        grouped[entry.category].append(entry)

    print("Dirty tree classification:\n")
    for category in sorted(grouped):
        print(f"{category}:")
        for entry in grouped[category]:
            print(f"  {entry.status} {entry.path}")
            print(f"     {entry.note}")
        print()

    cats = {entry.category for entry in entries}
    staged = staged_categories(entries)
    warnings: list[str] = []
    if "lifecycle-state" in cats and ({"rust-source", "tooling", "cargo"} & cats):
        warnings.append(
            "lifecycle state is dirty alongside source/tooling; commit or revert it separately."
        )
    if "lifecycle-state" in staged and ({"rust-source", "tooling", "cargo"} & staged):
        warnings.append(
            "staged lifecycle state is mixed with staged source/tooling; split the commit."
        )
    if "rust-source" in cats:
        warnings.append("for Rust source dirt, check whether unrelated files are rustfmt-only drift.")
    if "release-memory" not in cats and ({"rust-source", "tooling", "cargo"} & cats):
        warnings.append("behavior/tooling changes may require CHANGELOG.md under [Unreleased].")

    if warnings:
        print("Warnings:")
        for warning in warnings:
            print(f"  - {warning}")
        print()

    print("Suggested hygiene:")
    print("  - Start/end implementation slices with `just dirty-report`.")
    print("  - Use explicit-path commits; avoid `git add .` for mixed worktrees.")
    print("  - Keep lifecycle-state changes in separate commits unless the task is lifecycle metadata.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

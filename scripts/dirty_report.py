#!/usr/bin/env python3
"""Classify git working-tree dirt for safer scoped commits.

Omegon-operated worktrees have two dirty planes:

* source plane: code, docs, specs, release memory, configs, tooling
* agent-state plane: live `.omegon/` coordination and telemetry

Raw `git status` is still useful, but it is the wrong release/PR gate once
multiple agents append and consolidate audit logs. Use `--source-clean` when a
workflow needs to know whether source-plane files are dirty while live agent
state may legitimately be changing.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import defaultdict
from dataclasses import asdict, dataclass

SOURCE_CATEGORIES = {
    "cargo",
    "design",
    "lifecycle",
    "lifecycle-state",
    "other",
    "release-memory",
    "rust-source",
    "site",
    "skills",
    "spec",
    "tooling",
}
AGENT_STATE_CATEGORIES = {"agent-state"}


@dataclass(frozen=True)
class Entry:
    status: str
    path: str
    category: str
    plane: str
    note: str

    @property
    def staged(self) -> bool:
        return self.status[0] not in {" ", "?"}


def run_git(args: list[str]) -> str:
    result = subprocess.run(
        ["git", *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout


def is_agent_state_path(path: str) -> bool:
    """Return true for live Omegon coordination/telemetry paths.

    Keep this repo-local and explicit. Arbitrary JSONL elsewhere is source dirt.
    """
    if path == ".omegon/audit-log.jsonl":
        return True
    if path == ".omegon/ipc.sock":
        return True
    if path.startswith(".omegon/runtime/"):
        return True
    if path.startswith(".omegon/leases/"):
        return True
    if path.startswith(".omegon/workspace/"):
        return True
    if path.startswith(".omegon/tmp/"):
        return True
    if path.startswith(".omegon/") and (
        path.endswith(".db")
        or path.endswith(".db-wal")
        or path.endswith(".db-shm")
        or path.endswith(".sock")
    ):
        return True
    return False


def classify(path: str) -> tuple[str, str, str]:
    if is_agent_state_path(path):
        return (
            "agent-state",
            "agent-state",
            "live Omegon coordination/telemetry; may change while agents are active",
        )
    if path == "ai/lifecycle/state.json":
        return (
            "lifecycle-state",
            "source",
            "tracked lifecycle/runtime state; commit separately only when intentional",
        )
    if path.startswith("ai/lifecycle/"):
        return "lifecycle", "source", "lifecycle artifact; avoid mixing with source commits"
    if path.startswith("openspec/"):
        return "spec", "source", "OpenSpec artifact; should usually pair with planned implementation"
    if path.startswith("docs/design/") or path.startswith("ai/design/"):
        return "design", "source", "design artifact; avoid accidental runtime churn"
    if path == "CHANGELOG.md":
        return "release-memory", "source", "required for behavior/operator-workflow changes"
    if path.endswith(".rs") and path.startswith("core/crates/"):
        return "rust-source", "source", "Rust source; check for unrelated rustfmt drift"
    if path == "Cargo.lock" or path.endswith("Cargo.toml"):
        return "cargo", "source", "Cargo manifest/lockfile"
    if path.startswith("scripts/") or path in {"Justfile", "justfile"}:
        return "tooling", "source", "developer/release tooling"
    if path.startswith("skills/"):
        return "skills", "source", "operator skill content"
    if path.startswith("site/"):
        return "site", "source", "generated or authored site content"
    if path.startswith(".omegon/"):
        return (
            "omegon-local",
            "source",
            "repo-local Omegon artifact not classified as live agent state; inspect before committing",
        )
    return "other", "source", "inspect manually"


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
        category, plane, note = classify(path.split(" -> ")[-1])
        entries.append(Entry(status=status, path=path, category=category, plane=plane, note=note))
    return entries


def staged_categories(entries: list[Entry]) -> set[str]:
    return {entry.category for entry in entries if entry.status[0] != " " and entry.status[0] != "?"}


def source_entries(entries: list[Entry]) -> list[Entry]:
    return [entry for entry in entries if entry.plane == "source"]


def agent_state_entries(entries: list[Entry]) -> list[Entry]:
    return [entry for entry in entries if entry.plane == "agent-state"]


def build_report(entries: list[Entry]) -> dict[str, object]:
    sources = source_entries(entries)
    agent_state = agent_state_entries(entries)
    return {
        "clean": not entries,
        "source_clean": not sources,
        "agent_state_dirty": bool(agent_state),
        "staged_agent_state_count": sum(1 for entry in agent_state if entry.staged),
        "source_count": len(sources),
        "agent_state_count": len(agent_state),
        "entries": [asdict(entry) for entry in entries],
    }


def print_human_report(entries: list[Entry]) -> None:
    if not entries:
        print("Working tree clean.")
        return

    sources = source_entries(entries)
    agent_state = agent_state_entries(entries)
    if not sources and agent_state:
        print("Source tree clean; agent state dirty:\n")
    else:
        print("Dirty tree classification:\n")

    grouped: dict[str, list[Entry]] = defaultdict(list)
    for entry in entries:
        grouped[entry.category].append(entry)

    for category in sorted(grouped):
        print(f"{category}:")
        for entry in grouped[category]:
            print(f"  {entry.status} {entry.path}")
            print(f"     plane={entry.plane}; {entry.note}")
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
    staged_agent_state = [entry for entry in agent_state if entry.staged]
    if staged_agent_state:
        warnings.append(
            "agent-state is staged; unstage it before source commits: "
            + ", ".join(entry.path for entry in staged_agent_state)
        )
    if agent_state and sources:
        warnings.append("agent-state is dirty alongside source; use explicit-path commits.")

    if warnings:
        print("Warnings:")
        for warning in warnings:
            print(f"  - {warning}")
        print()

    print("Suggested hygiene:")
    print("  - Start/end implementation slices with `just dirty-report`.")
    print("  - Use `just source-clean` for PR/release gates while agents are active.")
    print("  - Use explicit-path commits; avoid `git add .` for mixed worktrees.")
    print("  - Keep lifecycle-state changes in separate commits unless the task is lifecycle metadata.")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="emit machine-readable classification")
    parser.add_argument(
        "--source-clean",
        action="store_true",
        help="exit non-zero only when source-plane files are dirty",
    )
    args = parser.parse_args(argv)

    try:
        entries = parse_status()
    except subprocess.CalledProcessError as exc:
        sys.stderr.write(exc.stderr)
        return exc.returncode

    report = build_report(entries)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    elif args.source_clean:
        if report["source_clean"] and report["staged_agent_state_count"] == 0:
            if report["agent_state_dirty"]:
                print("Source tree clean; agent state dirty.")
            else:
                print("Source tree clean.")
        else:
            print_human_report(entries)
    else:
        print_human_report(entries)

    if args.source_clean and (not report["source_clean"] or report["staged_agent_state_count"]):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

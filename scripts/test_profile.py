#!/usr/bin/env python3
"""Summarize Rust test/coupling hotspots for local validation planning.

This is a static, fast report: it does not run tests. It counts test
annotations and source lines per crate/file so agents can decide where scoped
validation and future crate extraction will buy the most velocity.
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import asdict, dataclass
from pathlib import Path

TEST_RE = re.compile(r"(?m)^\s*#\[(?:tokio::)?test\]")


@dataclass(frozen=True)
class FileProfile:
    path: str
    lines: int
    tests: int


@dataclass(frozen=True)
class CrateProfile:
    name: str
    lines: int
    tests: int
    files: int
    test_files: int
    large_files: tuple[FileProfile, ...]


def profile_file(path: Path) -> FileProfile:
    text = path.read_text(errors="ignore")
    return FileProfile(
        path=str(path),
        lines=text.count("\n") + 1,
        tests=len(TEST_RE.findall(text)),
    )


def profile_crate(crate_root: Path, large_threshold: int, per_crate_files: int) -> CrateProfile:
    src = crate_root / "src"
    profiles = [profile_file(path) for path in sorted(src.rglob("*.rs"))]
    large = tuple(
        sorted(
            (profile for profile in profiles if profile.lines >= large_threshold or profile.tests > 0),
            key=lambda profile: (profile.lines, profile.tests),
            reverse=True,
        )[:per_crate_files]
    )
    return CrateProfile(
        name=crate_root.name,
        lines=sum(profile.lines for profile in profiles),
        tests=sum(profile.tests for profile in profiles),
        files=len(profiles),
        test_files=sum(1 for profile in profiles if profile.tests),
        large_files=large,
    )


def profiles(root: Path, large_threshold: int, per_crate_files: int) -> list[CrateProfile]:
    crates_root = root / "core" / "crates"
    return [
        profile_crate(crate_root, large_threshold, per_crate_files)
        for crate_root in sorted(crates_root.iterdir())
        if (crate_root / "Cargo.toml").exists() and (crate_root / "src").exists()
    ]


def print_human(items: list[CrateProfile], top_files: int) -> None:
    total_tests = sum(item.tests for item in items)
    total_lines = sum(item.lines for item in items)
    print(f"Rust workspace profile: {total_lines} source lines, {total_tests} test annotations\n")
    print("Crates:")
    for item in sorted(items, key=lambda crate: (crate.tests, crate.lines), reverse=True):
        pct = (item.tests / total_tests * 100.0) if total_tests else 0.0
        print(
            f"  {item.name:16s} {item.tests:5d} tests ({pct:5.1f}%) "
            f"{item.lines:6d} lines {item.files:4d} files {item.test_files:4d} test-files"
        )
    print("\nLargest/test-heavy files:")
    files = sorted(
        (profile for item in items for profile in item.large_files),
        key=lambda profile: (profile.lines, profile.tests),
        reverse=True,
    )[:top_files]
    for profile in files:
        print(f"  {profile.lines:6d} lines {profile.tests:4d} tests {profile.path}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument("--large-threshold", type=int, default=1000)
    parser.add_argument("--top-files", type=int, default=30)
    parser.add_argument(
        "--per-crate-files",
        type=int,
        default=30,
        help="maximum file profiles retained per crate before global top-file ranking",
    )
    args = parser.parse_args()

    root = Path.cwd()
    items = profiles(root, args.large_threshold, args.per_crate_files)
    if args.json:
        print(json.dumps([asdict(item) for item in items], indent=2))
    else:
        print_human(items, args.top_files)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

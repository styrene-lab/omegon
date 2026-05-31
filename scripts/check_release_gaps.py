#!/usr/bin/env python3
"""Check for upstream stable tags that do not have GitHub Releases.

This catches the partial-release state where a `vX.Y.Z` tag was pushed and the
release workflow failed before `gh release create` completed. By default the
script inspects all upstream stable tags matching `v*.*.*` and ignores
pre-release/nightly tags.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys

STABLE_TAG_RE = re.compile(r"^v\d+\.\d+\.\d+$")


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, text=True, capture_output=True, check=False)


def upstream_stable_tags(remote: str, pattern: str) -> list[str]:
    result = run(["git", "ls-remote", "--tags", remote, pattern])
    if result.returncode != 0:
        print(result.stderr.strip() or result.stdout.strip(), file=sys.stderr)
        raise SystemExit(result.returncode)

    tags: set[str] = set()
    for line in result.stdout.splitlines():
        if not line.strip():
            continue
        ref = line.split("\t", 1)[1]
        tag = ref.removeprefix("refs/tags/").removesuffix("^{}")
        if STABLE_TAG_RE.fullmatch(tag):
            tags.add(tag)
    return sorted(tags, key=lambda tag: tuple(int(part) for part in tag[1:].split(".")))


def release_exists(repo: str, tag: str) -> bool:
    return run(["gh", "release", "view", tag, "--repo", repo]).returncode == 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--remote", default="origin", help="git remote to inspect (default: origin)")
    parser.add_argument("--repo", required=True, help="GitHub repo, e.g. styrene-lab/omegon")
    parser.add_argument(
        "--pattern",
        default="refs/tags/v*.*.*",
        help="git ls-remote tag pattern (default: refs/tags/v*.*.*)",
    )
    parser.add_argument(
        "--since",
        help="only check tags >= this stable tag, e.g. v0.25.0",
    )
    args = parser.parse_args()

    tags = upstream_stable_tags(args.remote, args.pattern)
    if args.since:
        if not STABLE_TAG_RE.fullmatch(args.since):
            print(f"--since must be a stable tag like v0.25.0, got {args.since!r}", file=sys.stderr)
            return 2
        floor = tuple(int(part) for part in args.since[1:].split("."))
        tags = [tag for tag in tags if tuple(int(part) for part in tag[1:].split(".")) >= floor]

    missing = [tag for tag in tags if not release_exists(args.repo, tag)]
    if missing:
        print("Stable tag(s) missing GitHub Releases:")
        for tag in missing:
            print(f"  - {tag}")
        return 1

    print(f"All {len(tags)} stable tag(s) have GitHub Releases.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

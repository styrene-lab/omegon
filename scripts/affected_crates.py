#!/usr/bin/env python3
"""Report Rust workspace crates affected by changed paths.

The script maps git-changed paths to Cargo packages and applies a small
reverse-dependency policy so local validation can be scoped without ignoring
crate coupling. It is intentionally conservative: root manifests, lockfiles,
shared tooling, or unknown Rust paths select the whole workspace.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import defaultdict, deque
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Package:
    name: str
    root: Path
    deps: tuple[str, ...]


@dataclass(frozen=True)
class Affected:
    packages: tuple[str, ...]
    changed_paths: tuple[str, ...]
    docs_only: bool
    workspace: bool
    reason: str


def run(args: list[str]) -> str:
    result = subprocess.run(args, check=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return result.stdout


def repo_root() -> Path:
    return Path(run(["git", "rev-parse", "--show-toplevel"]).strip())


def tracking_branch() -> str | None:
    try:
        value = run(["git", "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]).strip()
    except subprocess.CalledProcessError:
        return None
    return value or None


def changed_paths(base: str | None) -> list[str]:
    paths: set[str] = set()
    if base:
        merge_base = run(["git", "merge-base", "HEAD", base]).strip()
        diff_base = f"{merge_base}...HEAD"
        paths.update(run(["git", "diff", "--name-only", diff_base]).splitlines())
    paths.update(run(["git", "diff", "--name-only"]).splitlines())
    paths.update(run(["git", "diff", "--name-only", "--cached"]).splitlines())
    status = run(["git", "status", "--porcelain=v1", "-z"])
    parts = status.split("\0")
    i = 0
    while i < len(parts):
        item = parts[i]
        i += 1
        if not item:
            continue
        code = item[:2]
        path = item[3:]
        if code.startswith(("R", "C")) and i < len(parts) and parts[i]:
            paths.add(parts[i])
            i += 1
        paths.add(path)
    return sorted(p for p in paths if p)


def workspace_packages(root: Path) -> dict[str, Package]:
    metadata = json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--quiet"]))
    packages: dict[str, Package] = {}
    for item in metadata["packages"]:
        manifest = Path(item["manifest_path"])
        package_root = manifest.parent.relative_to(root)
        deps = tuple(dep["name"] for dep in item.get("dependencies", []) if dep.get("path"))
        packages[item["name"]] = Package(item["name"], package_root, deps)
    return packages


def reverse_dependencies(packages: dict[str, Package]) -> dict[str, set[str]]:
    reverse: dict[str, set[str]] = defaultdict(set)
    for package in packages.values():
        for dep in package.deps:
            reverse[dep].add(package.name)
    return reverse


def closure(names: set[str], reverse: dict[str, set[str]]) -> set[str]:
    seen = set(names)
    queue: deque[str] = deque(names)
    while queue:
        name = queue.popleft()
        for dependent in reverse.get(name, set()):
            if dependent not in seen:
                seen.add(dependent)
                queue.append(dependent)
    return seen


def is_docs_only(paths: list[str]) -> bool:
    """Return true only for low-risk documentation/content paths.

    Be deliberately conservative. Directories such as `site/`, `ai/`, and
    `skills/` can contain executable/configuration artifacts as well as prose;
    only known content extensions should skip Rust validation.
    """
    if not paths:
        return False
    doc_prefixes = ("docs/", "openspec/")
    content_prefixes = ("ai/design/", "ai/memory/", "skills/")
    content_suffixes = (".md", ".txt", ".png", ".jpg", ".jpeg", ".gif", ".svg")
    for path in paths:
        if path == "CHANGELOG.md":
            continue
        if path.startswith(doc_prefixes) and path.endswith(content_suffixes):
            continue
        if path.startswith(content_prefixes) and path.endswith(content_suffixes):
            continue
        return False
    return True


def affected(base: str | None) -> Affected:
    root = repo_root()
    paths = changed_paths(base)
    packages = workspace_packages(root)
    if not paths:
        return Affected((), (), False, False, "no changed paths")
    if is_docs_only(paths):
        return Affected((), tuple(paths), True, False, "docs/content only")

    whole_workspace_markers = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rustfmt.toml"}
    if any(path in whole_workspace_markers or path.startswith(".github/") for path in paths):
        return Affected(tuple(sorted(packages)), tuple(paths), False, True, "workspace-level config changed")

    direct: set[str] = set()
    unknown_rust = False
    sorted_packages = sorted(packages.values(), key=lambda package: len(package.root.parts), reverse=True)
    for path_text in paths:
        path = Path(path_text)
        matched = False
        for package in sorted_packages:
            try:
                path.relative_to(package.root)
            except ValueError:
                continue
            direct.add(package.name)
            matched = True
            break
        if path.suffix == ".rs" and not matched:
            unknown_rust = True

    if unknown_rust:
        return Affected(tuple(sorted(packages)), tuple(paths), False, True, "unknown Rust path changed")
    if not direct:
        return Affected((), tuple(paths), False, False, "no Rust crate changes")

    selected = closure(direct, reverse_dependencies(packages))
    reason = "direct crates plus reverse dependents" if selected != direct else "direct crate changes"
    return Affected(tuple(sorted(selected)), tuple(paths), False, False, reason)


def print_shell(affected_result: Affected) -> None:
    print(" ".join(affected_result.packages))


def print_json(affected_result: Affected) -> None:
    print(json.dumps({
        "packages": list(affected_result.packages),
        "changed_paths": list(affected_result.changed_paths),
        "docs_only": affected_result.docs_only,
        "workspace": affected_result.workspace,
        "reason": affected_result.reason,
    }, indent=2))


def print_human(affected_result: Affected) -> None:
    print(f"Reason: {affected_result.reason}")
    if affected_result.changed_paths:
        print("Changed paths:")
        for path in affected_result.changed_paths:
            print(f"  - {path}")
    if affected_result.docs_only:
        print("Docs/content-only change: Rust validation can be skipped for quick local feedback.")
    elif affected_result.packages:
        scope = "workspace" if affected_result.workspace else "scoped"
        print(f"Affected crates ({scope}):")
        for package in affected_result.packages:
            print(f"  - {package}")
    else:
        print("No Rust crates selected.")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", help="Git ref to compare against; defaults to upstream tracking branch")
    parser.add_argument("--format", choices=("human", "json", "shell"), default="human")
    args = parser.parse_args()

    base = args.base if args.base is not None else tracking_branch()
    try:
        result = affected(base)
    except subprocess.CalledProcessError as exc:
        sys.stderr.write(exc.stderr)
        return exc.returncode

    if args.format == "json":
        print_json(result)
    elif args.format == "shell":
        print_shell(result)
    else:
        print_human(result)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

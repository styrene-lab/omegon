#!/usr/bin/env python3
"""Release preflight checks for Omegon.

This script validates that the repository is in a coherent state before a
stable release is cut from an RC line.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

VERSION_RE = re.compile(r'^version = "([^"]+)"', re.MULTILINE)
CHANGELOG_SECTION_RE = r"^## \[{version}\](?=\s|$)"
INSTALL_PLACEHOLDER_PATTERNS = (
    re.compile(r"Replace [0-9.]+ with the release you actually want"),
    re.compile(r"Replace [0-9.]+ with the release you downloaded"),
)


class PreflightError(Exception):
    pass


def read_workspace_version(repo_root: Path) -> str:
    cargo_toml = (repo_root / "core" / "Cargo.toml").read_text()
    match = VERSION_RE.search(cargo_toml)
    if not match:
        raise PreflightError("Could not read workspace version from core/Cargo.toml")
    return match.group(1)


def stable_version_from_rc(version: str) -> str:
    if "-rc." not in version:
        raise PreflightError(f"Workspace version {version} is not an RC version")
    return version.split("-rc.", 1)[0]


def changelog_has_version(repo_root: Path, version: str) -> bool:
    changelog = (repo_root / "CHANGELOG.md").read_text()
    return re.search(CHANGELOG_SECTION_RE.format(version=re.escape(version)), changelog, flags=re.MULTILINE) is not None


def install_docs_use_placeholders(repo_root: Path) -> bool:
    install_doc = (repo_root / "site" / "src" / "pages" / "docs" / "install.astro").read_text()
    return all(pattern.search(install_doc) for pattern in INSTALL_PLACEHOLDER_PATTERNS)


def workflows_use_release_manifest(repo_root: Path) -> bool:
    release_workflow = (repo_root / ".github" / "workflows" / "release.yml").read_text()
    homebrew_workflow = (repo_root / ".github" / "workflows" / "homebrew.yml").read_text()
    return "release-manifest.json" in release_workflow and "release-manifest.json" in homebrew_workflow


def git_stdout(repo_root: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def read_workspace_role(repo_root: Path) -> str | None:
    lease_path = repo_root / ".omegon" / "runtime" / "workspace.json"
    if not lease_path.exists():
        return None
    try:
        payload = json.loads(lease_path.read_text())
    except json.JSONDecodeError as err:
        raise PreflightError(f"Could not parse workspace lease {lease_path}: {err}") from err
    role = payload.get("role")
    return role if isinstance(role, str) and role else None


def ensure_release_workspace_role(repo_root: Path) -> None:
    lease_path = repo_root / ".omegon" / "runtime" / "workspace.json"
    payload: dict[str, object]
    if lease_path.exists():
        try:
            loaded = json.loads(lease_path.read_text())
            payload = loaded if isinstance(loaded, dict) else {}
        except json.JSONDecodeError as err:
            raise PreflightError(f"Could not parse workspace lease {lease_path}: {err}") from err
    else:
        payload = {}
        lease_path.parent.mkdir(parents=True, exist_ok=True)

    payload["role"] = "release"
    payload.setdefault("workspace_kind", "release")
    payload.setdefault("label", "release")
    lease_path.write_text(json.dumps(payload, indent=2) + "\n")


def collect_failures(repo_root: Path) -> list[str]:
    failures: list[str] = []

    branch = git_stdout(repo_root, "branch", "--show-current")
    if branch != "main":
        failures.append(f"must be on main (currently: {branch or 'detached'})")

    role = read_workspace_role(repo_root)
    if role != "release":
        failures.append(
            f"workspace role must be 'release' for RC/release cuts (currently: {role or 'unset'})"
        )

    dirty = git_stdout(repo_root, "status", "--porcelain")
    if dirty:
        failures.append("working tree is not clean")

    try:
        current_version = read_workspace_version(repo_root)
        stable_version = stable_version_from_rc(current_version)
    except PreflightError as err:
        failures.append(str(err))
        return failures

    if not changelog_has_version(repo_root, stable_version):
        failures.append(f"CHANGELOG.md is missing section [{stable_version}]")

    if not install_docs_use_placeholders(repo_root):
        failures.append("site/src/pages/docs/install.astro versioned examples are not marked as placeholders")

    if not workflows_use_release_manifest(repo_root):
        failures.append("release workflows are not consistently wired through release-manifest.json")

    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument(
        "--ensure-release-workspace-role",
        action="store_true",
        help="repair/create .omegon/runtime/workspace.json with role=release before exiting",
    )
    args = parser.parse_args(argv)

    repo_root = args.repo_root.resolve()
    if args.ensure_release_workspace_role:
        ensure_release_workspace_role(repo_root)
        print(repo_root / ".omegon" / "runtime" / "workspace.json")
        return 0

    failures = collect_failures(repo_root)
    if failures:
        print("✗ Release preflight failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    stable = stable_version_from_rc(read_workspace_version(repo_root))
    print(f"✓ Release preflight passed — repo is releasable as {stable}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

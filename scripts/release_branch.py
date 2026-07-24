#!/usr/bin/env python3
"""Branch-based release helpers for Omegon.

The public channel model is intentionally small:

- stable resolves to the latest stable semver tag.
- nightly resolves to main.
- explicit vX.Y.Z tags remain available for pins.

Release branches are internal stabilization/patch branches only. The authority
split is:

- main is trunk, owns nightly tags, and receives normal development.
- release/X.Y owns stable tags for that X.Y line while it is active.

These helpers keep branch creation and forward merges mechanical so release
hardening fixes flow back to trunk without pulling release-branch version state
backward into main.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path


VERSION_RE = re.compile(r'^version = "([^"]+)"', re.MULTILINE)
RELEASE_BRANCH_RE = re.compile(r"^release/(?P<major>\d+)\.(?P<minor>\d+)$")
VERSION_STATE_PATHS = ("Cargo.toml", "Cargo.lock", ".omegon/milestones.json")


class ReleaseBranchError(Exception):
    pass


def run(repo_root: Path, *args: str, capture: bool = False) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        check=True,
        capture_output=capture,
        text=True,
    )
    return completed.stdout.strip() if capture else ""


def current_branch(repo_root: Path) -> str:
    return run(repo_root, "branch", "--show-current", capture=True)


def ensure_clean(repo_root: Path) -> None:
    dirty = run(repo_root, "status", "--porcelain", capture=True)
    if dirty:
        raise ReleaseBranchError(f"working tree is not clean:\n{dirty}")


def read_workspace_version(repo_root: Path) -> str:
    cargo_toml = (repo_root / "Cargo.toml").read_text()
    match = VERSION_RE.search(cargo_toml)
    if not match:
        raise ReleaseBranchError("could not read workspace version from Cargo.toml")
    return match.group(1)


def stable_version(version: str) -> str:
    if "-" in version:
        raise ReleaseBranchError(
            f"workspace version {version} is not a stable release version"
        )
    return version


def version_sort_key(version: str) -> tuple[int, int, int, int]:
    """Sortable key for workspace versions; stable ranks above its prereleases."""
    core, _, prerelease = version.partition("-")
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", core)
    if not match:
        raise ReleaseBranchError(f"could not parse workspace version {version!r}")
    major, minor, patch = (int(part) for part in match.groups())
    return (major, minor, patch, 0 if prerelease else 1)


def release_branch_for_version(version: str) -> str:
    stable = stable_version(version)
    parts = stable.split(".")
    if len(parts) < 2:
        raise ReleaseBranchError(f"could not derive release branch from version {version}")
    return f"release/{parts[0]}.{parts[1]}"


def validate_release_branch_name(branch: str) -> None:
    if not RELEASE_BRANCH_RE.fullmatch(branch):
        raise ReleaseBranchError(f"{branch} is not a release/X.Y branch")


def local_branch_exists(repo_root: Path, branch: str) -> bool:
    return (
        subprocess.run(
            ["git", "show-ref", "--verify", "--quiet", f"refs/heads/{branch}"],
            cwd=repo_root,
        ).returncode
        == 0
    )


def remote_branch_exists(repo_root: Path, branch: str) -> bool:
    return (
        subprocess.run(
            ["git", "ls-remote", "--exit-code", "--heads", "origin", branch],
            cwd=repo_root,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        ).returncode
        == 0
    )


def create_branch(repo_root: Path) -> None:
    ensure_clean(repo_root)
    branch = current_branch(repo_root)
    if not branch:
        raise ReleaseBranchError("detached HEAD; check out main or release/X.Y first")

    version = read_workspace_version(repo_root)
    target = release_branch_for_version(version)

    if branch not in ("main", target):
        raise ReleaseBranchError(
            f"branch-release must run from main or {target}; current branch is {branch}"
        )

    if local_branch_exists(repo_root, target):
        run(repo_root, "switch", target)
    else:
        run(repo_root, "switch", "-c", target)

    if remote_branch_exists(repo_root, target):
        run(repo_root, "push", "-u", "origin", target)
    else:
        run(repo_root, "push", "-u", "origin", target)

    print(f"release branch ready: {target}")
    print(f"workspace version: {version}")


def assert_main_version_not_behind(repo_root: Path, release_version: str) -> None:
    """Refuse publication while origin/main builds an older workspace version."""
    main_cargo = subprocess.check_output(
        ["git", "show", "origin/main:Cargo.toml"], cwd=repo_root, text=True
    )
    match = VERSION_RE.search(main_cargo)
    if not match:
        raise ReleaseBranchError("could not read workspace version from origin/main Cargo.toml")
    main_version = match.group(1)
    if version_sort_key(main_version) < version_sort_key(release_version):
        raise ReleaseBranchError(
            f"origin/main workspace version {main_version} is behind release version "
            f"{release_version}; merge the release branch forward before publishing"
        )


def merge_forward(repo_root: Path, release_branch: str | None) -> None:
    ensure_clean(repo_root)
    start_branch = current_branch(repo_root)
    if not start_branch:
        raise ReleaseBranchError("detached HEAD; check out a branch first")

    branch = release_branch or start_branch
    validate_release_branch_name(branch)

    run(repo_root, "fetch", "origin", "main", branch)

    if not local_branch_exists(repo_root, branch):
        run(repo_root, "branch", "--track", branch, f"origin/{branch}")

    run(repo_root, "switch", branch)
    ensure_clean(repo_root)
    run(repo_root, "merge", "--ff-only", f"origin/{branch}")
    release_version = read_workspace_version(repo_root)

    with tempfile.TemporaryDirectory(prefix="omegon-main-version-state-") as temp:
        temp_dir = Path(temp)
        for path in VERSION_STATE_PATHS:
            (temp_dir / path.replace("/", "__")).write_bytes(
                subprocess.check_output(["git", "show", f"origin/main:{path}"], cwd=repo_root)
            )

        main_cargo = (temp_dir / "Cargo.toml").read_text()
        main_match = VERSION_RE.search(main_cargo)
        if not main_match:
            raise ReleaseBranchError("could not read workspace version from origin/main Cargo.toml")
        main_version = main_match.group(1)
        preserve_main_state = version_sort_key(main_version) >= version_sort_key(release_version)
        if not preserve_main_state:
            print(
                f"main workspace version {main_version} is behind release version "
                f"{release_version}; taking release-branch version state instead of preserving main's"
            )

        run(repo_root, "switch", "main")
        ensure_clean(repo_root)
        run(repo_root, "merge", "--ff-only", "origin/main")

        merge = subprocess.run(
            ["git", "merge", "--no-ff", "--no-commit", branch],
            cwd=repo_root,
            text=True,
            capture_output=True,
        )
        if merge.returncode != 0:
            sys.stderr.write(merge.stdout)
            sys.stderr.write(merge.stderr)
            raise ReleaseBranchError(
                "merge conflict while merging release branch forward; resolve on main"
            )

        if "Already up to date." in merge.stdout:
            print(f"main already contains {branch}")
            run(repo_root, "switch", branch)
            return

        if preserve_main_state:
            for path in VERSION_STATE_PATHS:
                target = repo_root / path
                target.write_bytes((temp_dir / path.replace("/", "__")).read_bytes())
                run(repo_root, "add", path)

        staged = run(repo_root, "diff", "--cached", "--name-only", capture=True)
        if not staged:
            run(repo_root, "merge", "--abort")
            print(f"main already contains {branch} after preserving version state")
            run(repo_root, "switch", branch)
            return

        run(repo_root, "commit", "-m", f"chore(release): merge {branch} forward")
        run(repo_root, "push", "origin", "main")
        run(repo_root, "switch", branch)

    print(f"merged {branch} forward to main and restored release working branch")


def verify_publish_invariant(repo_root: Path) -> None:
    """Verify the tagged release and public trunk cannot advertise different versions."""
    ensure_clean(repo_root)
    release_version = read_workspace_version(repo_root)
    run(repo_root, "fetch", "origin", "main")
    assert_main_version_not_behind(repo_root, release_version)
    branch = current_branch(repo_root)
    source = branch if branch else "detached release tag"
    print(
        f"publish invariant satisfied from {source}: "
        f"origin/main builds {release_version} or newer"
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("branch-release")
    merge_parser = subcommands.add_parser("merge-forward")
    merge_parser.add_argument("release_branch", nargs="?")
    subcommands.add_parser("verify-publish")
    args = parser.parse_args(argv)

    repo_root = args.repo_root.resolve()
    try:
        if args.command == "branch-release":
            create_branch(repo_root)
        elif args.command == "merge-forward":
            merge_forward(repo_root, args.release_branch)
        elif args.command == "verify-publish":
            verify_publish_invariant(repo_root)
        else:
            parser.error(f"unknown command {args.command}")
    except (ReleaseBranchError, subprocess.CalledProcessError) as err:
        print(f"error: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

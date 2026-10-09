#!/usr/bin/env python3
"""Resolve the active Rust TUI stack from Cargo metadata without hard-coded versions."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

DEFAULT_STACK = {
    "ratatui",
    "crossterm",
    "tachyonfx",
    "ratatui-image",
    "ratatui-textarea",
    "ratatui-toaster",
    "tui-tree-widget",
    "tui-popup",
    "tui-syntax-highlight",
    "ansi-to-tui",
    "hyperrat",
    "syntect",
    "unicode-width",
}


def cargo_metadata(root: Path) -> dict:
    command = ["cargo", "metadata", "--format-version", "1", "--locked"]
    try:
        result = subprocess.run(
            command,
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
            timeout=60,
            env={**os.environ, "CARGO_TERM_COLOR": "never"},
        )
    except subprocess.TimeoutExpired as error:
        raise RuntimeError("cargo metadata timed out after 60 seconds") from error
    except subprocess.CalledProcessError as error:
        detail = error.stderr[-4000:].strip()
        raise RuntimeError(f"cargo metadata failed: {detail}") from error
    if len(result.stdout) > 32 * 1024 * 1024:
        raise RuntimeError("cargo metadata output exceeded 32 MiB")
    return json.loads(result.stdout)


def select_package(metadata: dict, requested: str | None) -> dict:
    members = set(metadata.get("workspace_members", []))
    packages = [p for p in metadata["packages"] if p["id"] in members]
    if requested:
        matches = [p for p in packages if p["name"] == requested]
    else:
        matches = [p for p in packages if p["name"] == "omegon"]
        if not matches and len(packages) == 1:
            matches = packages
    if len(matches) != 1:
        names = ", ".join(sorted(p["name"] for p in packages))
        raise RuntimeError(f"select a workspace package with --package (available: {names})")
    return matches[0]


def resolve_stack(metadata: dict, package: dict) -> list[dict]:
    node = next(
        (n for n in metadata.get("resolve", {}).get("nodes", []) if n["id"] == package["id"]),
        None,
    )
    resolved_ids = {dep["pkg"]: dep for dep in (node or {}).get("deps", [])}
    packages = {p["id"]: p for p in metadata["packages"]}
    tui_members = set(package.get("features", {}).get("tui", []))
    names = set(DEFAULT_STACK)
    for member in tui_members:
        candidate = member.removeprefix("dep:").split("/", 1)[0]
        names.add(candidate)

    declared = {dep["name"]: dep for dep in package["dependencies"] if dep["name"] in names}
    rows = []
    for name, dep in sorted(declared.items()):
        resolved = next(
            (packages[pkg_id] for pkg_id in resolved_ids if packages[pkg_id]["name"] == name),
            None,
        )
        version = resolved["version"] if resolved else None
        manifest = Path(resolved["manifest_path"]) if resolved else None
        source_dir = str(manifest.parent) if manifest else None
        rows.append(
            {
                "name": name,
                "requirement": dep["req"],
                "resolved_version": version,
                "optional": dep["optional"],
                "declared_features": dep.get("features", []),
                "enabled_by_tui_feature": f"dep:{name}" in tui_members or name in tui_members,
                "source": resolved.get("source") if resolved else None,
                "local_source": source_dir,
                "docs": f"https://docs.rs/{name}/{version}/{name.replace('-', '_')}/" if version else None,
                "repository": resolved.get("repository") if resolved else None,
            }
        )
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--package")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        metadata = cargo_metadata(args.root.resolve())
        package = select_package(metadata, args.package)
        stack = resolve_stack(metadata, package)
    except (RuntimeError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    payload = {"package": package["name"], "manifest": package["manifest_path"], "stack": stack}
    if args.json:
        print(json.dumps(payload, indent=2))
        return 0
    print(f"TUI stack for {package['name']} ({package['manifest_path']})")
    for row in stack:
        resolved = row["resolved_version"] or "unresolved"
        marker = "active:tui" if row["enabled_by_tui_feature"] else "direct"
        print(f"- {row['name']} {resolved} (declared {row['requirement']}; {marker})")
        if row["declared_features"]:
            print(f"  features: {', '.join(row['declared_features'])}")
        if row["docs"]:
            print(f"  docs: {row['docs']}")
        if row["local_source"]:
            print(f"  source: {row['local_source']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

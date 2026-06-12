#!/usr/bin/env python3
"""Collect broad local headroom corpus samples from safe read-only sources.

This script materializes public-repo/document samples under .tmp/headroom and emits a
manifest consumable by scripts/headroom_dogfood.py. It never writes committed
fixtures and intentionally supports only bounded, read-only operations.
"""
from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
TMP = ROOT / ".tmp" / "headroom"
REPOS = TMP / "repos"
SOURCES = TMP / "sources"
GENERATED = TMP / "generated-corpus.json"
DEFAULT_MAX_BYTES = 500_000
SAFE_GIT_COMMANDS = {"log", "show", "grep", "diff", "ls-files"}
VALID_KIND_HINTS = {"json", "code", "log", "diff", "markdown", "plain_text"}


def slug(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-").lower() or "sample"


def require_str(obj: dict[str, Any], key: str, context: str) -> str:
    value = obj.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{context}.{key} must be a non-empty string")
    return value


def optional_int(obj: dict[str, Any], key: str, default: int, context: str) -> int:
    value = obj.get(key, default)
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ValueError(f"{context}.{key} must be a positive integer")
    return value


def validate_kind(value: str, context: str) -> str:
    if value not in VALID_KIND_HINTS:
        raise ValueError(f"{context}.kind_hint must be one of {sorted(VALID_KIND_HINTS)}")
    return value


def run(cmd: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=str(cwd) if cwd else str(ROOT),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def ensure_repo(spec: dict[str, Any], context: str) -> Path:
    name = slug(require_str(spec, "name", context))
    url = require_str(spec, "url", context)
    ref = spec.get("ref")
    if ref is not None and not isinstance(ref, str):
        raise ValueError(f"{context}.ref must be a string when present")
    path = REPOS / name
    if path.exists():
        proc = run(["git", "fetch", "--depth", "1", "origin"], cwd=path)
        if proc.returncode != 0:
            raise RuntimeError(f"git fetch failed for {name}: {proc.stderr.strip()}")
    else:
        proc = run(["git", "clone", "--depth", "1", url, str(path)])
        if proc.returncode != 0:
            raise RuntimeError(f"git clone failed for {name}: {proc.stderr.strip()}")
    if ref:
        proc = run(["git", "checkout", ref], cwd=path)
        if proc.returncode != 0:
            raise RuntimeError(f"git checkout {ref} failed for {name}: {proc.stderr.strip()}")
    return path


def clipped(text: str, max_bytes: int) -> str:
    data = text.encode("utf-8")
    if len(data) <= max_bytes:
        return text
    cut = data[:max_bytes].decode("utf-8", errors="ignore")
    return f"{cut}\n[headroom-corpus: clipped to {max_bytes} bytes]\n"


def write_sample(name: str, text: str, max_bytes: int) -> Path:
    SOURCES.mkdir(parents=True, exist_ok=True)
    path = SOURCES / f"{slug(name)}.txt"
    path.write_text(clipped(text, max_bytes), encoding="utf-8")
    return path


def manifest_file_entry(name: str, kind_hint: str, path: Path, spec: dict[str, Any], context: str) -> dict[str, Any]:
    return {
        "name": name,
        "kind_hint": validate_kind(kind_hint, context),
        "path": str(path.relative_to(ROOT)),
        "min_savings_percent": optional_int(spec, "min_savings_percent", 30, context),
        "expected_compressed": spec.get("expected_compressed", True),
        "max_output_bytes": optional_int(spec, "max_output_bytes", DEFAULT_MAX_BYTES, context),
    }


def collect_repo_file(repo: Path, repo_name: str, spec: dict[str, Any], context: str) -> dict[str, Any]:
    rel = require_str(spec, "path", context)
    src = (repo / rel).resolve()
    if not src.is_file() or not str(src).startswith(str(repo.resolve())):
        raise ValueError(f"{context}.path does not resolve to a file inside repo: {rel}")
    max_bytes = optional_int(spec, "max_output_bytes", DEFAULT_MAX_BYTES, context)
    text = src.read_text(encoding="utf-8", errors="replace")
    name = spec.get("name") if isinstance(spec.get("name"), str) else f"{repo_name}-{rel}"
    out = write_sample(name, text, max_bytes)
    return manifest_file_entry(name, require_str(spec, "kind_hint", context), out, spec, context)


def collect_repo_command(repo: Path, repo_name: str, spec: dict[str, Any], context: str) -> dict[str, Any]:
    command = spec.get("command")
    if not isinstance(command, list) or len(command) < 2 or not all(isinstance(p, str) and p for p in command):
        raise ValueError(f"{context}.command must be a non-empty string array like ['git','log',...]")
    if command[0] != "git" or command[1] not in SAFE_GIT_COMMANDS:
        raise ValueError(f"{context}.command must be a safe git command: {sorted(SAFE_GIT_COMMANDS)}")
    proc = run(command, cwd=repo)
    text = proc.stdout + (f"\n[stderr]\n{proc.stderr}" if proc.stderr else "")
    name = spec.get("name") if isinstance(spec.get("name"), str) else f"{repo_name}-{'-'.join(command[1:3])}"
    out = write_sample(name, text, optional_int(spec, "max_output_bytes", DEFAULT_MAX_BYTES, context))
    return manifest_file_entry(name, require_str(spec, "kind_hint", context), out, spec, context)


def collect_document(spec: dict[str, Any], context: str) -> dict[str, Any] | None:
    src = (ROOT / require_str(spec, "path", context)).resolve()
    optional = spec.get("optional", False)
    if not isinstance(optional, bool):
        raise ValueError(f"{context}.optional must be a boolean when present")
    if not src.is_file() or not str(src).startswith(str(ROOT)):
        if optional:
            print(f"skipping optional missing document {src}", file=sys.stderr)
            return None
        raise ValueError(f"{context}.path must be a file inside workspace/.tmp: {src}")
    text = src.read_text(encoding="utf-8", errors="replace")
    name = require_str(spec, "name", context)
    out = write_sample(name, text, optional_int(spec, "max_output_bytes", DEFAULT_MAX_BYTES, context))
    return manifest_file_entry(name, require_str(spec, "kind_hint", context), out, spec, context)


def collect(manifest: Path) -> dict[str, Any]:
    raw = json.loads(manifest.read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        raise ValueError("manifest root must be an object")
    REPOS.mkdir(parents=True, exist_ok=True)
    SOURCES.mkdir(parents=True, exist_ok=True)
    files: list[dict[str, Any]] = []
    for i, repo_spec in enumerate(raw.get("repos", [])):
        if not isinstance(repo_spec, dict):
            raise ValueError(f"repos[{i}] must be an object")
        repo_name = slug(require_str(repo_spec, "name", f"repos[{i}]"))
        repo_path = ensure_repo(repo_spec, f"repos[{i}]")
        for j, file_spec in enumerate(repo_spec.get("files", [])):
            files.append(collect_repo_file(repo_path, repo_name, file_spec, f"repos[{i}].files[{j}]"))
        for j, cmd_spec in enumerate(repo_spec.get("commands", [])):
            files.append(collect_repo_command(repo_path, repo_name, cmd_spec, f"repos[{i}].commands[{j}]"))
    for i, doc_spec in enumerate(raw.get("documents", [])):
        if not isinstance(doc_spec, dict):
            raise ValueError(f"documents[{i}] must be an object")
        collected = collect_document(doc_spec, f"documents[{i}]")
        if collected is not None:
            files.append(collected)
    return {"files": files}


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: headroom_corpus_collect.py MANIFEST.json", file=sys.stderr)
        return 2
    try:
        generated = collect((ROOT / sys.argv[1]).resolve())
    except Exception as exc:  # noqa: BLE001 - CLI reports all setup failures.
        print(f"error: {exc}", file=sys.stderr)
        return 1
    GENERATED.parent.mkdir(parents=True, exist_ok=True)
    GENERATED.write_text(json.dumps(generated, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"manifest": str(GENERATED.relative_to(ROOT)), "files": len(generated["files"])}, indent=2))
    print(f"run: python3 scripts/headroom_dogfood.py {GENERATED.relative_to(ROOT)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

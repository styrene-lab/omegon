#!/usr/bin/env python3
"""Generate a small Omegon evidence-map surface stream from rustdoc JSON.

Prototype dogfood generator for `.omegon/evidence/`.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import time
from typing import Any

STREAMS = {
    "records": "records.jsonl",
    "surfaces": "surfaces.jsonl",
    "edges": "edges.jsonl",
    "artifacts": "artifacts.jsonl",
}


def run(cmd: list[str], cwd: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)


def git_output(cwd: pathlib.Path, args: list[str]) -> str | None:
    proc = run(["git", *args], cwd)
    if proc.returncode != 0:
        return None
    return proc.stdout.strip()


def source_state(cwd: pathlib.Path) -> dict[str, Any]:
    diff = run(["git", "diff", "--binary", "--", "."], cwd).stdout
    untracked = git_output(cwd, ["ls-files", "--others", "--exclude-standard", "--", "."]) or ""
    material = diff + "\n-- untracked --\n" + untracked
    return {
        "git_head": git_output(cwd, ["rev-parse", "HEAD"]),
        "branch": git_output(cwd, ["branch", "--show-current"]),
        "worktree_diff_hash": "sha256:" + hashlib.sha256(material.encode()).hexdigest(),
        "dirty": bool((git_output(cwd, ["status", "--porcelain=v1"]) or "").strip()),
    }


def now_ms() -> int:
    return int(time.time() * 1000)


def normalize_visibility(value: Any) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        return next(iter(value.keys()), "restricted")
    return "unknown"


def inner_kind(inner: dict[str, Any]) -> str | None:
    for key in ["module", "struct", "enum", "trait", "function", "impl", "type_alias", "constant", "static"]:
        if key in inner:
            return key
    return None


def surface_kind(kind: str) -> str:
    return {
        "module": "rust-module",
        "struct": "rust-struct",
        "enum": "rust-enum",
        "trait": "rust-trait",
        "function": "rust-function",
        "impl": "rust-impl",
        "type_alias": "rust-type-alias",
        "constant": "rust-constant",
        "static": "rust-static",
    }.get(kind, f"rust-{kind}")


def source_hash(project_root: pathlib.Path, span: dict[str, Any] | None) -> str | None:
    if not span:
        return None
    path = project_root / span.get("filename", "")
    if not path.is_file():
        alt = next(
            (
                project_root / candidate / span.get("filename", "")
                for candidate in ["extensions/omegon-tdd-savepoint"]
                if (project_root / candidate / span.get("filename", "")).is_file()
            ),
            None,
        )
        if alt is None:
            return None
        path = alt
    try:
        return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
    except OSError:
        return None


def item_path(doc: dict[str, Any], item_id: str, item: dict[str, Any]) -> str:
    path = doc.get("paths", {}).get(str(item_id), {}).get("path")
    if path:
        return "::".join(path)
    name = item.get("name")
    if name:
        return f"omegon_tdd_savepoint::{name}"
    return f"omegon_tdd_savepoint::item_{item_id}"


def signature(item: dict[str, Any], kind: str) -> str | None:
    name = item.get("name") or "<anonymous>"
    inner = item.get("inner", {}).get(kind, {})
    if kind == "function":
        return f"fn {name}(...)"
    if kind in {"struct", "enum", "trait"}:
        return f"{kind} {name}"
    if kind == "module":
        return f"mod {name}"
    return None


def generate_surfaces(doc: dict[str, Any], project_root: pathlib.Path, crate_name: str) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    created = now_ms()
    for item_id, item in doc.get("index", {}).items():
        inner = item.get("inner") or {}
        if not isinstance(inner, dict):
            continue
        kind = inner_kind(inner)
        if not kind:
            continue
        span = item.get("span")
        filename = span.get("filename") if isinstance(span, dict) else None
        # Keep this prototype project-local and skip dependency surfaces.
        if filename and (filename.startswith("/") or "/.cargo/" in filename):
            continue
        full = item_path(doc, item_id, item)
        name = item.get("name") or full.split("::")[-1]
        if kind == "impl" and not item.get("name"):
            # Anonymous impl blocks are noisy in the first dogfood surface map;
            # methods and associated items are emitted separately.
            continue
        sid = f"surface:rust:{full}"
        record = {
            "schema": "surface-record/v1",
            "id": sid,
            "kind": surface_kind(kind),
            "name": name,
            "source_path": filename,
            "source_span": None,
            "visibility": normalize_visibility(item.get("visibility")),
            "signature": signature(item, kind),
            "description": item.get("docs"),
            "extractor": "rustdoc-json",
            "source_hash": source_hash(project_root, span if isinstance(span, dict) else None),
            "created_at_ms": created,
            "metadata": {
                "crate": crate_name,
                "rustdoc_id": item_id,
                "rustdoc_kind": kind,
                "docs_present": bool(item.get("docs")),
            },
        }
        if isinstance(span, dict):
            begin = span.get("begin") or [None, None]
            end = span.get("end") or [None, None]
            record["source_span"] = {
                "start_line": begin[0],
                "start_col": begin[1],
                "end_line": end[0],
                "end_col": end[1],
            }
        out.append(record)
    return out


def write_jsonl(path: pathlib.Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, sort_keys=True, separators=(",", ":")))
            f.write("\n")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest-path", default="extensions/omegon-tdd-savepoint/Cargo.toml")
    ap.add_argument("--crate-name", default="omegon-tdd-savepoint")
    ap.add_argument("--project-root", default=".")
    ap.add_argument("--no-run-rustdoc", action="store_true")
    args = ap.parse_args()

    root = pathlib.Path(args.project_root).resolve()
    manifest = pathlib.Path(args.manifest_path)
    doc_json = root / manifest.parent / "target/doc/omegon_tdd_savepoint.json"

    stderr_tail = ""
    status = "surface-pass"
    command = [
        "cargo", "+nightly", "rustdoc", "--manifest-path", str(manifest), "--",
        "-Z", "unstable-options", "--output-format", "json",
    ]
    if not args.no_run_rustdoc:
        proc = run(command, root)
        stderr_tail = proc.stderr[-8192:]
        if proc.returncode != 0:
            status = "surface-fail"
    if not doc_json.is_file():
        status = "surface-fail"
        doc = {"index": {}}
    else:
        doc = json.loads(doc_json.read_text())

    surfaces = generate_surfaces(doc, root, args.crate_name) if status != "surface-fail" else []
    evidence_dir = root / ".omegon/evidence"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    for name in STREAMS.values():
        (evidence_dir / name).touch()

    write_jsonl(evidence_dir / "surfaces.jsonl", surfaces)
    state = source_state(root)
    created = now_ms()
    manifest_doc = {
        "schema": "omegon-evidence-manifest/v1",
        "generator": {"name": "omegon-surface-prototype", "version": "0.1.0"},
        "project": {"root": ".", "name": root.name},
        "created_at_ms": created,
        "source_state": state,
        "files": STREAMS,
        "providers": [
            {"id": "surface-map", "kind": "surface-index", "raw_roots": [str(doc_json.relative_to(root)) if doc_json.exists() else str(doc_json)]},
        ],
    }
    (evidence_dir / "manifest.json").write_text(json.dumps(manifest_doc, indent=2, sort_keys=True) + "\n")

    evidence = {
        "schema": "evidence-record/v1",
        "id": f"evidence:surface-map:rust:{created}",
        "provider": "surface-map",
        "kind": "rust-surface",
        "status": status,
        "subjects": [f"project:{root.name}", f"crate:{args.crate_name}"],
        "claims": [],
        "artifacts": [f"path:{doc_json.relative_to(root)}"] if doc_json.exists() else [],
        "source_state": state,
        "created_at_ms": created,
        "metadata": {
            "extractor": "rustdoc-json",
            "command": command,
            "surface_count": len(surfaces),
            "rustdoc_format_version": doc.get("format_version"),
            "stderr_tail": stderr_tail,
        },
    }
    with (evidence_dir / "records.jsonl").open("a", encoding="utf-8") as f:
        f.write(json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n")
    print(json.dumps({"status": status, "surface_count": len(surfaces), "evidence_dir": str(evidence_dir)}, indent=2))
    return 0 if status != "surface-fail" else 1


if __name__ == "__main__":
    raise SystemExit(main())

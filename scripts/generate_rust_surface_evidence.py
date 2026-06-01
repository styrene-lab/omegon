#!/usr/bin/env python3
"""Generate a small Omegon evidence-map surface stream from rustdoc JSON.

Prototype dogfood generator for `.omegon/evidence/`.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import time
from typing import Any

STREAMS = {
    "records": "records.jsonl",
    "surfaces": "surfaces.jsonl",
    "edges": "edges.jsonl",
    "artifacts": "artifacts.jsonl",
}
PROTOTYPE_VERSION = "0.3.0"
DEFAULT_MANIFEST = pathlib.Path("extensions/omegon-tdd-savepoint/Cargo.toml")
DEFAULT_CRATE = "omegon-tdd-savepoint"
DEFAULT_RUSTDOC_JSON = pathlib.Path("extensions/omegon-tdd-savepoint/target/doc/omegon_tdd_savepoint.json")
SOURCE_PREFIX_CANDIDATES = [pathlib.Path("extensions/omegon-tdd-savepoint")]


def run(cmd: list[str], cwd: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, check=False)


def git_output(cwd: pathlib.Path, args: list[str]) -> str | None:
    proc = run(["git", *args], cwd)
    if proc.returncode != 0:
        return None
    return proc.stdout.strip()


def source_state(cwd: pathlib.Path, scopes: list[pathlib.Path] | None = None) -> dict[str, Any]:
    scope_args = [str(scope) for scope in (scopes or [pathlib.Path(".")])]
    diff = run(["git", "diff", "--binary", "--", *scope_args], cwd).stdout
    untracked = git_output(cwd, ["ls-files", "--others", "--exclude-standard", "--", *scope_args]) or ""
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


def resolve_source_path(project_root: pathlib.Path, filename: str | None) -> pathlib.Path | None:
    if not filename:
        return None
    path = pathlib.Path(filename)
    if path.is_absolute():
        return path if path.is_file() and project_root in path.parents else None
    direct = project_root / path
    if direct.is_file():
        return direct
    return next(
        (candidate for prefix in SOURCE_PREFIX_CANDIDATES if (candidate := project_root / prefix / path).is_file()),
        None,
    )


def project_relative(project_root: pathlib.Path, path: pathlib.Path | None, fallback: str | None = None) -> str | None:
    if path is None:
        return fallback
    try:
        return str(path.relative_to(project_root))
    except ValueError:
        return fallback or str(path)


def source_hash(project_root: pathlib.Path, span: dict[str, Any] | None) -> str | None:
    if not span:
        return None
    path = resolve_source_path(project_root, span.get("filename"))
    if path is None:
        return None
    try:
        return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
    except OSError:
        return None


def rust_type_to_string(value: Any) -> str:
    if isinstance(value, str):
        return value
    if not isinstance(value, dict):
        return "_"
    if "primitive" in value:
        return str(value["primitive"])
    if "resolved_path" in value:
        return value["resolved_path"].get("path", "_")
    if "borrowed_ref" in value:
        inner = value["borrowed_ref"]
        prefix = "&mut " if inner.get("is_mutable") else "&"
        return prefix + rust_type_to_string(inner.get("type"))
    if "generic" in value:
        return value["generic"]
    if "tuple" in value:
        return "(" + ", ".join(rust_type_to_string(v) for v in value["tuple"]) + ")"
    if "slice" in value:
        return "[" + rust_type_to_string(value["slice"]) + "]"
    if "array" in value:
        arr = value["array"]
        return f"[{rust_type_to_string(arr.get('type'))}; {arr.get('len', '_')}]"
    return next(iter(value.keys()), "_")


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
        func = inner.get("function", inner) if isinstance(inner, dict) else {}
        sig = func.get("sig", {}) if isinstance(func, dict) else {}
        inputs = sig.get("inputs", []) if isinstance(sig, dict) else []
        rendered = []
        for arg in inputs:
            if isinstance(arg, list) and len(arg) == 2:
                rendered.append(f"{arg[0]}: {rust_type_to_string(arg[1])}")
        output = sig.get("output")
        suffix = "" if output is None else f" -> {rust_type_to_string(output)}"
        return f"fn {name}({', '.join(rendered)}){suffix}"
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
        if kind == "function" and item.get("name") in {"serialize", "deserialize"}:
            # rustdoc JSON exposes derive-generated serde methods as ordinary
            # functions at the derive span. They are implementation noise for
            # the high-level evidence map; the owning types remain represented.
            continue
        sid = f"surface:rust:{full}"
        resolved_source = resolve_source_path(project_root, filename)
        record = {
            "schema": "surface-record/v1",
            "id": sid,
            "kind": surface_kind(kind),
            "name": name,
            "source_path": project_relative(project_root, resolved_source, filename),
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


def artifact_id_for_path(path: str) -> str:
    safe = path.replace("/", ":").replace(".", "_").replace(" ", "_")
    return f"artifact:path:{safe}"


def source_id_for_path(path: str) -> str:
    return f"source:{path}"


def crate_id(crate_name: str) -> str:
    return f"crate:{crate_name}"


def make_edge(source: str, target: str, kind: str, created_at_ms: int) -> dict[str, Any]:
    return {
        "schema": "evidence-edge/v1",
        "from": source,
        "to": target,
        "kind": kind,
        "created_at_ms": created_at_ms,
    }


def write_jsonl(path: pathlib.Path, rows: list[dict[str, Any]]) -> None:
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, sort_keys=True, separators=(",", ":")))
            f.write("\n")


def generate_edges(surfaces: list[dict[str, Any]], evidence_id: str, artifact_id: str, crate_name: str, created_at_ms: int) -> list[dict[str, Any]]:
    edges: list[dict[str, Any]] = []
    seen: set[tuple[str, str, str]] = set()

    def add(source: str, target: str, kind: str) -> None:
        key = (source, target, kind)
        if key not in seen:
            seen.add(key)
            edges.append(make_edge(source, target, kind, created_at_ms))

    crate = crate_id(crate_name)
    add(evidence_id, artifact_id, "generated_from")
    add(evidence_id, crate, "subjects")
    for surface in surfaces:
        sid = surface["id"]
        add(evidence_id, sid, "subjects")
        add(sid, crate, "belongs_to")
        if surface.get("source_path"):
            add(sid, source_id_for_path(surface["source_path"]), "declared_in")
        add(sid, artifact_id, "generated_from")
    return edges


def generate_artifacts(doc_json: pathlib.Path, project_root: pathlib.Path, created_at_ms: int) -> list[dict[str, Any]]:
    if not doc_json.exists():
        return []
    rel = str(doc_json.relative_to(project_root))
    return [
        {
            "schema": "artifact-record/v1",
            "id": artifact_id_for_path(rel),
            "kind": "rustdoc-json",
            "provider": "surface-map",
            "path": rel,
            "open_with": "editor",
            "hash": "sha256:" + hashlib.sha256(doc_json.read_bytes()).hexdigest(),
            "created_at_ms": created_at_ms,
        }
    ]


def doc_coverage_evidence(surfaces: list[dict[str, Any]], root_name: str, crate_name: str, state: dict[str, Any], created_at_ms: int) -> dict[str, Any]:
    public_surfaces = [s for s in surfaces if s.get("visibility") == "public"]
    public_missing = [s["id"] for s in public_surfaces if not s.get("metadata", {}).get("docs_present")]
    documented = len(public_surfaces) - len(public_missing)
    status = "docs-pass" if not public_missing else "docs-warnings"
    return {
        "schema": "evidence-record/v1",
        "id": f"evidence:surface-map:rust-doc-coverage:{created_at_ms}",
        "provider": "surface-map",
        "kind": "rust-doc-coverage",
        "status": status,
        "subjects": [f"project:{root_name}", crate_id(crate_name)],
        "claims": [],
        "artifacts": ["path:.omegon/evidence/surfaces.jsonl"],
        "source_state": state,
        "created_at_ms": created_at_ms,
        "metadata": {
            "extractor": "rustdoc-json",
            "public_surface_count": len(public_surfaces),
            "public_documented_count": documented,
            "public_missing_docs_count": len(public_missing),
            "public_missing_docs": public_missing[:100],
        },
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest-path", default=str(DEFAULT_MANIFEST))
    ap.add_argument("--crate-name", default=DEFAULT_CRATE)
    ap.add_argument("--project-root", default=".")
    ap.add_argument("--no-run-rustdoc", action="store_true")
    args = ap.parse_args()

    root = pathlib.Path(args.project_root).resolve()
    manifest = pathlib.Path(args.manifest_path)
    doc_json = root / DEFAULT_RUSTDOC_JSON

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

    scopes = [manifest.parent, pathlib.Path("scripts/generate_rust_surface_evidence.py")]
    state = source_state(root, scopes)
    created = now_ms()
    rustdoc_artifact_id = artifact_id_for_path(str(doc_json.relative_to(root))) if doc_json.exists() else "artifact:rustdoc-json:missing"
    surface_evidence_id = f"evidence:surface-map:rust:{created}"
    artifacts = generate_artifacts(doc_json, root, created)
    edges = generate_edges(surfaces, surface_evidence_id, rustdoc_artifact_id, args.crate_name, created)

    write_jsonl(evidence_dir / "surfaces.jsonl", surfaces)
    write_jsonl(evidence_dir / "artifacts.jsonl", artifacts)
    write_jsonl(evidence_dir / "edges.jsonl", edges)
    manifest_doc = {
        "schema": "omegon-evidence-manifest/v1",
        "generator": {"name": "omegon-surface-prototype", "version": PROTOTYPE_VERSION},
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
        "id": surface_evidence_id,
        "provider": "surface-map",
        "kind": "rust-surface",
        "status": status,
        "subjects": [f"project:{root.name}", f"crate:{args.crate_name}"],
        "claims": [],
        "artifacts": [rustdoc_artifact_id] if doc_json.exists() else [],
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
    coverage = doc_coverage_evidence(surfaces, root.name, args.crate_name, state, created)
    with (evidence_dir / "records.jsonl").open("a", encoding="utf-8") as f:
        f.write(json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n")
        f.write(json.dumps(coverage, sort_keys=True, separators=(",", ":")) + "\n")
    print(json.dumps({"status": status, "surface_count": len(surfaces), "edge_count": len(edges), "artifact_count": len(artifacts), "doc_coverage_status": coverage["status"], "evidence_dir": str(evidence_dir)}, indent=2))
    return 0 if status != "surface-fail" else 1


if __name__ == "__main__":
    raise SystemExit(main())

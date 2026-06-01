#!/usr/bin/env python3
"""Generate a small Omegon evidence-map surface stream from rustdoc JSON.

Prototype dogfood generator for `.omegon/evidence/`.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import time
from typing import Any

STREAMS = {
    "claims": "claims.jsonl",
    "records": "records.jsonl",
    "surfaces": "surfaces.jsonl",
    "edges": "edges.jsonl",
    "artifacts": "artifacts.jsonl",
    "summaries": "summaries/",
}
PROTOTYPE_VERSION = "0.3.0"
DEFAULT_MANIFEST = pathlib.Path("extensions/omegon-tdd-savepoint/Cargo.toml")
DEFAULT_CRATE = "omegon-tdd-savepoint"
PROVIDER_ID = "code-evidence"
MAX_RECORD_HISTORY = 20


def run(cmd: list[str], cwd: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, check=False)


def crate_doc_stem(crate_name: str) -> str:
    return crate_name.replace("-", "_")


def rustdoc_json_path(manifest: pathlib.Path, crate_name: str, root: pathlib.Path) -> pathlib.Path:
    return root / manifest.parent / "target" / "doc" / f"{crate_doc_stem(crate_name)}.json"


def source_prefix_candidates(manifest: pathlib.Path) -> list[pathlib.Path]:
    return [manifest.parent]


def safe_id_component(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.:-]+", "-", value).strip("-") or "unknown"


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


def resolve_source_path(project_root: pathlib.Path, filename: str | None, prefixes: list[pathlib.Path]) -> pathlib.Path | None:
    if not filename:
        return None
    path = pathlib.Path(filename)
    if path.is_absolute():
        return path if path.is_file() and project_root in path.parents else None
    direct = project_root / path
    if direct.is_file():
        return direct
    return next(
        (candidate for prefix in prefixes if (candidate := project_root / prefix / path).is_file()),
        None,
    )


def project_relative(project_root: pathlib.Path, path: pathlib.Path | None, fallback: str | None = None) -> str | None:
    if path is None:
        return fallback
    try:
        return str(path.relative_to(project_root))
    except ValueError:
        return fallback or str(path)


def source_hash(project_root: pathlib.Path, span: dict[str, Any] | None, prefixes: list[pathlib.Path]) -> str | None:
    if not span:
        return None
    path = resolve_source_path(project_root, span.get("filename"), prefixes)
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


def generate_surfaces(doc: dict[str, Any], project_root: pathlib.Path, crate_name: str, prefixes: list[pathlib.Path]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
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
        if sid in seen_ids:
            sid = f"{sid}:rustdoc-{item_id}"
        seen_ids.add(sid)
        resolved_source = resolve_source_path(project_root, filename, prefixes)
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
            "source_hash": source_hash(project_root, span if isinstance(span, dict) else None, prefixes),
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


def jsonl_rows(path: pathlib.Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.is_file():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows



def legacy_provider_ids(provider: str) -> set[str]:
    return {provider, "surface-map"}


def compact_provider_records(path: pathlib.Path, provider: str) -> None:
    providers = legacy_provider_ids(provider)
    existing = [row for row in jsonl_rows(path) if row.get("provider") not in providers]
    write_jsonl(path, existing)


def trim_provider_history(path: pathlib.Path, provider: str, max_records: int) -> None:
    providers = legacy_provider_ids(provider)
    rows = jsonl_rows(path)
    provider_rows = [row for row in rows if row.get("provider") in providers]
    if len(provider_rows) <= max_records:
        return
    keep_ids = {row.get("id") for row in provider_rows[-max_records:]}
    trimmed = [row for row in rows if row.get("provider") not in providers or row.get("id") in keep_ids]
    write_jsonl(path, trimmed)


def prune_legacy_provider_edges(path: pathlib.Path) -> None:
    rows = [
        row
        for row in jsonl_rows(path)
        if "evidence:surface-map:" not in row.get("from", "")
        and "evidence:surface-map:" not in row.get("to", "")
    ]
    write_jsonl(path, rows)

def init_sqlite_index(evidence_dir: pathlib.Path) -> None:
    import sqlite3

    index_dir = evidence_dir / "indexes"
    index_dir.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(index_dir / "evidence.sqlite")
    try:
        conn.executescript("""
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS claims (id TEXT PRIMARY KEY, kind TEXT, text TEXT, status TEXT, created_at_ms INTEGER, raw_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS evidence_records (id TEXT PRIMARY KEY, provider TEXT NOT NULL, kind TEXT NOT NULL, status TEXT NOT NULL, created_at_ms INTEGER, source_git_head TEXT, source_diff_hash TEXT, raw_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS surfaces (id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, source_path TEXT, start_line INTEGER, end_line INTEGER, visibility TEXT, source_hash TEXT, extractor TEXT, created_at_ms INTEGER, raw_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS artifacts (id TEXT PRIMARY KEY, provider TEXT, kind TEXT NOT NULL, path TEXT, uri TEXT, open_with TEXT, hash TEXT, created_at_ms INTEGER, raw_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS edges (id INTEGER PRIMARY KEY AUTOINCREMENT, from_id TEXT NOT NULL, to_id TEXT NOT NULL, kind TEXT NOT NULL, created_at_ms INTEGER, raw_json TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id);
            CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id);
            CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
            CREATE VIRTUAL TABLE IF NOT EXISTS evidence_fts USING fts5(id, kind, text, content);
        """)
        for table in ["claims", "evidence_records", "surfaces", "artifacts", "edges", "evidence_fts"]:
            conn.execute(f"DELETE FROM {table}")
        load_sqlite_index(conn, evidence_dir)
        conn.commit()
    finally:
        conn.close()


def load_sqlite_index(conn: Any, evidence_dir: pathlib.Path) -> None:
    for row in jsonl_rows(evidence_dir / "claims.jsonl"):
        raw = json.dumps(row, sort_keys=True)
        conn.execute("INSERT OR REPLACE INTO claims VALUES (?, ?, ?, ?, ?, ?)", (row.get("id"), row.get("kind"), row.get("text"), row.get("status"), row.get("created_at_ms"), raw))
        conn.execute("INSERT INTO evidence_fts VALUES (?, ?, ?, ?)", (row.get("id"), row.get("kind"), row.get("text", ""), raw))
    for row in jsonl_rows(evidence_dir / "records.jsonl"):
        raw = json.dumps(row, sort_keys=True)
        source = row.get("source_state") or {}
        conn.execute("INSERT OR REPLACE INTO evidence_records VALUES (?, ?, ?, ?, ?, ?, ?, ?)", (row.get("id"), row.get("provider"), row.get("kind"), row.get("status"), row.get("created_at_ms"), source.get("git_head"), source.get("worktree_diff_hash"), raw))
        conn.execute("INSERT INTO evidence_fts VALUES (?, ?, ?, ?)", (row.get("id"), row.get("kind"), " ".join(row.get("subjects") or []), raw))
    for row in jsonl_rows(evidence_dir / "surfaces.jsonl"):
        raw = json.dumps(row, sort_keys=True)
        span = row.get("source_span") or {}
        conn.execute("INSERT OR REPLACE INTO surfaces VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)", (row.get("id"), row.get("kind"), row.get("name"), row.get("source_path"), span.get("start_line"), span.get("end_line"), row.get("visibility"), row.get("source_hash"), row.get("extractor"), row.get("created_at_ms"), raw))
        conn.execute("INSERT INTO evidence_fts VALUES (?, ?, ?, ?)", (row.get("id"), row.get("kind"), row.get("signature") or row.get("name") or "", raw))
    for row in jsonl_rows(evidence_dir / "artifacts.jsonl"):
        raw = json.dumps(row, sort_keys=True)
        conn.execute("INSERT OR REPLACE INTO artifacts VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)", (row.get("id"), row.get("provider"), row.get("kind"), row.get("path"), row.get("uri"), row.get("open_with"), row.get("hash"), row.get("created_at_ms"), raw))
    for row in jsonl_rows(evidence_dir / "edges.jsonl"):
        conn.execute("INSERT INTO edges(from_id, to_id, kind, created_at_ms, raw_json) VALUES (?, ?, ?, ?, ?)", (row.get("from"), row.get("to"), row.get("kind"), row.get("created_at_ms"), json.dumps(row, sort_keys=True)))


def generate_edges(
    surfaces: list[dict[str, Any]],
    evidence_id: str,
    artifact_id: str,
    crate_name: str,
    created_at_ms: int,
    doc_coverage_id: str | None = None,
    public_docs_claim_id: str | None = None,
    doc_coverage_status: str | None = None,
) -> list[dict[str, Any]]:
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
    if doc_coverage_id and public_docs_claim_id:
        add(doc_coverage_id, crate, "subjects")
        add(doc_coverage_id, public_docs_claim_id, "supports" if doc_coverage_status == "docs-pass" else "refutes")
    return edges


def artifact_record(path: pathlib.Path, project_root: pathlib.Path, kind: str, created_at_ms: int, open_with: str = "editor") -> dict[str, Any] | None:
    if not path.exists():
        return None
    rel = str(path.relative_to(project_root))
    return {
        "schema": "artifact-record/v1",
        "id": artifact_id_for_path(rel),
        "kind": kind,
        "provider": PROVIDER_ID,
        "path": rel,
        "open_with": open_with,
        "hash": "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest(),
        "created_at_ms": created_at_ms,
    }


def generate_artifacts(doc_json: pathlib.Path, project_root: pathlib.Path, created_at_ms: int) -> list[dict[str, Any]]:
    record = artifact_record(doc_json, project_root, "rustdoc-json", created_at_ms)
    return [record] if record else []


def write_doc_coverage_summary(path: pathlib.Path, surfaces: list[dict[str, Any]], coverage: dict[str, Any], crate_name: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    missing_ids = set(coverage.get("metadata", {}).get("public_missing_docs", []))
    by_id = {surface["id"]: surface for surface in surfaces}
    lines = [
        "# Rust Documentation Coverage",
        "",
        f"Crate: `{crate_name}`",
        "",
        f"Status: `{coverage['status']}`",
        f"Public surfaces: {coverage['metadata']['public_surface_count']}",
        f"Documented public surfaces: {coverage['metadata']['public_documented_count']}",
        f"Missing public docs: {coverage['metadata']['public_missing_docs_count']}",
        "",
        f"Claim: `{coverage['claims'][0]}`",
        f"Evidence: `{coverage['id']}`",
        "",
        "## Missing public docs",
        "",
    ]
    if not missing_ids:
        lines.append("All public surfaces have rustdoc comments.")
    else:
        for sid in sorted(missing_ids):
            surface = by_id.get(sid, {})
            span = surface.get("source_span") or {}
            source = surface.get("source_path") or "<unknown>"
            line = span.get("start_line")
            anchor = f"{source}:{line}" if line else source
            lines.append(f"- `{sid}` — {anchor}")
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def public_docs_claim(crate_name: str, created_at_ms: int) -> dict[str, Any]:
    return {
        "schema": "claim-record/v1",
        "id": f"claim:crate:{crate_name}:public-api-documented",
        "kind": "documentation-quality",
        "text": f"Public Rust API surfaces for crate {crate_name} are documented.",
        "status": "asserted",
        "scope": [crate_id(crate_name)],
        "created_at_ms": created_at_ms,
        "metadata": {"provider": PROVIDER_ID, "threshold": "all public surfaces have rustdoc docs"},
    }


def doc_coverage_evidence(surfaces: list[dict[str, Any]], root_name: str, crate_name: str, state: dict[str, Any], created_at_ms: int) -> dict[str, Any]:
    public_surfaces = [s for s in surfaces if s.get("visibility") == "public"]
    public_missing = [s["id"] for s in public_surfaces if not s.get("metadata", {}).get("docs_present")]
    documented = len(public_surfaces) - len(public_missing)
    status = "docs-pass" if not public_missing else "docs-warnings"
    return {
        "schema": "evidence-record/v1",
        "id": f"evidence:{PROVIDER_ID}:rust-doc-coverage:{created_at_ms}",
        "provider": PROVIDER_ID,
        "kind": "rust-doc-coverage",
        "status": status,
        "subjects": [f"project:{root_name}", crate_id(crate_name)],
        "claims": [f"claim:crate:{crate_name}:public-api-documented"],
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
    ap.add_argument("--output-dir", default=".omegon/evidence")
    ap.add_argument("--replace-provider-records", action="store_true", default=True)
    ap.add_argument("--append-records", dest="replace_provider_records", action="store_false")
    ap.add_argument("--no-run-rustdoc", action="store_true")
    args = ap.parse_args()

    root = pathlib.Path(args.project_root).resolve()
    manifest = pathlib.Path(args.manifest_path)
    doc_json = rustdoc_json_path(manifest, args.crate_name, root)
    prefixes = source_prefix_candidates(manifest)

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

    surfaces = generate_surfaces(doc, root, args.crate_name, prefixes) if status != "surface-fail" else []
    evidence_dir = root / args.output_dir
    evidence_dir.mkdir(parents=True, exist_ok=True)
    for name in STREAMS.values():
        (evidence_dir / name).touch()

    scopes = [manifest.parent, pathlib.Path("scripts/generate_rust_surface_evidence.py")]
    state = source_state(root, scopes)
    created = now_ms()
    rustdoc_artifact_id = artifact_id_for_path(str(doc_json.relative_to(root))) if doc_json.exists() else "artifact:rustdoc-json:missing"
    provider_slug = safe_id_component(PROVIDER_ID)
    surface_evidence_id = f"evidence:{provider_slug}:rust:{created}"
    artifacts = generate_artifacts(doc_json, root, created)
    coverage = doc_coverage_evidence(surfaces, root.name, args.crate_name, state, created)
    summary_path = evidence_dir / "summaries" / "rust-doc-coverage.md"
    write_doc_coverage_summary(summary_path, surfaces, coverage, args.crate_name)
    summary_artifact = artifact_record(summary_path, root, "doc-coverage-summary", created, "markdown")
    if summary_artifact:
        artifacts.append(summary_artifact)
    claims = [public_docs_claim(args.crate_name, created)]
    edges = generate_edges(
        surfaces,
        surface_evidence_id,
        rustdoc_artifact_id,
        args.crate_name,
        created,
        coverage["id"],
        claims[0]["id"],
        coverage["status"],
    )

    write_jsonl(evidence_dir / "claims.jsonl", claims)
    write_jsonl(evidence_dir / "surfaces.jsonl", surfaces)
    if summary_artifact:
        edges.append(make_edge(coverage["id"], summary_artifact["id"], "summarized_by", created))
    write_jsonl(evidence_dir / "artifacts.jsonl", artifacts)
    write_jsonl(evidence_dir / "edges.jsonl", edges)
    manifest_doc = {
        "schema": "omegon-evidence-manifest/v1",
        "generator": {"name": "omegon-code-evidence-prototype", "version": PROTOTYPE_VERSION},
        "project": {"root": ".", "name": root.name},
        "created_at_ms": created,
        "source_state": state,
        "files": STREAMS,
        "indexes": {
            "sqlite": {
                "kind": "sqlite",
                "path": ".omegon/evidence/indexes/evidence.sqlite",
                "derived": True,
            }
        },
        "providers": [
            {"id": PROVIDER_ID, "kind": "surface-index", "raw_roots": [str(doc_json.relative_to(root)) if doc_json.exists() else str(doc_json)]},
        ],
    }
    (evidence_dir / "manifest.json").write_text(json.dumps(manifest_doc, indent=2, sort_keys=True) + "\n")

    evidence = {
        "schema": "evidence-record/v1",
        "id": surface_evidence_id,
        "provider": PROVIDER_ID,
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
    if args.replace_provider_records:
        compact_provider_records(evidence_dir / "records.jsonl", PROVIDER_ID)
        prune_legacy_provider_edges(evidence_dir / "edges.jsonl")
    with (evidence_dir / "records.jsonl").open("a", encoding="utf-8") as f:
        f.write(json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n")
        f.write(json.dumps(coverage, sort_keys=True, separators=(",", ":")) + "\n")
    trim_provider_history(evidence_dir / "records.jsonl", PROVIDER_ID, MAX_RECORD_HISTORY)
    init_sqlite_index(evidence_dir)
    print(json.dumps({"status": status, "surface_count": len(surfaces), "edge_count": len(edges), "artifact_count": len(artifacts), "doc_coverage_status": coverage["status"], "sqlite_index": str(evidence_dir / "indexes/evidence.sqlite"), "evidence_dir": str(evidence_dir)}, indent=2))
    return 0 if status != "surface-fail" else 1


if __name__ == "__main__":
    raise SystemExit(main())

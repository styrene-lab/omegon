#!/usr/bin/env python3
"""Query the derived Omegon evidence SQLite index."""
from __future__ import annotations

import argparse
import json
import pathlib
import sqlite3
import sys
from typing import Any

DEFAULT_DB = pathlib.Path(".omegon/evidence/indexes/evidence.sqlite")


def connect(path: pathlib.Path) -> sqlite3.Connection:
    if not path.is_file():
        raise SystemExit(f"evidence index not found: {path}")
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    return conn


def print_json(value: Any) -> None:
    print(json.dumps(value, indent=2, sort_keys=True))


def fts_query(text: str) -> str:
    # FTS5 treats hyphen as an operator unless quoted. Use a phrase query for
    # operator-facing search strings; callers still get canonical record IDs.
    escaped = text.replace('"', '""')
    return f'"{escaped}"'


def cmd_search(conn: sqlite3.Connection, text: str, limit: int) -> None:
    rows = conn.execute(
        "SELECT id, kind, text FROM evidence_fts WHERE evidence_fts MATCH ? LIMIT ?",
        (fts_query(text), limit),
    ).fetchall()
    print_json([dict(row) for row in rows])


def row_by_id(conn: sqlite3.Connection, ident: str) -> dict[str, Any] | None:
    for table, column in [
        ("claims", "id"),
        ("evidence_records", "id"),
        ("surfaces", "id"),
        ("artifacts", "id"),
    ]:
        row = conn.execute(f"SELECT raw_json FROM {table} WHERE {column} = ?", (ident,)).fetchone()
        if row:
            return json.loads(row["raw_json"])
    return None


def cmd_get(conn: sqlite3.Connection, ident: str) -> None:
    row = row_by_id(conn, ident)
    if row is None:
        raise SystemExit(f"not found: {ident}")
    print_json(row)


def cmd_neighbors(conn: sqlite3.Connection, ident: str) -> None:
    outgoing = [dict(row) for row in conn.execute("SELECT from_id, to_id, kind FROM edges WHERE from_id = ? ORDER BY kind, to_id", (ident,))]
    incoming = [dict(row) for row in conn.execute("SELECT from_id, to_id, kind FROM edges WHERE to_id = ? ORDER BY kind, from_id", (ident,))]
    print_json({"id": ident, "outgoing": outgoing, "incoming": incoming})


def cmd_claims(conn: sqlite3.Connection) -> None:
    rows = conn.execute("SELECT id, kind, status, text FROM claims ORDER BY id").fetchall()
    print_json([dict(row) for row in rows])


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=str(DEFAULT_DB))
    sub = ap.add_subparsers(dest="cmd", required=True)
    search = sub.add_parser("search")
    search.add_argument("text")
    search.add_argument("--limit", type=int, default=20)
    get = sub.add_parser("get")
    get.add_argument("id")
    neighbors = sub.add_parser("neighbors")
    neighbors.add_argument("id")
    sub.add_parser("claims")
    args = ap.parse_args(argv)
    conn = connect(pathlib.Path(args.db))
    if args.cmd == "search":
        cmd_search(conn, args.text, args.limit)
    elif args.cmd == "get":
        cmd_get(conn, args.id)
    elif args.cmd == "neighbors":
        cmd_neighbors(conn, args.id)
    elif args.cmd == "claims":
        cmd_claims(conn)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

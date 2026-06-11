#!/usr/bin/env python3
"""Create ignored dogfood fixtures for native headroom evaluation."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

VALID_KINDS = {"json", "code", "log", "diff", "markdown", "plain_text"}
OUT_DIR = Path(".tmp/headroom/fixtures")


def usage() -> int:
    print(
        "usage: scripts/headroom_fixture.py NAME KIND INPUT_FILE\n\n"
        "KIND: json | code | log | diff | markdown | plain_text\n"
        "Writes .tmp/headroom/fixtures/<name>.json. Edit required_facts before using the fixture as evidence.",
        file=sys.stderr,
    )
    return 2


def slugify(name: str) -> str:
    slug = re.sub(r"[^A-Za-z0-9._-]+", "-", name.strip()).strip("-._")
    return slug or "dogfood-fixture"


def main(argv: list[str]) -> int:
    if len(argv) != 4 or argv[1] in {"-h", "--help"}:
        return usage()

    name = argv[1]
    kind = argv[2]
    input_path = Path(argv[3])

    if kind not in VALID_KINDS:
        print(f"invalid kind: {kind}", file=sys.stderr)
        return usage()
    if not input_path.exists():
        print(f"input file does not exist: {input_path}", file=sys.stderr)
        return 2
    if not input_path.is_file():
        print(f"input path is not a file: {input_path}", file=sys.stderr)
        return 2

    try:
        text = input_path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        print(f"input file is not valid UTF-8: {input_path}: {exc}", file=sys.stderr)
        return 2

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    output_path = OUT_DIR / f"{slugify(name)}.json"
    fixture = {
        "name": name,
        "class": "dogfood",
        "kind_hint": kind,
        "input": text,
        "required_facts": [],
        "min_savings_percent": 50,
        "expected_compressed": None,
    }
    output_path.write_text(json.dumps(fixture, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"wrote {output_path}")
    print("edit required_facts before treating this fixture as evidence")
    print("run: just headroom-eval --text --fixtures .tmp/headroom/fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

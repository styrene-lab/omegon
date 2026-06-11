#!/usr/bin/env python3
"""Generate ignored headroom dogfood fixtures from real local commands.

The script intentionally uses only Python stdlib and writes under .tmp/ so the
fixtures can be inspected locally without becoming source artifacts.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TMP = ROOT / ".tmp" / "headroom"
SAMPLES = TMP / "samples"
FIXTURES = TMP / "fixtures"


@dataclass(frozen=True)
class CommandFixture:
    name: str
    kind_hint: str
    command: list[str]
    min_savings_percent: int
    expected_compressed: bool | None
    max_output_bytes: int = 500_000


COMMANDS = [
    CommandFixture(
        name="headroom-tests",
        kind_hint="log",
        command=["cargo", "test", "-p", "omegon-headroom", "--", "--nocapture"],
        min_savings_percent=0,
        expected_compressed=False,
    ),
    CommandFixture(
        name="read-tests",
        kind_hint="log",
        command=["cargo", "test", "-p", "omegon", "read", "--", "--nocapture"],
        min_savings_percent=40,
        expected_compressed=True,
    ),
    CommandFixture(
        name="recent-headroom-work",
        kind_hint="diff",
        command=["git", "show", "--stat", "--patch", "HEAD~5..HEAD"],
        min_savings_percent=60,
        expected_compressed=True,
    ),
]

SIGNAL_SUBSTRINGS = (
    "headroom",
    "compression",
    "compress",
    "retrieve",
    "fixture",
    "fixtures",
    "compare",
    "save",
    "anchor",
    "anchors",
    "dogfood",
    "test result:",
    " passed;",
    " failed;",
    "error",
    "failed",
    "panic",
    "warning",
    "decision",
    "blocked",
    "critical",
)

PATH_LINE_RE = re.compile(r"\b[\w./-]+\.(?:rs|toml|md|json|py|just|lock):(\d+)\b")
RUST_TEST_RE = re.compile(r"^test\s+[\w:]+\s+\.\.\.\s+(?:ok|FAILED|ignored|measured)$")
RUST_FN_RE = re.compile(r"^[+\- ]*\s*(?:pub\s+)?fn\s+[A-Za-z0-9_]+\s*\(")
CLI_FLAG_RE = re.compile(r"--[A-Za-z0-9][A-Za-z0-9-]*")
HASH_RE = re.compile(r"\b[0-9a-f]{8,64}\b")


def run_command(spec: CommandFixture) -> tuple[str, int]:
    result = subprocess.run(
        spec.command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=300,
    )
    output = result.stdout
    if len(output.encode("utf-8")) > spec.max_output_bytes:
        encoded = output.encode("utf-8")[: spec.max_output_bytes]
        output = encoded.decode("utf-8", errors="ignore")
        output += f"\n[headroom-dogfood: output clipped to {spec.max_output_bytes} bytes]\n"
    return output, result.returncode


def extract_required_facts(text: str, limit: int = 24) -> list[str]:
    facts: list[str] = []
    seen: set[str] = set()

    def add(candidate: str) -> None:
        candidate = candidate.strip()
        if not candidate or len(candidate) < 4:
            return
        if len(candidate) > 180:
            candidate = candidate[:180]
        if candidate in seen:
            return
        seen.add(candidate)
        facts.append(candidate)

    lines = text.splitlines()

    # Highest value: explicit result summaries and failures.
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("@@"):
            continue
        lower = line.lower()
        if "test result:" in lower:
            add(line)
        elif any(word in lower for word in ("panic", "error", "failed", "failure")):
            add(line)

    # Preserve headroom-specific declarations/tool/CLI evidence without turning
    # arbitrary diff context into required facts.
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("@@"):
            continue
        lower = line.lower()
        if RUST_TEST_RE.match(stripped):
            add(line)
        if RUST_FN_RE.match(line):
            name = stripped.lstrip("+- ").strip()
            if any(token in name.lower() for token in ("headroom", "compress", "anchor", "fixture", "read")):
                add(name)
            continue
        if any(token in lower for token in ("headroom_", "headroom-", "omegon-headroom", "headroom:")):
            add(line)
        if "test(headroom):" in lower or "feat(headroom):" in lower or "fix(headroom):" in lower:
            add(line)

    # Pull stable path:line and CLI/hash fragments even if the full line is noisy.
    for line in lines:
        for match in PATH_LINE_RE.finditer(line):
            add(match.group(0))
        for match in CLI_FLAG_RE.finditer(line):
            add(match.group(0))
        for match in HASH_RE.finditer(line):
            add(match.group(0))

    # Keep deterministic first-N with preference already encoded by loop order.
    return facts[:limit]


def write_fixture(spec: CommandFixture, text: str, returncode: int) -> Path:
    sample_path = SAMPLES / f"{spec.name}.txt"
    fixture_path = FIXTURES / f"{spec.name}.json"
    sample_path.write_text(text, encoding="utf-8")
    facts = extract_required_facts(text)
    fixture = {
        "name": spec.name,
        "class": "dogfood",
        "kind_hint": spec.kind_hint,
        "input": text,
        "required_facts": facts,
        "min_savings_percent": spec.min_savings_percent,
        "expected_compressed": spec.expected_compressed,
        "metadata": {
            "command": spec.command,
            "returncode": returncode,
            "sample_path": str(sample_path.relative_to(ROOT)),
            "required_fact_count": len(facts),
            "required_facts_generated": True,
        },
    }
    fixture_path.write_text(json.dumps(fixture, indent=2) + "\n", encoding="utf-8")
    return fixture_path


def main() -> int:
    SAMPLES.mkdir(parents=True, exist_ok=True)
    FIXTURES.mkdir(parents=True, exist_ok=True)

    written: list[Path] = []
    failures: list[str] = []
    for spec in COMMANDS:
        print(f"running {' '.join(spec.command)}", file=sys.stderr)
        try:
            text, returncode = run_command(spec)
        except Exception as exc:  # noqa: BLE001 - CLI should report all command setup failures.
            failures.append(f"{spec.name}: {exc}")
            continue
        path = write_fixture(spec, text, returncode)
        written.append(path)
        print(f"wrote {path.relative_to(ROOT)}", file=sys.stderr)

    if failures:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        return 1

    print(json.dumps({"fixtures": [str(path.relative_to(ROOT)) for path in written]}, indent=2))
    print("run: just headroom-eval --text --fixtures .tmp/headroom/fixtures", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

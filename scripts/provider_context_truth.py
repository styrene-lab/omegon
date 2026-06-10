#!/usr/bin/env python3
"""Check provider-published context-window docs against Omegon's registry.

This is a lightweight drift sentinel, not a scraper that rewrites
`data/model-registry.json`. Provider docs are human-authored and change shape;
this script fetches canonical docs pages, normalizes their text, and verifies
that our key model-window claims have nearby upstream evidence.
"""

from __future__ import annotations

import argparse
import html
import json
import re
import sys
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

REPO_ROOT = Path(__file__).resolve().parents[1]
REGISTRY = REPO_ROOT / "data" / "model-registry.json"


@dataclass(frozen=True)
class ProviderDoc:
    id: str
    url: str


@dataclass(frozen=True)
class ContextExpectation:
    provider: str
    model: str
    expected_tokens: int
    doc_id: str
    model_aliases: tuple[str, ...]
    token_aliases: tuple[str, ...]


DOCS = {
    "anthropic-models": ProviderDoc(
        "anthropic-models",
        "https://docs.anthropic.com/en/docs/about-claude/models/overview",
    ),
    "openai-models": ProviderDoc(
        "openai-models",
        "https://platform.openai.com/docs/models",
    ),
    "google-gemini-models": ProviderDoc(
        "google-gemini-models",
        "https://ai.google.dev/gemini-api/docs/models",
    ),
}

# Keep this list intentionally small: defaults/frontier lines where overstating
# context windows would materially affect local context assembly.
EXPECTATIONS = (
    ContextExpectation(
        provider="anthropic",
        model="claude-sonnet-4-6",
        expected_tokens=1_000_000,
        doc_id="anthropic-models",
        model_aliases=("claude sonnet 4.6", "claude-sonnet-4-6", "sonnet 4.6"),
        token_aliases=("1m", "1 m", "1 million", "1,000,000"),
    ),
    ContextExpectation(
        provider="anthropic",
        model="claude-haiku-4-5-20251001",
        expected_tokens=200_000,
        doc_id="anthropic-models",
        model_aliases=("claude haiku 4.5", "claude-haiku-4-5", "haiku 4.5"),
        token_aliases=("200k", "200 k", "200,000"),
    ),
    ContextExpectation(
        provider="openai",
        model="gpt-5.5",
        expected_tokens=1_000_000,
        doc_id="openai-models",
        model_aliases=("gpt-5.5", "gpt 5.5"),
        token_aliases=("1m", "1 m", "1 million", "1,000,000"),
    ),
    ContextExpectation(
        provider="openai",
        model="gpt-5.4-mini",
        expected_tokens=400_000,
        doc_id="openai-models",
        model_aliases=("gpt-5.4-mini", "gpt 5.4 mini"),
        token_aliases=("400k", "400 k", "400,000"),
    ),
    ContextExpectation(
        provider="google",
        model="gemini-2.5-flash",
        expected_tokens=1_000_000,
        doc_id="google-gemini-models",
        model_aliases=("gemini 2.5 flash", "gemini-2.5-flash"),
        token_aliases=("1m", "1 m", "1 million", "1,000,000"),
    ),
)


def normalize(text: str) -> str:
    text = re.sub(r"<script[\s\S]*?</script>", " ", text, flags=re.I)
    text = re.sub(r"<style[\s\S]*?</style>", " ", text, flags=re.I)
    text = re.sub(r"<[^>]+>", " ", text)
    text = html.unescape(text)
    text = text.replace("\u00a0", " ")
    text = re.sub(r"\s+", " ", text)
    return text.lower()


def fetch(url: str, timeout: int) -> str:
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "omegon-provider-context-truth/1.0 (+https://github.com/styrene-lab/omegon)",
            "Accept": "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.8",
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:  # noqa: S310 - fixed public docs URLs
        return normalize(resp.read().decode("utf-8", errors="replace"))


def glob_match(pattern: str, value: str) -> bool:
    if pattern.endswith("*"):
        return value.startswith(pattern[:-1])
    return value == pattern


def registry_context(provider: str, model: str) -> int | None:
    data = json.loads(REGISTRY.read_text())
    for entry in data.get("models", []):
        if entry.get("provider") == provider and entry.get("id") == model:
            return int(entry["contextInput"])
    matches = [
        route
        for route in data.get("routes", [])
        if route.get("provider") == provider and glob_match(route.get("modelIdPattern", ""), model)
    ]
    if matches:
        best = max(matches, key=lambda route: len(route.get("modelIdPattern", "")))
        return int(best["contextCeiling"])
    return None


def positions(text: str, needle: str) -> list[int]:
    found: list[int] = []
    start = 0
    needle = needle.lower()
    while True:
        idx = text.find(needle, start)
        if idx < 0:
            return found
        found.append(idx)
        start = idx + max(1, len(needle))


def near_evidence(doc_text: str, model_aliases: Iterable[str], token_aliases: Iterable[str]) -> str | None:
    model_positions = [pos for alias in model_aliases for pos in positions(doc_text, alias)]
    token_positions = [pos for alias in token_aliases for pos in positions(doc_text, alias)]
    for model_pos in model_positions:
        for token_pos in token_positions:
            if abs(model_pos - token_pos) <= 5000:
                start = max(0, min(model_pos, token_pos) - 800)
                return doc_text[start : start + 1200]
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    parser.add_argument("--timeout", type=int, default=20)
    args = parser.parse_args()

    docs: dict[str, str] = {}
    failures: list[dict[str, object]] = []
    rows: list[dict[str, object]] = []

    for doc_id, doc in DOCS.items():
        try:
            docs[doc_id] = fetch(doc.url, args.timeout)
        except Exception as exc:  # noqa: BLE001 - this is a diagnostic script
            failures.append({"doc": doc_id, "url": doc.url, "error": str(exc)})

    for expectation in EXPECTATIONS:
        registry_tokens = registry_context(expectation.provider, expectation.model)
        registry_ok = registry_tokens == expectation.expected_tokens
        evidence = docs.get(expectation.doc_id)
        upstream_ok = bool(
            evidence
            and near_evidence(evidence, expectation.model_aliases, expectation.token_aliases)
        )
        row = {
            "provider": expectation.provider,
            "model": expectation.model,
            "expected_tokens": expectation.expected_tokens,
            "registry_tokens": registry_tokens,
            "registry_ok": registry_ok,
            "upstream_ok": upstream_ok,
            "doc": expectation.doc_id,
            "url": DOCS[expectation.doc_id].url,
        }
        rows.append(row)
        if not registry_ok or not upstream_ok:
            failures.append(row)

    if args.json:
        print(json.dumps({"ok": not failures, "checks": rows, "failures": failures}, indent=2))
    else:
        print("Provider context truth check")
        print("============================")
        for row in rows:
            status = "ok" if row["registry_ok"] and row["upstream_ok"] else "FAIL"
            print(
                f"{status:4} {row['provider']}:{row['model']} "
                f"registry={row['registry_tokens']} expected={row['expected_tokens']} "
                f"doc={row['doc']} upstream={'ok' if row['upstream_ok'] else 'missing'}"
            )
        if failures:
            print("\nFailures:", file=sys.stderr)
            print(json.dumps(failures, indent=2), file=sys.stderr)

    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())

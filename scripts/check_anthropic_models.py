#!/usr/bin/env python3
"""Cheap Anthropic model drift check.

Scrapes Anthropic's public models overview page for Claude API IDs and compares
those IDs with data/model-registry.json. This is intentionally lightweight: it
uses stdlib only, does not require credentials, and reports drift without trying
to infer all pricing/capability metadata.
"""

from __future__ import annotations

import json
import re
import sys
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
REGISTRY = REPO_ROOT / "data" / "model-registry.json"
OVERVIEW_URL = "https://platform.claude.com/docs/en/about-claude/models/overview"

MODEL_RE = re.compile(r"claude-(?:fable|mythos|opus|sonnet|haiku)-[a-z0-9-]+")
CURRENT_SECTION_START = "Claude Fable 5 and Claude Mythos 5"
CURRENT_SECTION_END = "Legacy models"


def fetch_text(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "omegon-upstream-check/1"})
    with urllib.request.urlopen(req, timeout=30) as resp:  # noqa: S310 - fixed HTTPS URL
        return resp.read().decode("utf-8", errors="replace")


def registry_anthropic_ids() -> set[str]:
    data = json.loads(REGISTRY.read_text())
    return {m["id"] for m in data["models"] if m.get("provider") == "anthropic"}


def current_models_section(text: str) -> str:
    start = text.find(CURRENT_SECTION_START)
    end = text.find(CURRENT_SECTION_END, start if start >= 0 else 0)
    if start >= 0 and end > start:
        return text[start:end]
    return text


def upstream_model_ids(text: str) -> set[str]:
    ids = set(MODEL_RE.findall(current_models_section(text)))
    # Drop Bedrock/Vertex decorated IDs and page slugs; keep Claude API IDs and aliases.
    return {
        mid
        for mid in ids
        if "-v1" not in mid
        and "-and-" not in mid
        and mid != "claude-haiku-4-5"  # documented alias; registry stores pinned ID
        and mid != "claude-mythos-preview"  # invitation-only preview, not a GA catalog target
        and not mid.endswith("-on")
        and not mid.endswith("-models")
    }


def main() -> int:
    try:
        text = fetch_text(OVERVIEW_URL)
    except Exception as exc:  # pragma: no cover - network dependent
        print(json.dumps({"error": f"failed to fetch {OVERVIEW_URL}: {exc}"}, indent=2))
        return 2

    upstream = upstream_model_ids(text)
    registry = registry_anthropic_ids()
    missing = sorted(upstream - registry)
    stale = sorted(registry - upstream)
    output = {
        "source": OVERVIEW_URL,
        "upstream_count": len(upstream),
        "registry_count": len(registry),
        "missing_from_registry": missing,
        "registry_only": stale,
    }
    print(json.dumps(output, indent=2))
    return 1 if missing else 0


if __name__ == "__main__":
    raise SystemExit(main())

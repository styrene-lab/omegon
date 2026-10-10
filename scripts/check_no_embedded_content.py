#!/usr/bin/env python3
"""Reject shipped content bodies or inventories embedded in production Rust."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RUST_ROOT = ROOT / "core" / "crates"
CONTENT_ROOTS = tuple(
    ROOT / directory
    for directory in ("skills", "prompts", "personas", "tones", "workflows", "catalog")
)
DATA_ROOT = ROOT / "data"
# The host core is a Rust constant in prompt.rs. Retired data bundles have no
# embedding exception; task-scoped references remain replaceable pack content.
INCLUDE = re.compile(r'include_(?:str|bytes)!\s*\(\s*"([^"]+)"')
BODY_FINGERPRINTS = (
    "You are a conversation summarizer. Produce a concise summary preserving:",
    "# Vox Communication Extension",
    "# Scry Image Generation Extension",
    "# Omegon Extension Authoring Reference",
    "# Tool Limitations",
    "# Omegon Capability Guidance",
)


def scan_source(path: Path, text: str) -> list[str]:
    findings: list[str] = []
    for match in INCLUDE.finditer(text):
        literal = match.group(1)
        target = (path.parent / literal).resolve()
        shipped_root = any(target.is_relative_to(root) for root in CONTENT_ROOTS)
        markdown_data = target.is_relative_to(DATA_ROOT) and target.suffix.lower() == ".md"
        if shipped_root or markdown_data:
            findings.append(f"embeds shipped content path {literal}")
    for fingerprint in BODY_FINGERPRINTS:
        if fingerprint in text:
            findings.append(f"contains shipped content body {fingerprint!r}")
    return findings


def main() -> int:
    findings: list[str] = []
    for path in sorted(RUST_ROOT.glob("*/src/**/*.rs")):
        text = path.read_text()
        findings.extend(
            f"{path.relative_to(ROOT)} {finding}" for finding in scan_source(path, text)
        )
    if findings:
        raise SystemExit("\n".join(findings))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

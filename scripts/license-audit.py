#!/usr/bin/env python3
"""
License audit for Omegon.

Reads `cargo license --json` output and checks for any copyleft or
non-permissive packages not listed in the allowlist. Fails with a
non-zero exit code if new packages are found, so CI catches drift
before release.

Usage:
    cd core && cargo license --json | python3 ../scripts/license-audit.py

Or with a pre-generated file:
    python3 scripts/license-audit.py --input licenses.json
"""

import json
import sys
import argparse
import textwrap

# ── Allowlist ──────────────────────────────────────────────────────────────
# Packages whose non-permissive license is already documented in
# THIRD_PARTY_NOTICES.md. Adding a package here acknowledges it.
# Format: { "name": "version" }  (version is informational only)
ACKNOWLEDGED = {
    # MPL-2.0 — documented in THIRD_PARTY_NOTICES.md
    "colored":          "3.1.1",
    "option-ext":       "0.2.0",
    "uluru":            "3.1.0",
    "cssparser":        "0.34.0",
    "cssparser-macros": "0.6.1",
    "dtoa-short":       "0.3.5",
    "selectors":        "0.26.0",
    # Apache-2.0 OR GPL-2.0 — documented in THIRD_PARTY_NOTICES.md;
    # Omegon selects Apache-2.0, not GPL-2.0.
    "self_cell":        "1.2.2",
}

# ── Own crates ─────────────────────────────────────────────────────────────
# First-party crates under BSL-1.1. Not third-party; skip in audit. Keep this
# explicit: a broad prefix could accidentally exempt an external package.
FIRST_PARTY_PACKAGES = {
    "omegon",
    "omegon-codescan",
    "omegon-git",
    "omegon-memory",
    "omegon-opsx",
    "omegon-rbac",
    "omegon-secrets",
    "omegon-skills",
    "omegon-traits",
    "omegon-web",
    "styrene-work-model",
    "styrene-work-runtime",
}

# ── Licenses we flag ──────────────────────────────────────────────────────
# Permissive licenses are fine. These need acknowledgement.
FLAGGED_KEYWORDS = [
    "GPL",       # GPL-2.0, GPL-3.0, AGPL-3.0
    "LGPL",      # LGPL-2.0, LGPL-2.1, LGPL-3.0
    "MPL",       # MPL-1.0, MPL-2.0
    "EUPL",      # European Union Public Licence
    "CDDL",      # Common Development and Distribution License
    "SSPL",      # Server Side Public License
    "BUSL",      # Business Source License (third-party, not our own)
    "Elastic",   # Elastic License 2.0
]

# These license strings are fine even if they contain a flagged keyword
SAFE_PATTERNS = [
    "Apache-2.0 OR LGPL",   # r-efi — we elect Apache-2.0 or MIT
    "LGPL-2.1-or-later OR MIT",
]


def is_flagged(license_str: str) -> bool:
    """Return True if this license string needs human review."""
    if not license_str:
        return False
    # Check safe patterns first
    for safe in SAFE_PATTERNS:
        if safe in license_str:
            return False
    return any(kw in license_str for kw in FLAGGED_KEYWORDS)


def main():
    parser = argparse.ArgumentParser(description="Omegon license audit")
    parser.add_argument(
        "--input", "-i",
        help="Path to cargo license --json output (default: stdin)",
        default=None,
    )
    parser.add_argument(
        "--summary",
        help="Print a license summary in addition to audit results",
        action="store_true",
    )
    args = parser.parse_args()

    if args.input:
        with open(args.input) as f:
            packages = json.load(f)
    else:
        if sys.stdin.isatty():
            print("ERROR: No input. Run: cargo license --json | python3 scripts/license-audit.py")
            sys.exit(1)
        packages = json.load(sys.stdin)

    # ── First-party filter ────────────────────────────────────────────────
    third_party = [
        package for package in packages
        if package["name"] not in FIRST_PARTY_PACKAGES
    ]

    # ── Find flagged packages ─────────────────────────────────────────────
    flagged = []
    acknowledged_and_present = []
    for pkg in third_party:
        lic = pkg.get("license") or ""
        name = pkg["name"]
        if is_flagged(lic):
            if name in ACKNOWLEDGED:
                acknowledged_and_present.append((name, pkg.get("version", "?"), lic))
            else:
                flagged.append((name, pkg.get("version", "?"), lic, pkg.get("repository", "N/A")))

    # ── Summary ──────────────────────────────────────────────────────────
    if args.summary:
        import collections
        by_lic = collections.defaultdict(int)
        for p in third_party:
            lic = p.get("license") or "UNKNOWN"
            if "Apache-2.0 OR MIT" in lic:
                lic = "Apache-2.0 OR MIT"
            elif "MIT" in lic and "Apache" not in lic and "GPL" not in lic:
                lic = "MIT"
            by_lic[lic] += 1
        print("=== License Summary ===")
        for lic, count in sorted(by_lic.items(), key=lambda x: -x[1]):
            print(f"  {count:4d}  {lic}")
        print(f"  ────  Total: {len(third_party)} third-party packages")
        print()

    # ── Acknowledged ──────────────────────────────────────────────────────
    if acknowledged_and_present:
        print(f"✓ {len(acknowledged_and_present)} acknowledged copyleft package(s) present (documented in THIRD_PARTY_NOTICES.md):")
        for name, version, lic in acknowledged_and_present:
            print(f"    {name} {version} — {lic}")
        print()

    # ── New unacknowledged packages ───────────────────────────────────────
    if not flagged:
        print("✓ License audit passed — no unacknowledged copyleft dependencies.")
        return 0

    print("✗ LICENSE AUDIT FAILED")
    print()
    print(textwrap.dedent("""
        The following package(s) use a non-permissive license and are not
        listed in THIRD_PARTY_NOTICES.md or the allowlist in this script.

        For each package you must:
          1. Verify the license is compatible with BSL-1.1 distribution
          2. Add attribution to THIRD_PARTY_NOTICES.md (include license text
             if required by that license — MPL-2.0 requires full text)
          3. Add the package name to ACKNOWLEDGED in scripts/license-audit.py
    """).strip())
    print()
    for name, version, lic, repo in flagged:
        print(f"  PACKAGE:  {name} {version}")
        print(f"  LICENSE:  {lic}")
        print(f"  SOURCE:   {repo}")
        print()
    return 1


if __name__ == "__main__":
    sys.exit(main())

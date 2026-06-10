#!/usr/bin/env python3
"""Unit tests for affected_crates.py."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts import affected_crates as ac  # noqa: E402


class AffectedCratesTests(unittest.TestCase):
    def test_docs_only_accepts_markdown_docs(self) -> None:
        self.assertTrue(ac.is_docs_only(["docs/devops.md", "CHANGELOG.md"]))
        self.assertTrue(ac.is_docs_only(["openspec/changes/foo/proposal.md"]))

    def test_docs_only_rejects_executable_content_under_content_dirs(self) -> None:
        self.assertFalse(ac.is_docs_only(["site/scripts/build.mjs"]))
        self.assertFalse(ac.is_docs_only(["skills/example/SKILL.md", "skills/example/tool.py"]))
        self.assertFalse(ac.is_docs_only(["ai/lifecycle/state.json"]))

    def test_reverse_dependency_closure_includes_dependents(self) -> None:
        packages = {
            "omegon": ac.Package(
                "omegon",
                Path("core/crates/omegon"),
                ("omegon-web", "omegon-traits"),
            ),
            "omegon-web": ac.Package("omegon-web", Path("core/crates/omegon-web"), ()),
            "omegon-traits": ac.Package(
                "omegon-traits",
                Path("core/crates/omegon-traits"),
                (),
            ),
            "omegon-memory": ac.Package(
                "omegon-memory",
                Path("core/crates/omegon-memory"),
                ("omegon-traits",),
            ),
        }
        reverse = ac.reverse_dependencies(packages)

        self.assertEqual(ac.closure({"omegon-web"}, reverse), {"omegon-web", "omegon"})
        self.assertEqual(
            ac.closure({"omegon-traits"}, reverse),
            {"omegon-traits", "omegon", "omegon-memory"},
        )


if __name__ == "__main__":
    unittest.main()

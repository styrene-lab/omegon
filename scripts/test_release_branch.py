#!/usr/bin/env python3
"""Regression tests for stable-release/trunk version invariants."""

from __future__ import annotations

import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("release_branch.py")
SPEC = importlib.util.spec_from_file_location("release_branch", MODULE_PATH)
assert SPEC and SPEC.loader
release_branch = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release_branch)


class PublishInvariantTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.repo = Path(self.temp.name)
        subprocess.run(["git", "init", "-q", "--initial-branch=main"], cwd=self.repo, check=True)
        subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=self.repo, check=True)
        subprocess.run(["git", "config", "user.name", "Test"], cwd=self.repo, check=True)
        (self.repo / "Cargo.toml").write_text('[workspace.package]\nversion = "0.28.7"\n')
        subprocess.run(["git", "add", "Cargo.toml"], cwd=self.repo, check=True)
        subprocess.run(["git", "commit", "-qm", "initial"], cwd=self.repo, check=True)
        subprocess.run(["git", "remote", "add", "origin", str(self.repo)], cwd=self.repo, check=True)
        subprocess.run(["git", "fetch", "-q", "origin", "main"], cwd=self.repo, check=True)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_rejects_main_older_than_release(self) -> None:
        with self.assertRaisesRegex(release_branch.ReleaseBranchError, "0.28.7 is behind.*0.28.8"):
            release_branch.assert_main_version_not_behind(self.repo, "0.28.8")

    def test_accepts_main_at_release_version(self) -> None:
        release_branch.assert_main_version_not_behind(self.repo, "0.28.7")

    def test_verify_publish_accepts_detached_release_tag_checkout(self) -> None:
        subprocess.run(["git", "checkout", "--detach", "-q", "HEAD"], cwd=self.repo, check=True)

        release_branch.verify_publish_invariant(self.repo)

    def test_accepts_main_newer_than_release(self) -> None:
        release_branch.assert_main_version_not_behind(self.repo, "0.28.6")


if __name__ == "__main__":
    unittest.main()

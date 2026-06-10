#!/usr/bin/env python3
"""Unit tests for test_profile.py."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts import test_profile as tp  # noqa: E402


class TestProfileTests(unittest.TestCase):
    def test_profile_file_counts_plain_and_tokio_tests(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "lib.rs"
            path.write_text(
                "#[test]\n"
                "fn plain() {}\n\n"
                "#[tokio::test]\n"
                "async fn async_test() {}\n"
            )

            profile = tp.profile_file(path)

        self.assertEqual(profile.tests, 2)
        self.assertEqual(profile.lines, 6)

    def test_per_crate_file_limit_is_configurable(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            crate = Path(tmp) / "demo"
            src = crate / "src"
            src.mkdir(parents=True)
            (crate / "Cargo.toml").write_text("[package]\nname='demo'\nversion='0.0.0'\n")
            for idx in range(5):
                (src / f"file_{idx}.rs").write_text("#[test]\nfn t() {}\n")

            profile = tp.profile_crate(crate, large_threshold=1, per_crate_files=2)

        self.assertEqual(profile.tests, 5)
        self.assertEqual(len(profile.large_files), 2)


if __name__ == "__main__":
    unittest.main()

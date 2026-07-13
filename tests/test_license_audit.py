from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "license-audit.py"


def load_module():
    spec = importlib.util.spec_from_file_location("license_audit", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class LicenseAuditTests(unittest.TestCase):
    def run_audit(self, packages: list[dict[str, str]]) -> subprocess.CompletedProcess[str]:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json") as fixture:
            json.dump(packages, fixture)
            fixture.flush()
            return subprocess.run(
                [sys.executable, str(SCRIPT), "--input", fixture.name, "--summary"],
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )

    def test_styrene_workspace_crates_are_first_party(self) -> None:
        result = self.run_audit([
            {"name": "styrene-work-model", "version": "0.28.0", "license": "BUSL-1.1"},
            {"name": "styrene-work-runtime", "version": "0.28.0", "license": "BUSL-1.1"},
        ])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Total: 0 third-party packages", result.stdout)

    def test_external_omegon_prefixed_package_is_not_implicitly_trusted(self) -> None:
        result = self.run_audit([
            {"name": "omegon-unrelated", "version": "1.0.0", "license": "BUSL-1.1"},
        ])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("omegon-unrelated", result.stdout)

    def test_first_party_inventory_matches_workspace_packages(self) -> None:
        module = load_module()
        metadata = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--quiet"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=True,
        )
        workspace_packages = {package["name"] for package in json.loads(metadata.stdout)["packages"]}
        self.assertEqual(module.FIRST_PARTY_PACKAGES, workspace_packages)


if __name__ == "__main__":
    unittest.main()

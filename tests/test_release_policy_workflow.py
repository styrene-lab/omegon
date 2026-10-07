import importlib.util
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[1]
REQUIRED_MODULES = {
    "tests.test_composition_release_gates",
    "tests.test_content_pack_packaging",
    "tests.test_distribution_policy",
    "tests.test_distribution_runtime_smoke",
    "tests.test_oci_distribution_policy",
    "tests.test_optional_domain_isolation",
    "tests.test_release_closeout",
    "tests.test_release_manifest",
    "tests.test_release_preflight",
    "tests.test_release_status",
    "tests.test_validate_companion",
    "tests.test_verify_homebrew_formula",
}


class ReleasePolicyWorkflowTests(unittest.TestCase):
    def test_release_policy_runner_covers_maintained_modules(self) -> None:
        path = ROOT / "scripts/test_release_policy.py"
        spec = importlib.util.spec_from_file_location("test_release_policy", path)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
        self.assertTrue(REQUIRED_MODULES.issubset(set(module.TEST_MODULES)))

    def test_pull_request_workflow_runs_the_release_policy_runner(self) -> None:
        workflow = (ROOT / ".github/workflows/test.yml").read_text()
        self.assertIn("python-release-policy:", workflow)
        self.assertIn("python3 scripts/test_release_policy.py", workflow)

    def test_rust_test_recipes_select_the_canonical_glyph_set(self) -> None:
        recipes = (ROOT / "Justfile").read_text().splitlines()
        cargo_test_lines = [line for line in recipes if "{{cargo}} test" in line]
        self.assertTrue(cargo_test_lines)
        self.assertEqual(
            [],
            [line for line in cargo_test_lines if "OMEGON_NERD_FONT=1" not in line],
        )

    def test_rust_build_resolves_baseline_and_keeps_stable_test_coverage(self) -> None:
        jobs = yaml.safe_load((ROOT / ".github/workflows/test.yml").read_text())["jobs"]
        steps = jobs["rust-build"]["steps"]
        resolver = next(step for step in steps if step.get("id") == "rust-baseline")
        install = next(step for step in steps if step.get("uses", "").startswith("dtolnay/"))
        self.assertLess(steps.index(resolver), steps.index(install))
        self.assertEqual(install["with"]["toolchain"], "${{ steps.rust-baseline.outputs.version }}")
        self.assertIn("clippy", install["with"]["components"])
        self.assertTrue(any("-- -D warnings" in step.get("run", "") for step in steps))
        for name in ("rust-unit", "rust-integration"):
            self.assertTrue(any(
                step.get("uses") == "dtolnay/rust-toolchain@stable"
                for step in jobs[name]["steps"]
            ))

        # Exercise the actual CI shell: propagate the evaluated version, and
        # fail closed if Nix cannot resolve the checked-in lockfile.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            nix = root / "nix"
            nix.write_text(
                '#!/bin/sh\n'
                'test "$*" = "eval --raw --no-update-lock-file '
                '.#packages.x86_64-linux.rust-toolchain.version" || exit 2\n'
                'if [ "$FAIL_NIX" = 1 ]; then exit 42; fi\n'
                'printf 1.234.5\n'
            )
            nix.chmod(0o755)
            output = root / "output"
            env = dict(os.environ, PATH=f"{root}{os.pathsep}{os.environ['PATH']}",
                       GITHUB_OUTPUT=str(output), FAIL_NIX="0")
            result = subprocess.run(["bash", "-e", "-c", resolver["run"]],
                                    env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(output.read_text(), "version=1.234.5\n")
            output.unlink()
            env["FAIL_NIX"] = "1"
            result = subprocess.run(["bash", "-e", "-c", resolver["run"]],
                                    env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 42, result.stderr)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()

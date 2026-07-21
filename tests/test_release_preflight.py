import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "release_preflight.py"


class ReleasePreflightTests(unittest.TestCase):
    def write_repo_fixture(
        self,
        root: Path,
        *,
        version: str,
        install_placeholder: bool = True,
        manifest_wired: bool = True,
        workspace_role: str | None = "release",
    ) -> None:
        (root / "site" / "src" / "pages" / "docs").mkdir(parents=True)
        (root / ".github" / "workflows").mkdir(parents=True)
        if workspace_role is not None:
            (root / ".omegon" / "runtime").mkdir(parents=True, exist_ok=True)
            (root / ".omegon" / "runtime" / "workspace.json").write_text(
                '{"role": "%s"}\n' % workspace_role
            )

        (root / "Cargo.toml").write_text(
            f'[workspace.package]\nversion = "{version}"\n'
        )
        (root / "CHANGELOG.md").write_text(
            f'# Changelog\n\n## [{version}] - 2026-07-12\n'
        )

        install_lines = [
            "VERSION=0.28.0 curl -fsSL https://omegon.styrene.io/install.sh | sh\n",
        ]
        if install_placeholder:
            install_lines.append(
                "# Replace 0.28.0 with the release you actually want\n"
            )
            install_lines.append("# Replace 0.28.0 with the release you downloaded\n")
        (root / "site" / "src" / "pages" / "docs" / "install.astro").write_text(
            "".join(install_lines)
        )

        manifest_line = "release-manifest.json\n" if manifest_wired else "checksums.sha256\n"
        (root / ".github" / "workflows" / "release.yml").write_text(manifest_line)
        (root / ".github" / "workflows" / "homebrew.yml").write_text(manifest_line)

    def init_git_repo(self, root: Path, *, branch: str = "release/0.28") -> None:
        subprocess.run(
            ["git", "init", "-b", branch], cwd=root, check=True, capture_output=True
        )
        subprocess.run(["git", "add", "."], cwd=root, check=True, capture_output=True)
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "init",
            ],
            cwd=root,
            check=True,
            capture_output=True,
        )

    def run_script(self, repo_root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                "python3",
                str(SCRIPT),
                "--repo-root",
                str(repo_root),
                "--skip-release-gap-check",
            ],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_preflight_passes_for_clean_stable_release_repo(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.28.0")
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("0.28.0", result.stdout)

    def test_preflight_fails_when_changelog_missing_target_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.28.0")
            (repo_root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.27.7] - 2026-07-01\n"
            )
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("CHANGELOG.md is missing section [0.28.0]", result.stderr)

    def test_preflight_fails_when_install_doc_is_not_placeholder_based(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(
                repo_root, version="0.28.0", install_placeholder=False
            )
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "install.astro versioned examples are not marked as placeholders",
                result.stderr,
            )

    def test_preflight_fails_when_manifest_wiring_missing(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(
                repo_root, version="0.28.0", manifest_wired=False
            )
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release-manifest.json", result.stderr)

    def test_preflight_fails_when_workspace_role_is_not_release(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(
                repo_root, version="0.28.0", workspace_role="feature"
            )
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("workspace role must be 'release'", result.stderr)

    def test_preflight_fails_when_release_branch_does_not_match_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.28.0")
            self.init_git_repo(repo_root, branch="release/0.27")

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "release branch release/0.27 does not match workspace release line 0.28.0",
                result.stderr,
            )


class ReleaseRecipeTests(unittest.TestCase):
    def release_block(self) -> str:
        justfile = (ROOT / "justfile").read_text()
        marker = "# Cut a stable release: test, commit milestone state if needed, tag, build.\nrelease:\n"
        start = justfile.index(marker) + len(marker) - len("release:\n")
        end = justfile.index("# Sign the local macOS validation binary", start)
        return justfile[start:end]

    def publish_block(self) -> str:
        justfile = (ROOT / "justfile").read_text()
        start = justfile.index("# Publish: push refs, trigger CI release/site workflows")
        end = justfile.index("\nsmoke:\n", start)
        return justfile[start:end]

    def test_publish_requires_main_version_before_push(self) -> None:
        block = self.publish_block()
        self.assertIn("python3 scripts/release_branch.py verify-publish", block)
        self.assertLess(
            block.index("python3 scripts/release_branch.py verify-publish"),
            block.index('git push origin "$BRANCH" "$TAG"'),
        )

    def test_release_recipe_runs_preflight_before_mutation(self) -> None:
        block = self.release_block()
        self.assertIn("just preflight", block)
        self.assertIn('./scripts/milestone-update.sh release "$NEW_VERSION"', block)
        self.assertLess(
            block.index("just preflight"),
            block.index('./scripts/milestone-update.sh release "$NEW_VERSION"'),
        )

    def test_release_recipe_requires_stable_semver(self) -> None:
        block = self.release_block()
        self.assertIn("Release version must be stable semver", block)
        self.assertIn("NEW_VERSION=\"$CURRENT\"", block)

    def test_release_recipe_uses_clippy_warning_gate_without_rebuilding_dependencies(self) -> None:
        block = self.release_block()
        self.assertIn("{{cargo}} clippy -p omegon --all-targets -- -D warnings", block)
        self.assertNotIn('RUSTFLAGS="-D warnings"', block)

    def test_release_recipe_builds_before_publish_instruction(self) -> None:
        block = self.release_block()
        self.assertIn("{{cargo}} build --release -p omegon", block)
        self.assertIn("just publish", block)
        self.assertLess(
            block.index("{{cargo}} build --release -p omegon"),
            block.index("just publish"),
        )

    def test_release_recipe_does_not_publish_inline(self) -> None:
        block = self.release_block()
        self.assertNotIn("gh release create", block)
        self.assertNotIn("\n    git push origin", block)
        self.assertIn("Stable release committed and tagged", block)


if __name__ == "__main__":
    unittest.main()

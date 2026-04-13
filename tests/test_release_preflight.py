import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "release_preflight.py"


class ReleasePreflightTests(unittest.TestCase):
    def write_repo_fixture(self, root: Path, *, version: str, install_placeholder: bool = True, manifest_wired: bool = True, workspace_role: str | None = "release") -> None:
        (root / "core").mkdir(parents=True)
        (root / "site" / "src" / "pages" / "docs").mkdir(parents=True)
        (root / ".github" / "workflows").mkdir(parents=True)
        if workspace_role is not None:
            (root / ".omegon" / "runtime").mkdir(parents=True, exist_ok=True)
            (root / ".omegon" / "runtime" / "workspace.json").write_text(
                '{"role": "%s"}\n' % workspace_role
            )

        (root / "core" / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{version}"\n')
        stable = version.split("-rc.", 1)[0] if "-rc." in version else version
        (root / "CHANGELOG.md").write_text(f'# Changelog\n\n## [{stable}] - 2026-04-04\n')

        install_lines = [
            "VERSION=0.15.7 curl -fsSL https://omegon.styrene.dev/install.sh | sh\n",
        ]
        if install_placeholder:
            install_lines.append("# Replace 0.15.7 with the release you actually want\n")
            install_lines.append("# Replace 0.15.7 with the release you downloaded\n")
        (root / "site" / "src" / "pages" / "docs" / "install.astro").write_text("".join(install_lines))

        manifest_line = "release-manifest.json\n" if manifest_wired else "checksums.sha256\n"
        (root / ".github" / "workflows" / "release.yml").write_text(manifest_line)
        (root / ".github" / "workflows" / "homebrew.yml").write_text(manifest_line)

    def init_git_repo(self, root: Path) -> None:
        subprocess.run(["git", "init", "-b", "main"], cwd=root, check=True, capture_output=True)
        subprocess.run(["git", "add", "."], cwd=root, check=True, capture_output=True)
        subprocess.run(
            ["git", "-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-m", "init"],
            cwd=root,
            check=True,
            capture_output=True,
        )

    def run_script(self, repo_root: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["python3", str(SCRIPT), "--repo-root", str(repo_root)],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_preflight_passes_for_clean_rc_repo(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.15.9-rc.14")
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("0.15.9", result.stdout)

    def test_preflight_fails_when_changelog_missing_target_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.15.9-rc.14")
            (repo_root / "CHANGELOG.md").write_text("# Changelog\n\n## [0.15.8] - 2026-04-04\n")
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("CHANGELOG.md is missing section [0.15.9]", result.stderr)

    def test_preflight_fails_when_install_doc_is_not_placeholder_based(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.15.9-rc.14", install_placeholder=False)
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("install.astro versioned examples are not marked as placeholders", result.stderr)

    def test_preflight_fails_when_manifest_wiring_missing(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.15.9-rc.14", manifest_wired=False)
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release-manifest.json", result.stderr)
    def test_preflight_fails_when_workspace_role_is_not_release(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir)
            self.write_repo_fixture(repo_root, version="0.15.9-rc.14", workspace_role="feature")
            self.init_git_repo(repo_root)

            result = self.run_script(repo_root)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("workspace role must be 'release'", result.stderr)


class ReleaseRecipeTests(unittest.TestCase):
    def test_rc_recipe_relinks_local_binary_after_build(self) -> None:
        justfile = (ROOT / "justfile").read_text()
        rc_start = justfile.index("rc:\n")
        preflight_start = justfile.index("# Release preflight:")
        rc_block = justfile[rc_start:preflight_start]

        self.assertIn('echo "Linking freshly built RC into PATH..."', rc_block)
        self.assertIn("just link", rc_block)
        self.assertLess(
            rc_block.index('echo "Linking freshly built RC into PATH..."'),
            rc_block.index('echo "Pushing rc tag..."'),
            "rc recipe should relink the local binary before pushing/tag completion output",
        )

    def test_rc_recipe_self_sets_release_workspace_role_before_preflight(self) -> None:
        justfile = (ROOT / "justfile").read_text()
        rc_start = justfile.index("rc:\n")
        preflight_start = justfile.index("# Release preflight:")
        rc_block = justfile[rc_start:preflight_start]

        repair_line = 'python3 scripts/release_preflight.py --ensure-release-workspace-role --repo-root .'
        preflight_line = 'python3 scripts/release_preflight.py\n'
        self.assertIn(repair_line, rc_block)
        self.assertIn(preflight_line, rc_block)
        self.assertLess(
            rc_block.index(repair_line),
            rc_block.rindex(preflight_line),
            "rc recipe should repair workspace role before preflight",
        )

    def test_rc_recipe_publishes_github_prerelease_before_success(self) -> None:
        justfile = (ROOT / "justfile").read_text()
        rc_start = justfile.index("rc:\n")
        preflight_start = justfile.index("# Release preflight:")
        rc_block = justfile[rc_start:preflight_start]

        self.assertIn('Waiting for release artifacts...', rc_block)
        self.assertIn('release-manifest.json', rc_block)
        self.assertIn('gh release edit "v${NEW_VERSION}" --draft=false --prerelease', rc_block)
        self.assertLess(
            rc_block.index('Waiting for release artifacts...'),
            rc_block.index('gh release edit "v${NEW_VERSION}" --draft=false --prerelease'),
            "rc recipe should wait for release artifacts before publishing",
        )
        self.assertLess(
            rc_block.index('gh release edit "v${NEW_VERSION}" --draft=false --prerelease'),
            rc_block.index('echo "✓ ${NEW_VERSION} — preflighted, committed, tagged, built, pushed, assets uploaded, published."'),
            "rc recipe should publish the prerelease before declaring success",
        )


if __name__ == "__main__":
    unittest.main()

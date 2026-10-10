import importlib.util
import json
import subprocess
import tarfile
import tempfile
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def load_release_manifest():
    spec = importlib.util.spec_from_file_location("release_manifest", ROOT / "scripts/release_manifest.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def load_content_guard():
    spec = importlib.util.spec_from_file_location(
        "check_no_embedded_content", ROOT / "scripts/check_no_embedded_content.py"
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class ContentPackPackagingTests(unittest.TestCase):
    def test_manifest_is_current_and_rust_has_no_embedded_shipped_content(self) -> None:
        for command in (
            ["python3", "scripts/content_pack_manifest.py", "--check"],
            ["python3", "scripts/check_no_embedded_content.py"],
        ):
            result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_source_guard_resolves_alternate_paths_and_rejects_literal_bodies(self) -> None:
        guard = load_content_guard()
        source = ROOT / "core/crates/omegon/src/fake.rs"
        findings = guard.scan_source(
            source,
            'const A: &str = include_str!("../../../../data/tool-limitations.md");\n'
            'const B: &str = "# Vox Communication Extension";',
        )
        self.assertEqual(len(findings), 2)
        disguised = guard.scan_source(
            source,
            'const A: &str = include_bytes!("../../../../prompts/../prompts/init.md");',
        )
        self.assertEqual(len(disguised), 1)
        lex_from_unapproved_owner = guard.scan_source(
            source,
            'const LEX: &str = include_str!("../../../../data/lex-imperialis.md");',
        )
        self.assertEqual(len(lex_from_unapproved_owner), 1)
        self.assertEqual(len(guard.scan_source(
            ROOT / "core/crates/omegon/src/prompt.rs",
            'const LEX: &str = include_str!("../../../../data/lex-imperialis.md");',
        )), 1, "retired Lex must not regain a host embedding exception")

        lex = (ROOT / "data/lex-imperialis.md").read_text()
        self.assertTrue(all(f"## {number}." in lex for number in ("I", "II", "III", "IV", "V", "VI")))
        self.assertNotIn("## VII. Capabilities", lex)

    def test_release_archive_contains_exact_pack_and_is_manifest_compatible(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / "bin"
            binaries.mkdir()
            for name in ("omegon", "omegon-maintain"):
                (binaries / name).write_bytes(name.encode())
            archive = root / "omegon-1.2.3-x86_64-unknown-linux-gnu.tar.gz"
            subprocess.run(
                [
                    "python3",
                    "scripts/package_release.py",
                    "--binary-dir",
                    str(binaries),
                    "--output",
                    str(archive),
                    "--without-codescan",
                ],
                cwd=ROOT,
                check=True,
            )
            with tarfile.open(archive, "r:gz") as package:
                members = package.getmembers()
                self.assertTrue(all(member.isfile() for member in members))
                names = {member.name for member in members}
                pack_manifest = "share/omegon/content-packs/omegon-shipped/content-pack.toml"
                inventory = tomllib.loads((ROOT / "content-pack.toml").read_text())["assets"]
                expected_names = {"omegon", "omegon-maintain", pack_manifest}
                expected_names.update({
                    "omegon.composition-lock.json",
                    "omegon-maintain.composition-lock.json",
                })
                expected_names.update(
                    f"share/omegon/content-packs/omegon-shipped/{asset['path']}"
                    for asset in inventory
                )
                self.assertEqual(names, expected_names)
                self.assertIn("share/omegon/content-packs/omegon-shipped/prompts/init.md", names)
                self.assertIn("share/omegon/content-packs/omegon-shipped/prompts/session-compaction.md", names)
                self.assertIn("share/omegon/content-packs/omegon-shipped/skills/scry/references/usage.md", names)
                self.assertIn("share/omegon/content-packs/omegon-shipped/skills/extension-authoring/references/authoring.md", names)
                self.assertFalse(any("/data/" in name for name in names), "retired automatic bundles are not prompt templates")
                self.assertIn("share/omegon/content-packs/omegon-shipped/skills/rust/SKILL.md", names)
                self.assertIn("share/omegon/content-packs/omegon-shipped/catalog/styrene.coding-agent/agent.toml", names)
                resident = json.load(package.extractfile("omegon.composition-lock.json"))
                self.assertEqual(resident["target"], "x86_64-unknown-linux-gnu")
                self.assertEqual(resident["signing_identity"]["verification"], "required")
                identities = {entry["identity"] for entry in resident["contributions"]}
                self.assertNotIn("feature:shipped-content", identities)
                self.assertTrue(
                    all(entry["artifact_path"] == "omegon" for entry in resident["contributions"])
                )
                self.assertEqual(
                    resident["signing_identity"]["workflow_identity"],
                    "https://github.com/styrene-lab/omegon/.github/workflows/release.yml@refs/tags/v1.2.3",
                )
            release_manifest = load_release_manifest()
            manifest = release_manifest.build_package_manifest(
                archive=archive,
                tag="v1.2.3",
                target="x86_64-unknown-linux-gnu",
                repo="styrene-lab/omegon",
                commit="a" * 40,
            )
            self.assertTrue(any(member["path"].endswith("content-pack.toml") for member in manifest["members"]))

    def test_release_archive_can_embed_codescan_extension(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / "bin"
            binaries.mkdir()
            for name in ("omegon", "omegon-maintain"):
                (binaries / name).write_bytes(name.encode())
            codescan = root / "omegon-codescan"
            codescan.write_bytes(b"codescan")
            manifest = root / "manifest.toml"
            manifest.write_text('[extension]\nname = "omegon-codescan"\n')
            archive = root / "omegon-1.2.3-x86_64-unknown-linux-gnu.tar.gz"

            subprocess.run(
                [
                    "python3",
                    "scripts/package_release.py",
                    "--binary-dir",
                    str(binaries),
                    "--output",
                    str(archive),
                    "--codescan-binary",
                    str(codescan),
                    "--codescan-manifest",
                    str(manifest),
                ],
                cwd=ROOT,
                check=True,
            )

            with tarfile.open(archive, "r:gz") as package:
                names = {member.name for member in package.getmembers()}
            prefix = "share/omegon/extensions/omegon-codescan"
            self.assertIn(f"{prefix}/manifest.toml", names)
            self.assertIn(f"{prefix}/target/release/omegon-codescan", names)
            release_manifest = load_release_manifest()
            manifest = release_manifest.build_package_manifest(
                archive=archive,
                tag="v1.2.3",
                target="x86_64-unknown-linux-gnu",
                repo="styrene-lab/omegon",
                commit="a" * 40,
            )
            self.assertTrue(
                any(
                    member["path"] == f"{prefix}/target/release/omegon-codescan"
                    for member in manifest["members"]
                )
            )

    def test_source_and_link_layouts_cover_exact_manifest_inventory(self) -> None:
        inventory = tomllib.loads((ROOT / "content-pack.toml").read_text())["assets"]
        expected = {asset["path"] for asset in inventory}
        self.assertTrue(all((ROOT / path).is_file() for path in expected))
        with tempfile.TemporaryDirectory() as directory:
            install_root = Path(directory) / "omegon-shipped"
            subprocess.run(
                [
                    "python3",
                    "scripts/content_pack_manifest.py",
                    "--install-root",
                    str(install_root),
                ],
                cwd=ROOT,
                check=True,
            )
            installed = {
                path.relative_to(install_root).as_posix()
                for path in install_root.rglob("*")
                if path.is_file() and path.name != "content-pack.toml"
            }
            self.assertEqual(installed, expected)

    def test_supported_package_surfaces_name_the_pack(self) -> None:
        required = {
            "Justfile": "content-packs/omegon-shipped",
            "core/install.sh": "content-packs/omegon-shipped",
            "homebrew/Formula/omegon.rb": "content-packs/omegon-shipped",
            "flake.nix": "content_pack_manifest.py",
            "nix/oci.nix": "omegon",
        }
        for relative, marker in required.items():
            self.assertIn(marker, (ROOT / relative).read_text(), relative)

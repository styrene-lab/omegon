#!/usr/bin/env python3
"""Black-box tests for dirty_report.py git porcelain behavior."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "dirty_report.py"


def run(cwd: Path, args: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=cwd, check=check, text=True, capture_output=True)


class DirtyReportGitTests(unittest.TestCase):
    def init_repo(self) -> Path:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        repo = Path(tmp.name)
        run(repo, ["git", "init"])
        run(repo, ["git", "config", "user.email", "test@example.invalid"])
        run(repo, ["git", "config", "user.name", "Dirty Report Test"])
        (repo / ".gitignore").write_text("*.pyc\n")
        run(repo, ["git", "add", ".gitignore"])
        run(repo, ["git", "commit", "-m", "init"])
        return repo

    def source_clean(self, repo: Path) -> subprocess.CompletedProcess[str]:
        return run(repo, [sys.executable, str(SCRIPT), "--source-clean"], check=False)

    def test_unstaged_tracked_audit_log_passes_source_clean(self) -> None:
        repo = self.init_repo()
        audit = repo / ".omegon" / "audit-log.jsonl"
        audit.parent.mkdir()
        audit.write_text('{"event":"initial"}\n')
        run(repo, ["git", "add", str(audit.relative_to(repo))])
        run(repo, ["git", "commit", "-m", "track audit"])

        audit.write_text('{"event":"initial"}\n{"event":"live"}\n')
        result = self.source_clean(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Source tree clean; agent state dirty", result.stdout)

    def test_staged_tracked_audit_log_fails_source_clean(self) -> None:
        repo = self.init_repo()
        audit = repo / ".omegon" / "audit-log.jsonl"
        audit.parent.mkdir()
        audit.write_text('{"event":"initial"}\n')
        run(repo, ["git", "add", str(audit.relative_to(repo))])
        run(repo, ["git", "commit", "-m", "track audit"])

        audit.write_text('{"event":"initial"}\n{"event":"live"}\n')
        run(repo, ["git", "add", str(audit.relative_to(repo))])
        result = self.source_clean(repo)

        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("agent-state is staged", result.stdout)

    def test_dirty_source_file_fails_source_clean(self) -> None:
        repo = self.init_repo()
        src = repo / "core" / "crates" / "omegon" / "src" / "lib.rs"
        src.parent.mkdir(parents=True)
        src.write_text("pub fn demo() {}\n")
        run(repo, ["git", "add", str(src.relative_to(repo))])
        run(repo, ["git", "commit", "-m", "track source"])

        src.write_text("pub fn demo() -> u8 { 1 }\n")
        result = self.source_clean(repo)

        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("rust-source", result.stdout)

    def test_untracked_ignored_audit_log_does_not_block(self) -> None:
        repo = self.init_repo()
        (repo / ".gitignore").write_text("*.pyc\n.omegon/audit-log.jsonl\n")
        run(repo, ["git", "add", ".gitignore"])
        run(repo, ["git", "commit", "-m", "ignore audit"])
        audit = repo / ".omegon" / "audit-log.jsonl"
        audit.parent.mkdir()
        audit.write_text('{"event":"live"}\n')

        result = self.source_clean(repo)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Source tree clean", result.stdout)


if __name__ == "__main__":
    unittest.main()

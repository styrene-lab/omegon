import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "benchmark_harness.py"


class BenchmarkHarnessTests(unittest.TestCase):
    def write_task(self, repo: Path, content: str) -> Path:
        task = repo / "task.yaml"
        task.write_text(content)
        return task

    def run_script(self, *args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["python3", str(SCRIPT), *args],
            cwd=cwd or ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    def init_repo(self, repo: Path) -> None:
        (repo / "ai" / "benchmarks" / "tasks").mkdir(parents=True, exist_ok=True)
        (repo / "scripts").mkdir(parents=True, exist_ok=True)
        (repo / "core").mkdir(parents=True, exist_ok=True)
        (repo / "core" / "Cargo.toml").write_text("[workspace]\n")

    def test_rejects_missing_required_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            task = self.write_task(repo, "id: broken\nrepo: .\n")
            result = self.run_script(str(task), "--root", str(repo))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing required fields", result.stderr)

    def test_rejects_unknown_harness(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            task = self.write_task(
                repo,
                """
id: t1
repo: .
base_ref: main
prompt: hi
harnesses: [bogus]
acceptance: [echo ok]
""",
            )
            result = self.run_script(str(task), "--root", str(repo))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported harness", result.stderr)

    def test_declared_harness_without_binary_fails_usefully(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            task = self.write_task(
                repo,
                """
id: t2
repo: .
base_ref: main
prompt: hi
harnesses: [claude-code]
acceptance: [echo ok]
""",
            )
            env = dict(os.environ)
            env["PATH"] = f"{repo / 'scripts'}:/usr/bin:/bin"
            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(task), "--root", str(repo), "--harness", "claude-code"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertEqual(result.returncode, 2)
            self.assertIn("claude-code adapter requires 'claude' in PATH", result.stderr)

    def test_writes_result_for_mocked_omegon_run(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_cargo = repo / "scripts" / "cargo"
            fake_cargo.write_text(
                "#!/bin/sh\n"
                "usage_json=''\n"
                "prev=''\n"
                "for arg in \"$@\"; do\n"
                "  if [ \"$prev\" = \"--usage-json\" ]; then usage_json=\"$arg\"; fi\n"
                "  prev=\"$arg\"\n"
                "done\n"
                "if [ -n \"$usage_json\" ]; then\n"
                "  cat > \"$usage_json\" <<'JSON'\n"
                '{"input_tokens": 1200, "output_tokens": 300, "cache_tokens": 0, "extra": {"context": {"sys": 100, "tools": 50}}}\n'
                "JSON\n"
                "fi\n"
                "echo fake omegon run\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t3
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
acceptance:
  - python3 -c \"print('ok')\"
""",
            )
            env = dict(os.environ)
            env["PATH"] = f"{repo / 'scripts'}:{env['PATH']}"
            result = subprocess.run(
                ["python3", str(SCRIPT), str(task), "--root", str(repo)],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            result_path = Path(result.stdout.strip())
            payload = json.loads(result_path.read_text())
            self.assertEqual(payload["status"], "pass")
            self.assertEqual(payload["benchmark_mode"]["adapter_profile"], "omegon-native")
            self.assertTrue(payload["benchmark_mode"]["clean_room"])
            self.assertEqual(payload["tokens"]["total"], 1500)
            self.assertEqual(payload["harness"], "omegon")
            self.assertEqual(payload["extra"]["context"]["sys"], 100)

    def test_pi_adapter_normalizes_json_usage(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_pi = repo / "scripts" / "pi"
            fake_pi.write_text(
                "#!/bin/sh\n"
                "cat <<'JSON'\n"
                '{"type":"session"}\n'
                '{"type":"message_end","message":{"role":"assistant","model":"openai/gpt-4o","usage":{"input":111,"output":22,"cacheRead":3,"cacheWrite":9}}}\n'
                "JSON\n"
            )
            fake_pi.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t4
repo: .
base_ref: main
prompt: hi
harnesses: [pi]
acceptance:
  - python3 -c \"print('ok')\"
""",
            )
            env = dict(os.environ)
            env["PATH"] = f"{repo / 'scripts'}:{env['PATH']}"
            result = subprocess.run(
                ["python3", str(SCRIPT), str(task), "--root", str(repo), "--harness", "pi"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(Path(result.stdout.strip()).read_text())
            self.assertEqual(payload["harness"], "pi")
            self.assertEqual(payload["benchmark_mode"]["adapter_profile"], "minimal")
            self.assertEqual(payload["model"], "openai/gpt-4o")
            self.assertEqual(payload["tokens"]["total"], 136)
            self.assertEqual(payload["tokens"]["cache_write"], 9)

    def test_claude_adapter_normalizes_json_usage(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_claude = repo / "scripts" / "claude"
            fake_claude.write_text(
                "#!/bin/sh\n"
                "cat <<'JSON'\n"
                '{"model":"claude-sonnet-4-6","usage":{"input_tokens":210,"output_tokens":34,"cache_read_input_tokens":5,"cache_creation_input_tokens":144}}\n'
                "JSON\n"
            )
            fake_claude.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t5
repo: .
base_ref: main
prompt: hi
harnesses: [claude-code]
acceptance:
  - python3 -c \"print('ok')\"
""",
            )
            env = dict(os.environ)
            env["PATH"] = f"{repo / 'scripts'}:{env['PATH']}"
            result = subprocess.run(
                ["python3", str(SCRIPT), str(task), "--root", str(repo), "--harness", "claude-code"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(Path(result.stdout.strip()).read_text())
            self.assertEqual(payload["harness"], "claude-code")
            self.assertEqual(payload["benchmark_mode"]["adapter_profile"], "default")
            self.assertEqual(payload["model"], "claude-sonnet-4-6")
            self.assertEqual(payload["tokens"]["total"], 249)
            self.assertEqual(payload["tokens"]["cache"], 5)
            self.assertEqual(payload["tokens"]["cache_write"], 144)


if __name__ == "__main__":
    unittest.main()

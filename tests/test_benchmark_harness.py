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
                '{"input_tokens": 1200, "output_tokens": 300, "cache_tokens": 0, "estimated_tokens": 1700, "context_window": 200000, "context_composition": {"system_tokens": 100, "tool_schema_tokens": 50, "conversation_tokens": 400, "memory_tokens": 25, "tool_history_tokens": 75, "thinking_tokens": 10, "free_tokens": 199340}, "extra": {"context": {"sys": 100, "tools": 50}}}\n'
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
            self.assertEqual(
                payload["omegon_context"],
                {
                    "sys": 100,
                    "tools": 50,
                    "conv": 400,
                    "mem": 25,
                    "hist": 75,
                    "think": 10,
                    "free": 199340,
                },
            )
            self.assertEqual(payload["telemetry"]["estimated_tokens"], 1700)
            self.assertEqual(payload["telemetry"]["context_window"], 200000)

    def test_acceptance_runs_in_clean_repo_not_source_repo(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            subprocess.run(["git", "init", "-b", "main"], cwd=repo, check=True, capture_output=True, text=True)
            subprocess.run(["git", "config", "user.name", "Benchmark Test"], cwd=repo, check=True)
            subprocess.run(["git", "config", "user.email", "benchmark@example.com"], cwd=repo, check=True)
            (repo / "marker.txt").write_text("source\n")
            subprocess.run(["git", "add", "."], cwd=repo, check=True)
            subprocess.run(["git", "commit", "-m", "init"], cwd=repo, check=True, capture_output=True, text=True)

            fake_cargo = repo / "scripts" / "cargo"
            fake_cargo.write_text(
                "#!/bin/sh\n"
                "usage_json=''\n"
                "prev=''\n"
                "for arg in \"$@\"; do\n"
                "  if [ \"$prev\" = \"--usage-json\" ]; then usage_json=\"$arg\"; fi\n"
                "  prev=\"$arg\"\n"
                "done\n"
                "printf 'clean\n' > marker.txt\n"
                "if [ -n \"$usage_json\" ]; then\n"
                "  cat > \"$usage_json\" <<'JSON'\n"
                '{"input_tokens": 10, "output_tokens": 5, "cache_tokens": 0}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)

            task = self.write_task(
                repo,
                """
id: t6
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
acceptance:
  - python3 -c \"from pathlib import Path; import sys; sys.exit(0 if Path('marker.txt').read_text().strip() == 'clean' else 1)\"
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
            payload = json.loads(Path(result.stdout.strip()).read_text())
            self.assertEqual(payload["status"], "pass")
            self.assertEqual((repo / "marker.txt").read_text().strip(), "source")

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
            self.assertEqual(payload["tokens"]["total"], 145)
            self.assertEqual(payload["tokens"]["cache_write"], 9)

    def test_claude_adapter_error_result_becomes_benchmark_error_even_if_acceptance_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_claude = repo / "scripts" / "claude"
            fake_claude.write_text(
                "#!/bin/sh\n"
                "cat <<'JSON'\n"
                '{"type":"result","subtype":"success","is_error":true,"result":"model access denied","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}\n'
                "JSON\n"
            )
            fake_claude.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: claude-error
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
            self.assertEqual(result.returncode, 3, result.stderr)
            payload = json.loads(Path(result.stdout.strip()).read_text())
            self.assertEqual(payload["status"], "error")
            self.assertEqual(payload["score"], 0.0)
            self.assertEqual(payload["adapter"]["execution_status"], "error")
            self.assertEqual(payload["adapter"]["error_message"], "model access denied")
            self.assertEqual(payload["acceptance"]["commands"][0]["exit"], 0)

    def test_task_spec_model_is_used_when_cli_override_is_absent(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_cargo = repo / "scripts" / "cargo"
            captured = repo / "captured-model.txt"
            fake_cargo.write_text(
                "#!/bin/sh\n"
                "usage_json=''\n"
                "model=''\n"
                "prev=''\n"
                "for arg in \"$@\"; do\n"
                "  if [ \"$prev\" = \"--usage-json\" ]; then usage_json=\"$arg\"; fi\n"
                "  if [ \"$prev\" = \"--model\" ]; then model=\"$arg\"; fi\n"
                "  prev=\"$arg\"\n"
                "done\n"
                f"printf '%s\\n' \"$model\" > \"{captured}\"\n"
                "if [ -n \"$usage_json\" ]; then\n"
                "  cat > \"$usage_json\" <<'JSON'\n"
                '{"input_tokens": 1, "output_tokens": 1, "cache_tokens": 0}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: model-task
repo: .
base_ref: main
model: anthropic:claude-sonnet-4-6
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
            self.assertEqual(captured.read_text().strip(), "anthropic:claude-sonnet-4-6")

    def test_cli_model_overrides_task_spec_model(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_cargo = repo / "scripts" / "cargo"
            captured = repo / "captured-model.txt"
            fake_cargo.write_text(
                "#!/bin/sh\n"
                "usage_json=''\n"
                "model=''\n"
                "prev=''\n"
                "for arg in \"$@\"; do\n"
                "  if [ \"$prev\" = \"--usage-json\" ]; then usage_json=\"$arg\"; fi\n"
                "  if [ \"$prev\" = \"--model\" ]; then model=\"$arg\"; fi\n"
                "  prev=\"$arg\"\n"
                "done\n"
                f"printf '%s\\n' \"$model\" > \"{captured}\"\n"
                "if [ -n \"$usage_json\" ]; then\n"
                "  cat > \"$usage_json\" <<'JSON'\n"
                '{"input_tokens": 1, "output_tokens": 1, "cache_tokens": 0}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: model-task
repo: .
base_ref: main
model: anthropic:claude-sonnet-4-6
prompt: hi
harnesses: [omegon]
acceptance:
  - python3 -c \"print('ok')\"
""",
            )
            env = dict(os.environ)
            env["PATH"] = f"{repo / 'scripts'}:{env['PATH']}"
            result = subprocess.run(
                [
                    "python3",
                    str(SCRIPT),
                    str(task),
                    "--root",
                    str(repo),
                    "--model",
                    "openai:gpt-4o",
                ],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(captured.read_text().strip(), "openai:gpt-4o")

    def test_report_mode_accepts_directory_and_handles_zero_token_baseline(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            runs = repo / "runs"
            runs.mkdir()
            (runs / "a-omegon.json").write_text(
                json.dumps(
                    {
                        "task_id": "hello-bench",
                        "harness": "omegon",
                        "model": "anthropic:claude-sonnet-4-6",
                        "status": "pass",
                        "score": 1.0,
                        "wall_clock_sec": 10,
                        "tokens": {"total": 0},
                    }
                )
            )
            (runs / "b-pi.json").write_text(
                json.dumps(
                    {
                        "task_id": "hello-bench",
                        "harness": "pi",
                        "model": "claude-sonnet-4-6",
                        "status": "pass",
                        "score": 1.0,
                        "wall_clock_sec": 11,
                        "tokens": {"total": 11},
                    }
                )
            )

            result = self.run_script("--report", str(runs), cwd=repo)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Task: hello-bench", result.stdout)
            self.assertIn("- token ratio: unavailable — baseline result reported zero total tokens", result.stdout)

    def test_report_mode_prints_plaintext_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            result_a = repo / "omegon.json"
            result_b = repo / "claude.json"
            result_a.write_text(
                json.dumps(
                    {
                        "task_id": "shadow-context-assembly",
                        "harness": "omegon",
                        "model": "anthropic:claude-sonnet-4-6",
                        "status": "pass",
                        "score": 1.0,
                        "wall_clock_sec": 812,
                        "tokens": {"total": 19336},
                        "omegon_context": {
                            "sys": 6200,
                            "tools": 4100,
                            "conv": 2800,
                            "mem": 700,
                            "hist": 3100,
                            "think": 1134,
                            "free": 181966,
                        },
                    }
                )
            )
            result_b.write_text(
                json.dumps(
                    {
                        "task_id": "shadow-context-assembly",
                        "harness": "claude-code",
                        "model": "claude-sonnet-4-6",
                        "status": "pass",
                        "score": 1.0,
                        "wall_clock_sec": 503,
                        "tokens": {"total": 7211},
                    }
                )
            )

            result = self.run_script("--report", str(result_a), str(result_b), cwd=repo)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Task: shadow-context-assembly", result.stdout)
            self.assertIn("- omegon / anthropic:claude-sonnet-4-6", result.stdout)
            self.assertIn("omegon context: sys 6200, tools 4100, conv 2800, mem 700, hist 3100, think 1134", result.stdout)
            self.assertIn("- claude-code / claude-sonnet-4-6", result.stdout)
            self.assertIn("token ratio: 2.68x more tokens for omegon", result.stdout)
            self.assertIn("likely excess buckets: sys + tools + hist", result.stdout)

    def test_report_mode_accepts_directory_and_groups_by_task(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            runs = repo / "runs"
            runs.mkdir()
            (runs / "task-a-omegon.json").write_text(
                json.dumps(
                    {
                        "task_id": "task-a",
                        "harness": "omegon",
                        "model": "anthropic:claude-sonnet-4-6",
                        "status": "pass",
                        "score": 1.0,
                        "wall_clock_sec": 100,
                        "tokens": {"total": 200},
                        "omegon_context": {"sys": 90, "tools": 60, "hist": 30, "conv": 10, "mem": 5, "think": 2},
                    }
                )
            )
            (runs / "task-a-claude.json").write_text(
                json.dumps(
                    {
                        "task_id": "task-a",
                        "harness": "claude-code",
                        "model": "claude-sonnet-4-6",
                        "status": "pass",
                        "score": 1.0,
                        "wall_clock_sec": 80,
                        "tokens": {"total": 100},
                    }
                )
            )
            (runs / "task-b-pi.json").write_text(
                json.dumps(
                    {
                        "task_id": "task-b",
                        "harness": "pi",
                        "model": "openai/gpt-4o",
                        "status": "fail",
                        "score": 0.0,
                        "wall_clock_sec": 50,
                        "tokens": {"total": 150},
                    }
                )
            )
            (runs / "notes.txt").write_text("ignore me\n")

            result = self.run_script("--report", str(runs), cwd=repo)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Task: task-a", result.stdout)
            self.assertIn("Task: task-b", result.stdout)
            self.assertEqual(result.stdout.count("Task:"), 2)
            self.assertIn("token ratio: 2.00x more tokens for omegon", result.stdout)
            self.assertIn("- pi / openai/gpt-4o", result.stdout)

    def test_report_mode_rejects_empty_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            empty = repo / "empty"
            empty.mkdir()
            result = self.run_script("--report", str(empty), cwd=repo)
            self.assertEqual(result.returncode, 1)
            self.assertIn("report requires at least one result artifact", result.stderr)

    def test_requires_task_when_not_reporting(self) -> None:
        result = self.run_script()
        self.assertEqual(result.returncode, 1)
        self.assertIn("task is required unless --report is used", result.stderr)

    def test_report_mode_rejects_non_object_json(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            result_file = repo / "bad.json"
            result_file.write_text("[]\n")
            result = self.run_script("--report", str(result_file), cwd=repo)
            self.assertEqual(result.returncode, 1)
            self.assertIn("result file must contain a JSON object", result.stderr)

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
            self.assertEqual(payload["tokens"]["total"], 393)
            self.assertEqual(payload["tokens"]["cache"], 5)
            self.assertEqual(payload["tokens"]["cache_write"], 144)


if __name__ == "__main__":
    unittest.main()

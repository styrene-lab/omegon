import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "benchmark_harness.py"
SPEC = importlib.util.spec_from_file_location("benchmark_harness_module", SCRIPT)
assert SPEC and SPEC.loader
BENCHMARK_HARNESS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = BENCHMARK_HARNESS
SPEC.loader.exec_module(BENCHMARK_HARNESS)


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

    def init_repo(self, repo: Path, *, workspace_role: str | None = None) -> None:
        (repo / "ai" / "benchmarks" / "tasks").mkdir(parents=True, exist_ok=True)
        (repo / "scripts").mkdir(parents=True, exist_ok=True)
        (repo / "core").mkdir(parents=True, exist_ok=True)
        (repo / "core" / "Cargo.toml").write_text("[workspace]\n")
        if workspace_role is not None:
            (repo / ".omegon" / "runtime").mkdir(parents=True, exist_ok=True)
            (repo / ".omegon" / "runtime" / "workspace.json").write_text(
                '{"role": "%s"}\n' % workspace_role
            )

    def test_release_eval_requires_benchmark_workspace_role(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo, workspace_role="feature")
            task = self.write_task(
                repo,
                """
id: t-release-eval
repo: .
base_ref: v0.15.10-rc.68
prompt: hi
harnesses: [omegon]
acceptance: [echo ok]
""",
            )
            result = self.run_script(str(task), "--root", str(repo))
            self.assertEqual(result.returncode, 1)
            self.assertIn("workspace role 'benchmark'", result.stderr)

    def test_release_eval_passes_with_benchmark_workspace_role(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo, workspace_role="benchmark")
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
                '{"input_tokens": 10, "output_tokens": 5, "cache_tokens": 0}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t-release-eval-pass
repo: .
base_ref: v0.15.10-rc.68
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

    def test_load_task_spec_accepts_richer_schema(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            task = self.write_task(
                repo,
                """
id: t-rich
kind: diagnosis
repo: .
base_ref: main
prompt: inspect and fix
matrix:
  harnesses: [omegon, pi]
  models: [anthropic:claude-sonnet-4-6, openai-codex:gpt-5.4]
acceptance:
  required:
    - python3 -c \"print('ok')\"
  optional:
    - python3 -c \"print('optional')\"
  failure_if:
    - python3 -c \"print('guard')\"
process_expectations:
  max_orientation_only_turns: 1
expected_solution:
  primary_files: [core/crates/omegon/src/context.rs]
notes: richer schema
""",
            )
            spec = BENCHMARK_HARNESS.load_task_spec(task)
            self.assertEqual(spec.kind, "diagnosis")
            self.assertEqual(spec.harnesses, ["omegon", "pi"])
            self.assertEqual(spec.models, ["anthropic:claude-sonnet-4-6", "openai-codex:gpt-5.4"])
            self.assertEqual(spec.acceptance, ["python3 -c \"print('ok')\""])
            self.assertEqual(spec.acceptance_optional, ["python3 -c \"print('optional')\""])
            self.assertEqual(spec.acceptance_failure_if, ["python3 -c \"print('guard')\""])
            self.assertEqual(spec.process_expectations["max_orientation_only_turns"], 1)
            self.assertEqual(spec.expected_solution["primary_files"], ["core/crates/omegon/src/context.rs"])
            self.assertEqual(spec.notes, "richer schema")

    def test_load_task_spec_legacy_schema_still_works(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            task = self.write_task(
                repo,
                """
id: t-legacy
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
model: anthropic:claude-sonnet-4-6
acceptance:
  - python3 -c \"print('ok')\"
""",
            )
            spec = BENCHMARK_HARNESS.load_task_spec(task)
            self.assertEqual(spec.kind, "implementation")
            self.assertEqual(spec.harnesses, ["omegon"])
            self.assertEqual(spec.models, ["anthropic:claude-sonnet-4-6"])
            self.assertEqual(spec.acceptance, ["python3 -c \"print('ok')\""])
            self.assertEqual(spec.acceptance_optional, [])
            self.assertEqual(spec.acceptance_failure_if, [])

    def test_structured_acceptance_and_task_metadata_emitted_in_result(self) -> None:
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
                '{"input_tokens": 10, "output_tokens": 5, "cache_tokens": 0}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t-structured
kind: diagnosis
repo: .
base_ref: main
prompt: inspect and fix
matrix:
  harnesses: [omegon]
  models: [anthropic:claude-sonnet-4-6, openai-codex:gpt-5.4]
acceptance:
  required:
    - python3 -c \"print('ok')\"
  optional:
    - python3 -c \"print('optional')\"
  failure_if:
    - python3 -c \"import sys; sys.exit(1)\"
process_expectations:
  max_orientation_only_turns: 1
expected_solution:
  primary_files: [core/crates/omegon/src/context.rs]
budget:
  max_turns: 12
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
            self.assertEqual(payload["task_kind"], "diagnosis")
            self.assertEqual(payload["task"]["kind"], "diagnosis")
            self.assertEqual(payload["task"]["matrix"]["models"], ["anthropic:claude-sonnet-4-6", "openai-codex:gpt-5.4"])
            self.assertEqual(payload["task"]["process_expectations"], {"max_orientation_only_turns": 1})
            self.assertEqual(payload["task"]["expected_solution"], {"primary_files": ["core/crates/omegon/src/context.rs"]})
            self.assertEqual(payload["task"]["budgets"], {"max_turns": 12})
            self.assertEqual(payload["process"]["expectations"], {"max_orientation_only_turns": 1})
            self.assertEqual(payload["process"]["turn_count"], None)
            self.assertEqual(payload["process"]["turn_end_reasons"], {})
            self.assertEqual(payload["process"]["dominant_phases"], {})
            self.assertEqual(payload["process"]["drift_kinds"], {})
            self.assertEqual(payload["process"]["progress_nudge_reasons"], {})
            self.assertEqual(payload["acceptance"]["status"], "pass")
            self.assertEqual(payload["acceptance"]["required_status"], "pass")
            self.assertFalse(payload["acceptance"]["failure_if_triggered"])
            self.assertEqual(len(payload["acceptance"]["required"]), 1)
            self.assertEqual(len(payload["acceptance"]["optional"]), 1)
            optional_entry = payload["acceptance"]["optional"][0]
            self.assertEqual(optional_entry["cmd"], "python3 -c \"print('optional')\"")
            self.assertEqual(optional_entry["status"], "pass")
            self.assertEqual(optional_entry["exit"], 0)
            self.assertEqual(len(payload["acceptance"]["failure_if"]), 1)
            guard_entry = payload["acceptance"]["failure_if"][0]
            self.assertEqual(guard_entry["cmd"], "python3 -c \"import sys; sys.exit(1)\"")
            self.assertEqual(guard_entry["status"], "clear")
            self.assertEqual(guard_entry["exit"], 1)

    def _write_passing_fake_cargo(self, repo: Path) -> None:
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
            '{"input_tokens": 10, "output_tokens": 5, "cache_tokens": 0}\n'
            "JSON\n"
            "fi\n"
            "exit 0\n"
        )
        fake_cargo.chmod(0o755)

    def test_failure_if_triggered_overrides_passing_required(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            self._write_passing_fake_cargo(repo)
            task = self.write_task(
                repo,
                """
id: t-failure-if-triggered
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
acceptance:
  required:
    - python3 -c \"print('ok')\"
  failure_if:
    - python3 -c \"print('boom')\"
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
            # main() returns 3 when the run fails
            self.assertEqual(result.returncode, 3)
            payload = json.loads(Path(result.stdout.strip()).read_text())
            self.assertEqual(payload["status"], "fail")
            self.assertEqual(payload["score"], 0.0)
            self.assertEqual(payload["acceptance"]["status"], "fail")
            self.assertEqual(payload["acceptance"]["required_status"], "pass")
            self.assertTrue(payload["acceptance"]["failure_if_triggered"])
            self.assertEqual(len(payload["acceptance"]["failure_if"]), 1)
            entry = payload["acceptance"]["failure_if"][0]
            self.assertEqual(entry["status"], "triggered")
            self.assertEqual(entry["exit"], 0)

    def test_grade_process_expectations_returns_pass_when_within_threshold(self) -> None:
        result = BENCHMARK_HARNESS.grade_process_expectations(
            {"max_orientation_only_turns": 2, "max_turns": 12},
            turn_count=4,
            derived={"orientation_only_turns": 1},
        )
        self.assertEqual(result["status"], "pass")
        self.assertEqual(result["violations"], [])
        statuses = {c["expectation"]: c["status"] for c in result["checks"]}
        self.assertEqual(statuses["max_orientation_only_turns"], "pass")
        self.assertEqual(statuses["max_turns"], "pass")
        # actual_source should be populated for evaluated checks
        sources = {c["expectation"]: c["actual_source"] for c in result["checks"]}
        self.assertEqual(sources["max_orientation_only_turns"], "derived.orientation_only_turns")
        self.assertEqual(sources["max_turns"], "turn_count")

    def test_grade_process_expectations_flags_violations_without_gating(self) -> None:
        result = BENCHMARK_HARNESS.grade_process_expectations(
            {"max_orientation_only_turns": 0, "max_turns": 12},
            turn_count=4,
            derived={"orientation_only_turns": 3},
        )
        self.assertEqual(result["status"], "fail")
        self.assertEqual(len(result["violations"]), 1)
        violation = result["violations"][0]
        self.assertEqual(violation["expectation"], "max_orientation_only_turns")
        self.assertEqual(violation["threshold"], 0)
        self.assertEqual(violation["actual"], 3)
        # max_turns is still recorded as a passing check alongside the violation
        check_names = {c["expectation"] for c in result["checks"]}
        self.assertEqual(check_names, {"max_orientation_only_turns", "max_turns"})

    def test_grade_process_expectations_marks_unsupported_keys_not_evaluated(self) -> None:
        result = BENCHMARK_HARNESS.grade_process_expectations(
            {
                "must_touch_repo_before_edit": True,
                "max_orientation_only_turns": 2,
            },
            turn_count=4,
            derived={"orientation_only_turns": 1},
        )
        # Overall is pass because at least one supported expectation evaluated to pass.
        self.assertEqual(result["status"], "pass")
        unsupported = next(c for c in result["checks"] if c["expectation"] == "must_touch_repo_before_edit")
        self.assertEqual(unsupported["status"], "not_evaluated")
        self.assertEqual(unsupported["reason"], "unsupported_expectation")

    def test_grade_process_expectations_returns_not_evaluated_without_telemetry(self) -> None:
        result = BENCHMARK_HARNESS.grade_process_expectations(
            {"max_orientation_only_turns": 1, "max_turns": 10},
            turn_count=None,
            derived={},
        )
        self.assertEqual(result["status"], "not_evaluated")
        self.assertEqual(result["violations"], [])
        for check in result["checks"]:
            self.assertEqual(check["status"], "not_evaluated")
            self.assertEqual(check["reason"], "process_telemetry_unavailable")

    def test_grade_process_expectations_empty_returns_not_evaluated(self) -> None:
        result = BENCHMARK_HARNESS.grade_process_expectations({}, turn_count=4, derived={"orientation_only_turns": 0})
        self.assertEqual(result["status"], "not_evaluated")
        self.assertEqual(result["checks"], [])
        self.assertEqual(result["violations"], [])

    def test_extract_budget_tiers_supports_flat_and_layered_schemas(self) -> None:
        flat_soft, flat_hard = BENCHMARK_HARNESS.extract_budget_tiers(
            {"max_turns": 20, "max_minutes": 30}
        )
        self.assertEqual(flat_soft, {"max_turns": 20, "max_minutes": 30})
        self.assertEqual(flat_hard, {})

        soft, hard = BENCHMARK_HARNESS.extract_budget_tiers(
            {"soft": {"max_turns": 12}, "hard": {"max_turns": 25}}
        )
        self.assertEqual(soft, {"max_turns": 12})
        self.assertEqual(hard, {"max_turns": 25})

        soft_only, hard_only = BENCHMARK_HARNESS.extract_budget_tiers({"soft": {"max_turns": 12}})
        self.assertEqual(soft_only, {"max_turns": 12})
        self.assertEqual(hard_only, {})

        empty_soft, empty_hard = BENCHMARK_HARNESS.extract_budget_tiers({})
        self.assertEqual(empty_soft, {})
        self.assertEqual(empty_hard, {})

    def test_grade_efficiency_budgets_pass_warn_fail_per_tier(self) -> None:
        budget = {
            "soft": {"max_turns": 12, "max_total_tokens": 1_000_000},
            "hard": {"max_turns": 25, "max_total_tokens": 2_000_000},
        }
        # Pass — both within soft.
        passing = BENCHMARK_HARNESS.grade_efficiency_budgets(
            budget, total_tokens=500_000, input_tokens=None, wall_clock_sec=None, turn_count=8
        )
        self.assertEqual(passing["status"], "pass")
        self.assertEqual(passing["score"], 1.0)
        statuses = {c["key"]: c["status"] for c in passing["checks"]}
        self.assertEqual(statuses, {"max_turns": "pass", "max_total_tokens": "pass"})

        # Warn — turns over soft but within hard, tokens still pass.
        warn = BENCHMARK_HARNESS.grade_efficiency_budgets(
            budget, total_tokens=500_000, input_tokens=None, wall_clock_sec=None, turn_count=18
        )
        self.assertEqual(warn["status"], "warn")
        # mean of 0.5 (turns warn) + 1.0 (tokens pass) = 0.75
        self.assertEqual(warn["score"], 0.75)
        warn_check = next(c for c in warn["checks"] if c["key"] == "max_turns")
        self.assertEqual(warn_check["status"], "warn")
        self.assertEqual(warn_check["score"], 0.5)

        # Fail — turns over hard.
        failing = BENCHMARK_HARNESS.grade_efficiency_budgets(
            budget, total_tokens=500_000, input_tokens=None, wall_clock_sec=None, turn_count=40
        )
        self.assertEqual(failing["status"], "fail")
        fail_check = next(c for c in failing["checks"] if c["key"] == "max_turns")
        self.assertEqual(fail_check["status"], "fail")
        self.assertEqual(fail_check["score"], 0.0)

    def test_grade_efficiency_budgets_legacy_flat_budget_treated_as_soft(self) -> None:
        budget = {"max_turns": 20, "max_minutes": 30}
        # Within both → pass.
        within = BENCHMARK_HARNESS.grade_efficiency_budgets(
            budget, total_tokens=None, input_tokens=None, wall_clock_sec=600.0, turn_count=10
        )
        self.assertEqual(within["status"], "pass")
        # Over the soft turn limit, no hard ceiling → fail (not warn).
        over = BENCHMARK_HARNESS.grade_efficiency_budgets(
            budget, total_tokens=None, input_tokens=None, wall_clock_sec=600.0, turn_count=25
        )
        self.assertEqual(over["status"], "fail")
        over_check = next(c for c in over["checks"] if c["key"] == "max_turns")
        self.assertEqual(over_check["status"], "fail")

    def test_grade_efficiency_budgets_missing_actuals_become_not_evaluated_per_key(self) -> None:
        budget = {"soft": {"max_turns": 12, "max_total_tokens": 1_000_000}}
        # Only turn_count is supplied; tokens are None → that key is not_evaluated.
        result = BENCHMARK_HARNESS.grade_efficiency_budgets(
            budget, total_tokens=None, input_tokens=None, wall_clock_sec=None, turn_count=4
        )
        per_key = {c["key"]: c for c in result["checks"]}
        self.assertEqual(per_key["max_turns"]["status"], "pass")
        self.assertEqual(per_key["max_total_tokens"]["status"], "not_evaluated")
        self.assertEqual(per_key["max_total_tokens"]["reason"], "actual_value_missing")
        # Composite is computed only over evaluated checks → still 1.0 here.
        self.assertEqual(result["score"], 1.0)
        self.assertEqual(result["status"], "pass")

    def test_grade_efficiency_budgets_empty_returns_not_evaluated(self) -> None:
        result = BENCHMARK_HARNESS.grade_efficiency_budgets(
            {}, total_tokens=10, input_tokens=5, wall_clock_sec=1.0, turn_count=1
        )
        self.assertEqual(result["status"], "not_evaluated")
        self.assertIsNone(result["score"])
        self.assertEqual(result["checks"], [])

    def test_grade_efficiency_budgets_unsupported_key_marked_not_evaluated(self) -> None:
        result = BENCHMARK_HARNESS.grade_efficiency_budgets(
            {"soft": {"max_turns": 10, "max_unicorns": 3}},
            total_tokens=None,
            input_tokens=None,
            wall_clock_sec=None,
            turn_count=5,
        )
        per_key = {c["key"]: c for c in result["checks"]}
        self.assertEqual(per_key["max_unicorns"]["status"], "not_evaluated")
        self.assertEqual(per_key["max_unicorns"]["reason"], "unsupported_budget_key")

    def test_grade_discipline_clean_run_is_full_score(self) -> None:
        result = BENCHMARK_HARNESS.grade_discipline(
            turn_count=4, derived={"progress_nudge_count": 0, "orientation_only_turns": 0}
        )
        self.assertEqual(result["score"], 1.0)
        self.assertEqual(result["status"], "pass")
        self.assertEqual(result["formula_version"], BENCHMARK_HARNESS.DISCIPLINE_FORMULA_VERSION)
        self.assertEqual(
            result["signals"],
            {"progress_nudge_count": 0, "orientation_only_turns": 0},
        )

    def test_grade_discipline_degrades_with_nudges_and_churn(self) -> None:
        # 1 nudge + 0 churn → 0.8 → still "pass"
        light = BENCHMARK_HARNESS.grade_discipline(
            turn_count=4, derived={"progress_nudge_count": 1, "orientation_only_turns": 0}
        )
        self.assertEqual(light["score"], 0.8)
        self.assertEqual(light["status"], "pass")
        # 2 nudges + 1 churn → 1.0 - 0.4 - 0.2 = 0.4 → "warn"
        warn = BENCHMARK_HARNESS.grade_discipline(
            turn_count=4, derived={"progress_nudge_count": 2, "orientation_only_turns": 1}
        )
        self.assertEqual(warn["score"], 0.4)
        self.assertEqual(warn["status"], "warn")
        # 5 nudges → score floored at 0 → "fail"
        bad = BENCHMARK_HARNESS.grade_discipline(
            turn_count=4, derived={"progress_nudge_count": 5, "orientation_only_turns": 0}
        )
        self.assertEqual(bad["score"], 0.0)
        self.assertEqual(bad["status"], "fail")

    def test_grade_discipline_returns_not_evaluated_without_telemetry(self) -> None:
        result = BENCHMARK_HARNESS.grade_discipline(turn_count=None, derived={})
        self.assertEqual(result["status"], "not_evaluated")
        self.assertIsNone(result["score"])
        self.assertEqual(result["signals"], {})

    def test_compose_scores_emits_four_axes_with_omegon_telemetry(self) -> None:
        process_metrics = {
            "turn_count": 4,
            "derived": {"progress_nudge_count": 0, "orientation_only_turns": 0},
            "grading": {"status": "pass", "checks": [{"status": "pass"}], "violations": []},
            "availability": "full",
        }
        scores = BENCHMARK_HARNESS.compose_scores(
            final_status="pass",
            final_score=1.0,
            process_metrics=process_metrics,
            budget={"soft": {"max_turns": 12}},
            total_tokens=10,
            input_tokens=5,
            wall_clock_sec=1.5,
            turn_count=4,
        )
        self.assertEqual(set(scores.keys()), {"outcome", "process", "efficiency", "discipline"})
        self.assertEqual(scores["outcome"], {"status": "pass", "score": 1.0})
        self.assertEqual(scores["process"]["status"], "pass")
        self.assertEqual(scores["process"]["score"], 1.0)
        self.assertEqual(scores["process"]["supported_checks"], 1)
        self.assertEqual(scores["process"]["passing_checks"], 1)
        self.assertEqual(scores["efficiency"]["status"], "pass")
        self.assertEqual(scores["efficiency"]["score"], 1.0)
        self.assertEqual(scores["discipline"]["status"], "pass")
        self.assertEqual(scores["discipline"]["score"], 1.0)

    def test_compose_scores_marks_process_and_discipline_not_evaluated_without_telemetry(self) -> None:
        process_metrics = {
            "turn_count": None,
            "derived": {},
            "grading": {"status": "not_evaluated", "checks": [], "violations": []},
            "availability": "none",
        }
        scores = BENCHMARK_HARNESS.compose_scores(
            final_status="pass",
            final_score=1.0,
            process_metrics=process_metrics,
            budget={"max_turns": 20},  # legacy flat
            total_tokens=10,
            input_tokens=5,
            wall_clock_sec=1.5,
            turn_count=None,
        )
        self.assertEqual(scores["process"]["status"], "not_evaluated")
        self.assertIsNone(scores["process"]["score"])
        self.assertEqual(scores["discipline"]["status"], "not_evaluated")
        self.assertIsNone(scores["discipline"]["score"])
        # Efficiency: max_turns is the only budget key but turn_count is None → not_evaluated.
        self.assertEqual(scores["efficiency"]["status"], "not_evaluated")
        # Outcome is still pass because the run passed regardless of telemetry.
        self.assertEqual(scores["outcome"]["status"], "pass")

    def test_scores_axes_appear_in_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            # Fake cargo emits clean telemetry: 4 turns, no nudges, no churn.
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
                '{"input_tokens": 100, "output_tokens": 50, "cache_tokens": 0, "turn_count": 4, "drift_kinds": {}, "progress_nudge_reasons": {}}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t-scores
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
acceptance:
  required:
    - python3 -c \"print('ok')\"
process_expectations:
  max_orientation_only_turns: 1
  max_turns: 20
budget:
  soft:
    max_turns: 12
    max_total_tokens: 1000000
  hard:
    max_turns: 25
    max_total_tokens: 2000000
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
            scores = payload["scores"]
            self.assertEqual(set(scores.keys()), {"outcome", "process", "efficiency", "discipline"})
            self.assertEqual(scores["outcome"]["status"], "pass")
            self.assertEqual(scores["outcome"]["score"], 1.0)
            self.assertEqual(scores["process"]["status"], "pass")
            self.assertEqual(scores["efficiency"]["status"], "pass")
            self.assertEqual(scores["discipline"]["status"], "pass")
            self.assertEqual(scores["discipline"]["formula_version"], "v1")
            # Existing top-level binary score is unchanged (outcome only).
            self.assertEqual(payload["score"], 1.0)
            self.assertEqual(payload["status"], "pass")

    def test_process_grading_violation_is_recorded_in_artifact_without_failing_run(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            # Fake cargo emits telemetry showing 1 orientation_churn turn.
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
                '{"input_tokens": 10, "output_tokens": 5, "cache_tokens": 0, "turn_count": 4, "drift_kinds": {"orientation_churn": 1}}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t-process-violation
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
acceptance:
  required:
    - python3 -c \"print('ok')\"
process_expectations:
  max_orientation_only_turns: 0
  max_turns: 100
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
            # Process violations are diagnostic only; the run still passes.
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(Path(result.stdout.strip()).read_text())
            self.assertEqual(payload["status"], "pass")
            grading = payload["process"]["grading"]
            self.assertEqual(grading["status"], "fail")
            self.assertEqual(len(grading["violations"]), 1)
            self.assertEqual(grading["violations"][0]["expectation"], "max_orientation_only_turns")
            self.assertEqual(grading["violations"][0]["actual"], 1)
            self.assertEqual(grading["violations"][0]["threshold"], 0)
            self.assertEqual(payload["process"]["availability"], "full")

    def test_process_grading_marks_availability_none_for_missing_telemetry(self) -> None:
        # When the omegon adapter emits no turn_count, availability should be "none"
        # and any declared expectations should grade as not_evaluated.
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
                '{"input_tokens": 10, "output_tokens": 5, "cache_tokens": 0}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t-process-availability
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
acceptance:
  required:
    - python3 -c \"print('ok')\"
process_expectations:
  max_orientation_only_turns: 1
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
            self.assertEqual(payload["process"]["availability"], "none")
            grading = payload["process"]["grading"]
            self.assertEqual(grading["status"], "not_evaluated")
            self.assertEqual(len(grading["checks"]), 1)
            self.assertEqual(grading["checks"][0]["status"], "not_evaluated")
            self.assertEqual(grading["checks"][0]["reason"], "process_telemetry_unavailable")

    def test_optional_acceptance_failure_does_not_gate_run(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            self._write_passing_fake_cargo(repo)
            task = self.write_task(
                repo,
                """
id: t-optional-fail
repo: .
base_ref: main
prompt: hi
harnesses: [omegon]
acceptance:
  required:
    - python3 -c \"print('ok')\"
  optional:
    - python3 -c \"import sys; sys.exit(2)\"
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
            # main() returns 0 when the run passes; optional failures must not gate
            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(Path(result.stdout.strip()).read_text())
            self.assertEqual(payload["status"], "pass")
            self.assertEqual(payload["score"], 1.0)
            self.assertEqual(payload["acceptance"]["status"], "pass")
            self.assertFalse(payload["acceptance"]["failure_if_triggered"])
            self.assertEqual(len(payload["acceptance"]["optional"]), 1)
            entry = payload["acceptance"]["optional"][0]
            self.assertEqual(entry["status"], "fail")
            self.assertEqual(entry["exit"], 2)

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

    def test_benchmark_process_env_uses_dedicated_target_dir_per_task_and_harness(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir) / "repo"
            clean = Path(tmpdir) / "clean"
            (repo / "core").mkdir(parents=True)
            (clean / "core").mkdir(parents=True)

            env = BENCHMARK_HARNESS.benchmark_process_env(repo, clean, "omegon", "task:alpha")
            expected = (repo / "core" / "target" / "benchmark-harness" / "task-alpha" / "omegon").resolve()
            self.assertEqual(env["CARGO_TARGET_DIR"], str(expected))

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
                '{"input_tokens": 1200, "output_tokens": 300, "cache_tokens": 0, "cache_write_tokens": 25, "estimated_tokens": 1700, "context_window": 200000, "turn_count": 4, "turn_end_reasons": {"tool_continuation": 3, "assistant_completed": 1}, "dominant_phases": {"observe": 2, "act": 2}, "drift_kinds": {"orientation_churn": 1}, "progress_nudge_reasons": {"anti_orientation": 1}, "requested_model": "anthropic:claude-sonnet-4-6", "requested_provider": "anthropic", "resolved_provider": "anthropic", "provider": "anthropic", "per_turn": {"avg_input_tokens": 300, "avg_output_tokens": 75, "avg_cache_tokens": 0, "avg_cache_write_tokens": 6, "avg_estimated_tokens": 425}, "context_composition": {"system_tokens": 100, "tool_schema_tokens": 50, "conversation_tokens": 400, "memory_tokens": 25, "tool_history_tokens": 75, "thinking_tokens": 10, "free_tokens": 199340}, "extra": {"context": {"sys": 100, "tools": 50}}}\n'
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
            self.assertEqual(payload["task_kind"], "implementation")
            self.assertEqual(payload["task"]["kind"], "implementation")
            self.assertEqual(payload["task"]["matrix"]["harnesses"], ["omegon"])
            self.assertEqual(payload["task"]["matrix"]["models"], [])
            self.assertEqual(payload["acceptance"]["status"], "pass")
            self.assertEqual(len(payload["acceptance"]["required"]), 1)
            self.assertEqual(payload["acceptance"]["optional"], [])
            self.assertEqual(payload["acceptance"]["failure_if"], [])
            self.assertEqual(payload["benchmark_mode"]["adapter_profile"], "omegon-native")
            self.assertTrue(payload["benchmark_mode"]["clean_room"])
            self.assertEqual(payload["tokens"]["total"], 1525)
            self.assertEqual(payload["harness"], "omegon")
            self.assertEqual(payload["requested_model"], "anthropic:claude-sonnet-4-6")
            self.assertEqual(payload["requested_provider"], "anthropic")
            self.assertEqual(payload["resolved_provider"], "anthropic")
            self.assertEqual(payload["provider"], "anthropic")
            self.assertEqual(payload["process"]["turn_count"], 4)
            self.assertEqual(payload["process"]["turn_end_reasons"], {"tool_continuation": 3, "assistant_completed": 1})
            self.assertEqual(payload["process"]["dominant_phases"], {"observe": 2, "act": 2})
            self.assertEqual(payload["process"]["drift_kinds"], {"orientation_churn": 1})
            self.assertEqual(payload["process"]["progress_nudge_reasons"], {"anti_orientation": 1})
            self.assertEqual(payload["process"]["derived"]["orientation_only_turns"], 1)
            self.assertEqual(payload["process"]["derived"]["progress_nudge_count"], 1)
            self.assertEqual(payload["process"]["derived"]["tool_continuation_turns"], 3)
            self.assertEqual(payload["process"]["derived"]["avg_input_tokens"], 300)
            self.assertEqual(payload["dominant_phases"], {"observe": 2, "act": 2})
            self.assertEqual(payload["drift_kinds"], {"orientation_churn": 1})
            self.assertEqual(payload["progress_nudge_reasons"], {"anti_orientation": 1})
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
            self.assertEqual(payload["telemetry"]["turn_count"], 4)
            self.assertEqual(payload["telemetry"]["turn_end_reasons"], {"tool_continuation": 3, "assistant_completed": 1})
            self.assertEqual(payload["telemetry"]["per_turn"]["avg_input_tokens"], 300)
            self.assertEqual(payload["telemetry"]["per_turn"]["avg_cache_write_tokens"], 6)

    def test_slim_omegon_results_are_labeled_om(self) -> None:
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
                '{"input_tokens": 10, "output_tokens": 5, "cache_tokens": 0}\n'
                "JSON\n"
                "fi\n"
                "exit 0\n"
            )
            fake_cargo.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: t-slim
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
                ["python3", str(SCRIPT), str(task), "--root", str(repo), "--slim"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            result_path = Path(result.stdout.strip())
            payload = json.loads(result_path.read_text())
            self.assertEqual(payload["harness"], "om")
            self.assertTrue(result_path.name.endswith("-om.json"))

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

    def test_claude_adapter_normalizes_anthropic_prefixed_model(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_claude = repo / "scripts" / "claude"
            captured = repo / "captured-model.txt"
            fake_claude.write_text(
                "#!/bin/sh\n"
                "model=''\n"
                "prev=''\n"
                "for arg in \"$@\"; do\n"
                "  if [ \"$prev\" = \"--model\" ]; then model=\"$arg\"; fi\n"
                "  prev=\"$arg\"\n"
                "done\n"
                f"printf '%s\\n' \"$model\" > \"{captured}\"\n"
                "cat <<'JSON'\n"
                '{"model":"claude-sonnet-4-6","usage":{"input_tokens":1,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}\n'
                "JSON\n"
            )
            fake_claude.chmod(0o755)
            task = self.write_task(
                repo,
                """
id: claude-model
repo: .
base_ref: main
model: anthropic:claude-sonnet-4-6
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
            self.assertEqual(captured.read_text().strip(), "claude-sonnet-4-6")

    def test_claude_adapter_rejects_non_anthropic_provider_prefixed_model(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            task = self.write_task(
                repo,
                """
id: claude-model
repo: .
base_ref: main
model: openai:gpt-4o
prompt: hi
harnesses: [claude-code]
acceptance:
  - python3 -c \"print('ok')\"
""",
            )
            result = self.run_script(str(task), "--root", str(repo), "--harness", "claude-code")
            self.assertEqual(result.returncode, 1)
            self.assertIn("do not support provider-prefixed non-Anthropic model specs", result.stderr)

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
            self.assertEqual(payload["acceptance"]["required"][0]["exit"], 0)

    def test_task_spec_slim_mode_is_used_when_cli_flag_is_absent(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_cargo = repo / "scripts" / "cargo"
            captured = repo / "captured-args.txt"
            fake_cargo.write_text(
                "#!/bin/sh\n"
                f"printf '%s\\n' \"$@\" > \"{captured}\"\n"
                "usage_json=''\n"
                "prev=''\n"
                "for arg in \"$@\"; do\n"
                "  if [ \"$prev\" = \"--usage-json\" ]; then usage_json=\"$arg\"; fi\n"
                "  prev=\"$arg\"\n"
                "done\n"
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
id: slim-task
repo: .
base_ref: main
model: anthropic:claude-sonnet-4-6
slim: true
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
            self.assertIn("--slim", captured.read_text())

    def test_cli_slim_flag_overrides_task_default_false(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            fake_cargo = repo / "scripts" / "cargo"
            captured = repo / "captured-args.txt"
            fake_cargo.write_text(
                "#!/bin/sh\n"
                f"printf '%s\\n' \"$@\" > \"{captured}\"\n"
                "usage_json=''\n"
                "prev=''\n"
                "for arg in \"$@\"; do\n"
                "  if [ \"$prev\" = \"--usage-json\" ]; then usage_json=\"$arg\"; fi\n"
                "  prev=\"$arg\"\n"
                "done\n"
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
id: slim-task
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
                ["python3", str(SCRIPT), str(task), "--root", str(repo), "--slim"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("--slim", captured.read_text())

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

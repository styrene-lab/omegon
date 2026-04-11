import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HARNESS_SCRIPT = ROOT / "scripts" / "benchmark_harness.py"
MATRIX_SCRIPT = ROOT / "scripts" / "benchmark_matrix.py"

_HARNESS_SPEC = importlib.util.spec_from_file_location("benchmark_harness_module_for_matrix_tests", HARNESS_SCRIPT)
assert _HARNESS_SPEC and _HARNESS_SPEC.loader
BENCHMARK_HARNESS = importlib.util.module_from_spec(_HARNESS_SPEC)
sys.modules[_HARNESS_SPEC.name] = BENCHMARK_HARNESS
_HARNESS_SPEC.loader.exec_module(BENCHMARK_HARNESS)

_MATRIX_SPEC = importlib.util.spec_from_file_location("benchmark_matrix_module", MATRIX_SCRIPT)
assert _MATRIX_SPEC and _MATRIX_SPEC.loader
MATRIX = importlib.util.module_from_spec(_MATRIX_SPEC)
sys.modules[_MATRIX_SPEC.name] = MATRIX
_MATRIX_SPEC.loader.exec_module(MATRIX)


@dataclass
class StubSpec:
    """Mimics the harness TaskSpec subset that expand_matrix actually reads."""
    id: str = "stub"
    harnesses: list[str] = field(default_factory=list)
    models: list[str] = field(default_factory=list)


class ExpandMatrixTests(unittest.TestCase):
    def test_basic_cross_product(self) -> None:
        spec = StubSpec(harnesses=["omegon", "pi"], models=["m1", "m2"])
        cells = MATRIX.expand_matrix(spec)
        self.assertEqual(
            [(c.harness, c.model, c.slim, c.label) for c in cells],
            [
                ("omegon", "m1", False, "omegon"),
                ("omegon", "m2", False, "omegon"),
                ("pi", "m1", False, "pi"),
                ("pi", "m2", False, "pi"),
            ],
        )

    def test_om_shorthand_translates_to_omegon_slim_with_om_label(self) -> None:
        spec = StubSpec(harnesses=["omegon", "om"], models=["m1"])
        cells = MATRIX.expand_matrix(spec)
        # Two distinct cells: omegon non-slim and omegon slim (labelled "om").
        self.assertEqual(len(cells), 2)
        plain = next(c for c in cells if not c.slim)
        slim = next(c for c in cells if c.slim)
        self.assertEqual((plain.harness, plain.label, plain.slim), ("omegon", "omegon", False))
        self.assertEqual((slim.harness, slim.label, slim.slim), ("omegon", "om", True))

    def test_no_models_yields_single_default_slot(self) -> None:
        spec = StubSpec(harnesses=["omegon"], models=[])
        cells = MATRIX.expand_matrix(spec)
        self.assertEqual(len(cells), 1)
        self.assertIsNone(cells[0].model)

    def test_restrict_filters_apply_to_normalized_harness(self) -> None:
        spec = StubSpec(harnesses=["omegon", "om", "pi"], models=["m1"])
        # Restricting to {"omegon"} should keep BOTH the plain omegon cell
        # and the om cell, since om normalizes to omegon.
        cells = MATRIX.expand_matrix(spec, restrict_harnesses={"omegon"})
        self.assertEqual({c.label for c in cells}, {"omegon", "om"})
        # Restricting to {"pi"} should drop everything else.
        cells = MATRIX.expand_matrix(spec, restrict_harnesses={"pi"})
        self.assertEqual([c.label for c in cells], ["pi"])

    def test_restrict_models_filters(self) -> None:
        spec = StubSpec(harnesses=["omegon"], models=["m1", "m2", "m3"])
        cells = MATRIX.expand_matrix(spec, restrict_models={"m2"})
        self.assertEqual([c.model for c in cells], ["m2"])

    def test_restrict_models_drops_default_only_cell(self) -> None:
        # When the spec declares no models, the default cell has model=None.
        # Restricting by name should leave the matrix empty rather than
        # silently keeping the default.
        spec = StubSpec(harnesses=["omegon"], models=[])
        cells = MATRIX.expand_matrix(spec, restrict_models={"m1"})
        self.assertEqual(cells, [])

    def test_include_slim_adds_slim_variant_alongside_plain_omegon(self) -> None:
        spec = StubSpec(harnesses=["omegon"], models=["m1"])
        cells = MATRIX.expand_matrix(spec, include_slim=True)
        self.assertEqual(len(cells), 2)
        self.assertEqual({(c.label, c.slim) for c in cells}, {("omegon", False), ("om", True)})

    def test_include_slim_does_not_duplicate_already_present_om(self) -> None:
        spec = StubSpec(harnesses=["omegon", "om"], models=["m1"])
        cells = MATRIX.expand_matrix(spec, include_slim=True)
        # Already had both via the explicit `om` shorthand; include_slim
        # must not produce a duplicate slim cell.
        self.assertEqual(len(cells), 2)
        self.assertEqual({(c.label, c.slim) for c in cells}, {("omegon", False), ("om", True)})


class SummarizeAndRenderTests(unittest.TestCase):
    def _stub_record(
        self,
        *,
        label: str,
        model: str | None,
        status: str,
        outcome: str | None = None,
        process: str | None = None,
        efficiency: str | None = None,
        discipline: str | None = None,
        tokens_total: int | None = 100,
        wall: float = 1.0,
        turns: int | None = 4,
        exit_code: int = 0,
        include_result: bool = True,
    ) -> dict:
        cell = {"harness": "omegon", "model": model, "slim": label == "om", "label": label}
        record: dict = {
            "cell": cell,
            "exit_code": exit_code,
            "wall_clock_sec": wall,
            "result_path": "/tmp/fake.json",
            "stderr_tail": [],
        }
        if include_result:
            record["result"] = {
                "status": status,
                "wall_clock_sec": wall,
                "tokens": {"total": tokens_total},
                "process": {"turn_count": turns},
                "scores": {
                    "outcome": {"status": outcome, "score": 1.0 if outcome == "pass" else 0.0},
                    "process": {"status": process, "score": None},
                    "efficiency": {"status": efficiency, "score": None},
                    "discipline": {"status": discipline, "score": None},
                },
            }
        return record

    def test_summarize_counts_pass_fail_error(self) -> None:
        records = [
            self._stub_record(label="omegon", model="m1", status="pass", outcome="pass"),
            self._stub_record(label="om", model="m1", status="fail", outcome="fail"),
            # An errored cell: subprocess succeeded its argv but no result was parsed.
            self._stub_record(
                label="pi",
                model="m1",
                status="pass",
                outcome="pass",
                include_result=False,
                exit_code=2,
            ),
        ]
        summary = MATRIX.summarize_matrix("t-stub", records)
        self.assertEqual(summary["task_id"], "t-stub")
        self.assertEqual(summary["cells_total"], 3)
        self.assertEqual(summary["cells_passed"], 1)
        self.assertEqual(summary["cells_failed"], 1)
        self.assertEqual(summary["cells_errored"], 1)
        self.assertEqual(len(summary["rows"]), 3)

    def test_render_summary_contains_header_and_rows(self) -> None:
        records = [
            self._stub_record(
                label="omegon",
                model="anthropic:claude-sonnet-4-6",
                status="pass",
                outcome="pass",
                process="pass",
                efficiency="pass",
                discipline="pass",
                tokens_total=12345,
                wall=1.5,
                turns=4,
            ),
        ]
        summary = MATRIX.summarize_matrix("t-render", records)
        text = MATRIX.render_summary(summary)
        self.assertIn("Matrix: t-render", text)
        self.assertIn("cells: 1", text)
        self.assertIn("pass: 1", text)
        self.assertIn("anthropic:claude-sonnet-4-6", text)
        # All four scoring axes should be visible in the table.
        for column in ("out", "proc", "eff", "disc"):
            self.assertIn(column, text)
        self.assertIn("12,345", text)


class MatrixRunnerIntegrationTests(unittest.TestCase):
    def setUp(self) -> None:
        # Hermetic cargo target cache per test — see the matching note in
        # tests/test_benchmark_harness.py.
        self._cache_tmpdir = tempfile.TemporaryDirectory(prefix="omegon-bench-cache-matrix-test-")
        self._saved_cache_env = os.environ.get("OMEGON_BENCHMARK_CACHE_DIR")
        os.environ["OMEGON_BENCHMARK_CACHE_DIR"] = self._cache_tmpdir.name

    def tearDown(self) -> None:
        if self._saved_cache_env is None:
            os.environ.pop("OMEGON_BENCHMARK_CACHE_DIR", None)
        else:
            os.environ["OMEGON_BENCHMARK_CACHE_DIR"] = self._saved_cache_env
        self._cache_tmpdir.cleanup()

    def init_repo(self, repo: Path) -> None:
        (repo / "ai" / "benchmarks" / "tasks").mkdir(parents=True, exist_ok=True)
        (repo / "scripts").mkdir(parents=True, exist_ok=True)
        (repo / "core").mkdir(parents=True, exist_ok=True)
        (repo / "core" / "Cargo.toml").write_text("[workspace]\n")

    def write_passing_fake_cargo(self, repo: Path) -> None:
        # The fake cargo prints the model it was launched with into the
        # usage_json so we can verify the matrix is fanning out correctly.
        fake = repo / "scripts" / "cargo"
        fake.write_text(
            "#!/bin/sh\n"
            "model=''\n"
            "slim='false'\n"
            "usage_json=''\n"
            "prev=''\n"
            "for arg in \"$@\"; do\n"
            "  if [ \"$prev\" = \"--model\" ]; then model=\"$arg\"; fi\n"
            "  if [ \"$prev\" = \"--usage-json\" ]; then usage_json=\"$arg\"; fi\n"
            "  if [ \"$arg\" = \"--slim\" ]; then slim='true'; fi\n"
            "  prev=\"$arg\"\n"
            "done\n"
            "if [ -n \"$usage_json\" ]; then\n"
            "  printf '{\"input_tokens\": 10, \"output_tokens\": 5, \"cache_tokens\": 0, \"turn_count\": 4, \"requested_model\": \"%s\", \"slim_observed\": %s, \"drift_kinds\": {}, \"progress_nudge_reasons\": {}}\\n' \"$model\" \"$slim\" > \"$usage_json\"\n"
            "fi\n"
            "exit 0\n"
        )
        fake.chmod(0o755)

    def run_matrix(self, *args: str, repo: Path) -> subprocess.CompletedProcess:
        env = dict(os.environ)
        env["PATH"] = f"{repo / 'scripts'}:{env['PATH']}"
        return subprocess.run(
            [sys.executable, str(MATRIX_SCRIPT), *args],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )

    def test_matrix_runner_fans_out_and_writes_summary(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            self.write_passing_fake_cargo(repo)
            task = repo / "task.yaml"
            task.write_text(
                """
id: t-matrix
repo: .
base_ref: main
prompt: hi
matrix:
  harnesses: [omegon]
  models: [anthropic:claude-sonnet-4-6, openai-codex:gpt-5.4]
acceptance:
  required:
    - python3 -c \"print('ok')\"
"""
            )
            out_dir = repo / "matrix-out"
            result = self.run_matrix(str(task), "--root", str(repo), "--out-dir", str(out_dir), repo=repo)

            self.assertEqual(result.returncode, 0, result.stderr)
            # Two per-cell artifacts + one matrix-summary artifact in the out dir.
            artifacts = sorted(p.name for p in out_dir.iterdir() if p.suffix == ".json")
            cell_artifacts = [a for a in artifacts if not a.startswith("matrix-")]
            matrix_artifacts = [a for a in artifacts if a.startswith("matrix-")]
            self.assertEqual(len(cell_artifacts), 2, f"expected 2 per-cell artifacts, got {artifacts}")
            self.assertEqual(len(matrix_artifacts), 1, f"expected 1 matrix artifact, got {artifacts}")

            matrix_payload = json.loads((out_dir / matrix_artifacts[0]).read_text())
            self.assertEqual(matrix_payload["schema_version"], 1)
            self.assertEqual(matrix_payload["task_id"], "t-matrix")
            summary = matrix_payload["summary"]
            self.assertEqual(summary["cells_total"], 2)
            self.assertEqual(summary["cells_passed"], 2)
            self.assertEqual(summary["cells_failed"], 0)
            self.assertEqual(summary["cells_errored"], 0)
            # Each cell should have its own model recorded.
            cell_models = {row["model"] for row in summary["rows"]}
            self.assertEqual(
                cell_models,
                {"anthropic:claude-sonnet-4-6", "openai-codex:gpt-5.4"},
            )
            # All four scoring axes should be present per row.
            for row in summary["rows"]:
                for axis in ("outcome", "process", "efficiency", "discipline"):
                    self.assertIn(axis, row)

            # Stdout should include the rendered table.
            self.assertIn("Matrix: t-matrix", result.stdout)

    def test_matrix_runner_continues_after_cell_failure(self) -> None:
        # One cell triggers a failure_if; the other passes. The matrix
        # should record both, exit with 1, and the failed cell should not
        # poison the second cell's run.
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            self.write_passing_fake_cargo(repo)
            # The failure_if predicate fires only when the per-cell harness
            # was launched with the second model, by detecting it in the
            # working tree's clean repo. Simpler approach: the failure_if
            # predicate always fires (so EVERY cell fails). That tests the
            # "every cell ran and was recorded" path even when all fail.
            # We then assert the matrix exits 1, both cells ran, both
            # produced result files, and both have status fail.
            task = repo / "task.yaml"
            task.write_text(
                """
id: t-matrix-fail
repo: .
base_ref: main
prompt: hi
matrix:
  harnesses: [omegon]
  models: [m1, m2]
acceptance:
  required:
    - python3 -c \"print('ok')\"
  failure_if:
    - python3 -c \"print('boom')\"
"""
            )
            out_dir = repo / "matrix-out"
            result = self.run_matrix(str(task), "--root", str(repo), "--out-dir", str(out_dir), repo=repo)

            # Both cells produced a result and reported `fail`, so the
            # matrix exits with 1 (failure), not 2 (error).
            self.assertEqual(result.returncode, 1, result.stderr)
            artifacts = sorted(out_dir.iterdir(), key=lambda p: p.name)
            matrix_payload = json.loads(
                next(p for p in artifacts if p.name.startswith("matrix-")).read_text()
            )
            summary = matrix_payload["summary"]
            self.assertEqual(summary["cells_total"], 2)
            self.assertEqual(summary["cells_passed"], 0)
            self.assertEqual(summary["cells_failed"], 2)
            self.assertEqual(summary["cells_errored"], 0)
            for row in summary["rows"]:
                self.assertEqual(row["status"], "fail")

    def test_matrix_runner_records_errored_cell_when_adapter_validation_fails(self) -> None:
        # claude-code adapter requires `claude` in PATH. By restricting PATH
        # to one that does not include `claude`, the per-cell harness will
        # exit 2 with no result file. The matrix runner should record the
        # cell as errored without aborting.
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            self.write_passing_fake_cargo(repo)
            task = repo / "task.yaml"
            task.write_text(
                """
id: t-matrix-error
repo: .
base_ref: main
prompt: hi
matrix:
  harnesses: [omegon, claude-code]
  models: [m1]
acceptance:
  required:
    - python3 -c \"print('ok')\"
"""
            )
            out_dir = repo / "matrix-out"
            # Constrain PATH so cargo (fake) is found but claude is not.
            env = dict(os.environ)
            env["PATH"] = f"{repo / 'scripts'}:/usr/bin:/bin"
            result = subprocess.run(
                [
                    sys.executable,
                    str(MATRIX_SCRIPT),
                    str(task),
                    "--root",
                    str(repo),
                    "--out-dir",
                    str(out_dir),
                ],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                env=env,
            )

            # cells_errored > 0 → exit 2.
            self.assertEqual(result.returncode, 2, result.stderr)
            matrix_artifact = next(
                p for p in out_dir.iterdir() if p.name.startswith("matrix-")
            )
            matrix_payload = json.loads(matrix_artifact.read_text())
            summary = matrix_payload["summary"]
            self.assertEqual(summary["cells_total"], 2)
            self.assertEqual(summary["cells_passed"], 1)
            self.assertEqual(summary["cells_errored"], 1)
            errored = next(c for c in matrix_payload["cells"] if "result" not in c)
            self.assertEqual(errored["cell"]["harness"], "claude-code")
            # Stderr tail captured for diagnosis without re-running.
            self.assertTrue(any("claude" in line for line in errored["stderr_tail"]))

    def test_matrix_runner_empty_matrix_after_filters_fails_cleanly(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            self.init_repo(repo)
            task = repo / "task.yaml"
            task.write_text(
                """
id: t-matrix-empty
repo: .
base_ref: main
prompt: hi
matrix:
  harnesses: [omegon]
  models: [m1]
acceptance:
  required:
    - echo ok
"""
            )
            result = self.run_matrix(
                str(task),
                "--root",
                str(repo),
                "--harness",
                "pi",
                repo=repo,
            )
            self.assertEqual(result.returncode, 1)
            self.assertIn("matrix is empty after filters", result.stderr)


if __name__ == "__main__":
    unittest.main()

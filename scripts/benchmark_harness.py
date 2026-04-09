#!/usr/bin/env python3
"""Lightweight token-efficiency comparison harness (Phase 1).

This runner stays deliberately small:
- loads a task spec
- validates declared harnesses
- runs a single adapter (default: first declared harness)
- executes deterministic acceptance commands
- writes a JSON result artifact

All target harnesses share the same adapter contract:
- omegon
- claude-code
- pi

Adapters may differ in telemetry richness, but not in output shape.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from shutil import copytree, which
from typing import Iterable
from typing import Any

import yaml

SUPPORTED_HARNESSES = {"omegon", "claude-code", "pi"}


class TaskSpecError(ValueError):
    pass


@dataclass
class TaskSpec:
    id: str
    repo: str
    base_ref: str
    prompt: str
    acceptance: list[str]
    harnesses: list[str]
    success_files: list[str]
    budget: dict[str, Any]
    model: str | None = None
    notes: str | None = None


@dataclass
class AdapterResult:
    model: str | None
    usage: dict[str, Any]
    extra: dict[str, Any]
    profile: str
    execution_status: str = "ok"
    error_message: str | None = None
    log_path: Path | None = None
    patch_path: Path | None = None


class AdapterError(RuntimeError):
    pass


class HarnessAdapter:
    harness_name: str

    def __init__(self, repo_path: Path, spec: TaskSpec, model: str | None, clean_repo_path: Path) -> None:
        self.repo_path = repo_path
        self.spec = spec
        self.model = model
        self.clean_repo_path = clean_repo_path

    def validate_environment(self) -> None:
        raise NotImplementedError

    def run(self) -> AdapterResult:
        raise NotImplementedError


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run a benchmark task through one harness")
    parser.add_argument("task", nargs="?", help="Path to task YAML")
    parser.add_argument("--root", default=".", help="Repo root for relative task paths")
    parser.add_argument("--harness", help="Harness to run; defaults to first declared harness")
    parser.add_argument("--model", help="Optional model override for implemented adapters")
    parser.add_argument(
        "--out-dir",
        help="Directory for JSON result artifacts (default: <root>/ai/benchmarks/runs)",
    )
    parser.add_argument(
        "--report",
        nargs="+",
        help="Print a plain-text comparison report from one or more result JSON artifacts or directories",
    )
    return parser.parse_args()


def load_task_spec(path: Path) -> TaskSpec:
    raw = yaml.safe_load(path.read_text())
    if not isinstance(raw, dict):
        raise TaskSpecError("task file must contain a YAML object")

    required = ["id", "repo", "base_ref", "prompt", "acceptance"]
    missing = [key for key in required if key not in raw]
    if missing:
        raise TaskSpecError(f"missing required fields: {', '.join(missing)}")

    harnesses = raw.get("harnesses") or ["omegon"]
    if not isinstance(harnesses, list) or not all(isinstance(v, str) for v in harnesses):
        raise TaskSpecError("harnesses must be a list of strings")
    for harness in harnesses:
        if harness not in SUPPORTED_HARNESSES:
            raise TaskSpecError(f"unsupported harness: {harness}")

    acceptance = raw["acceptance"]
    if not isinstance(acceptance, list) or not acceptance or not all(isinstance(v, str) for v in acceptance):
        raise TaskSpecError("acceptance must be a non-empty list of shell commands")

    return TaskSpec(
        id=str(raw["id"]),
        repo=str(raw["repo"]),
        base_ref=str(raw["base_ref"]),
        prompt=str(raw["prompt"]),
        acceptance=acceptance,
        harnesses=harnesses,
        success_files=[str(v) for v in raw.get("success_files", [])],
        budget=raw.get("budget") or {},
        model=str(raw["model"]) if raw.get("model") is not None else None,
        notes=str(raw["notes"]) if raw.get("notes") is not None else None,
    )


def resolve_repo_path(root: Path, spec: TaskSpec) -> Path:
    repo_path = Path(spec.repo)
    if not repo_path.is_absolute():
        repo_path = (root / repo_path).resolve()
    return repo_path


def ensure_clean_out_dir(root: Path, out_dir: str | None) -> Path:
    path = Path(out_dir).resolve() if out_dir else (root / "ai" / "benchmarks" / "runs").resolve()
    path.mkdir(parents=True, exist_ok=True)
    return path


def select_harness(spec: TaskSpec, explicit: str | None) -> str:
    harness = explicit or spec.harnesses[0]
    if harness not in spec.harnesses:
        raise TaskSpecError(f"harness '{harness}' not declared in task harnesses")
    return harness


def select_model(spec: TaskSpec, explicit: str | None) -> str | None:
    return explicit or spec.model


def prepare_clean_repo(repo_path: Path, base_ref: str) -> Path:
    clean_root = Path(tempfile.mkdtemp(prefix="benchmark-repo-"))
    if (repo_path / ".git").exists():
        subprocess.run(["git", "clone", "--quiet", "--no-checkout", str(repo_path), str(clean_root)], check=True)
        subprocess.run(["git", "checkout", "--quiet", base_ref], cwd=clean_root, check=True)
    else:
        copytree(repo_path, clean_root, dirs_exist_ok=True)
    return clean_root


class OmegonAdapter(HarnessAdapter):
    harness_name = "omegon"

    def validate_environment(self) -> None:
        cargo_toml = self.clean_repo_path / "core" / "Cargo.toml"
        if not cargo_toml.exists():
            raise AdapterError(f"omegon adapter requires {cargo_toml}")
        if which("cargo") is None:
            raise AdapterError("omegon adapter requires cargo in PATH")

    def run(self) -> AdapterResult:
        usage_file = Path(tempfile.NamedTemporaryFile(prefix="benchmark-usage-", suffix=".json", delete=False).name)
        log_file = Path(tempfile.NamedTemporaryFile(prefix="benchmark-omegon-", suffix=".log", delete=False).name)
        cmd = [
            "cargo",
            "run",
            "--manifest-path",
            str((self.clean_repo_path / "core" / "Cargo.toml").resolve()),
            "-p",
            "omegon",
            "--",
            "bench",
            "run-task",
            "--prompt",
            self.spec.prompt,
            "--usage-json",
            str(usage_file),
        ]
        if self.model:
            cmd.extend(["--model", self.model])

        with log_file.open("w") as handle:
            proc = subprocess.run(cmd, cwd=self.clean_repo_path, check=False, stdout=handle, stderr=subprocess.STDOUT, text=True)

        usage: dict[str, Any] = {}
        if usage_file.exists():
            try:
                usage = json.loads(usage_file.read_text())
            except json.JSONDecodeError:
                usage = {"raw_usage_error": "invalid json"}
        usage.setdefault("input_tokens", None)
        usage.setdefault("output_tokens", None)
        usage.setdefault("cache_tokens", None)
        usage["exit_code"] = proc.returncode

        return AdapterResult(
            model=self.model or usage.get("model") or "omegon-default",
            usage=usage,
            extra=usage.get("extra", {}),
            profile="omegon-native",
            execution_status="ok" if proc.returncode == 0 else "error",
            error_message=None if proc.returncode == 0 else f"omegon exited with code {proc.returncode}",
            log_path=log_file,
            patch_path=None,
        )


class PiAdapter(HarnessAdapter):
    harness_name = "pi"

    def validate_environment(self) -> None:
        if which("pi") is None:
            raise AdapterError("pi adapter requires 'pi' in PATH")

    def run(self) -> AdapterResult:
        log_file = Path(tempfile.NamedTemporaryFile(prefix="benchmark-pi-", suffix=".log", delete=False).name)
        cmd = [
            "pi",
            "--print",
            "--mode",
            "json",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-tools",
        ]
        if self.model:
            cmd.extend(["--model", self.model])
        cmd.append(self.spec.prompt)
        env = dict(os.environ)
        env.setdefault("PI_CODING_AGENT_DIR", str(Path.home() / ".pi" / "agent"))
        proc = subprocess.run(
            cmd,
            cwd=self.clean_repo_path,
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        log_file.write_text(proc.stdout + ("\n" if proc.stdout and proc.stderr else "") + proc.stderr)

        payload = extract_pi_result(proc.stdout)
        usage = extract_usage(payload)
        usage["exit_code"] = proc.returncode
        usage.setdefault("input_tokens", None)
        usage.setdefault("output_tokens", None)
        usage.setdefault("cache_tokens", None)
        extra = {"raw_json": payload} if payload is not None else {"raw_stdout": proc.stdout}
        model = self.model or extract_model(payload) or "pi-default"
        execution_status = "ok"
        error_message = None
        if proc.returncode != 0:
            execution_status = "error"
            error_message = f"pi exited with code {proc.returncode}"
        return AdapterResult(
            model=model,
            usage=usage,
            extra=extra,
            profile="minimal",
            execution_status=execution_status,
            error_message=error_message,
            log_path=log_file,
        )


class ClaudeCodeAdapter(HarnessAdapter):
    harness_name = "claude-code"

    def validate_environment(self) -> None:
        if which("claude") is None:
            raise AdapterError("claude-code adapter requires 'claude' in PATH")

    def run(self) -> AdapterResult:
        log_file = Path(tempfile.NamedTemporaryFile(prefix="benchmark-claude-", suffix=".log", delete=False).name)
        cmd = ["claude", "--print", "--output-format", "json", "--permission-mode", "acceptEdits"]
        if self.model:
            cmd.extend(["--model", self.model])
        cmd.append(self.spec.prompt)
        proc = subprocess.run(cmd, cwd=self.clean_repo_path, check=False, capture_output=True, text=True)
        log_file.write_text(proc.stdout + ("\n" if proc.stdout and proc.stderr else "") + proc.stderr)

        payload: dict[str, Any] | None = None
        try:
            payload = json.loads(proc.stdout) if proc.stdout.strip() else None
        except json.JSONDecodeError:
            payload = None

        usage = extract_usage(payload)
        usage["exit_code"] = proc.returncode
        usage.setdefault("input_tokens", None)
        usage.setdefault("output_tokens", None)
        usage.setdefault("cache_tokens", None)
        extra = {"raw_json": payload} if payload is not None else {"raw_stdout": proc.stdout}
        model = self.model or extract_model(payload) or "claude-code-default"
        execution_status = "ok"
        error_message = None
        if proc.returncode != 0:
            execution_status = "error"
            error_message = f"claude-code exited with code {proc.returncode}"
        elif isinstance(payload, dict) and payload.get("is_error") is True:
            execution_status = "error"
            result_text = payload.get("result")
            error_message = result_text if isinstance(result_text, str) else "claude-code reported an error result"
        return AdapterResult(
            model=model,
            usage=usage,
            extra=extra,
            profile="default",
            execution_status=execution_status,
            error_message=error_message,
            log_path=log_file,
        )


def extract_pi_result(stdout: str) -> dict[str, Any] | None:
    last_message: dict[str, Any] | None = None
    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") == "turn_end" and isinstance(event.get("message"), dict):
            last_message = event["message"]
        elif event.get("type") == "message_end" and isinstance(event.get("message"), dict):
            message = event["message"]
            if message.get("role") == "assistant":
                last_message = message
    return last_message


def extract_nested(payload: dict[str, Any] | None, *paths: tuple[str, ...]) -> Any:
    if not isinstance(payload, dict):
        return None
    for path in paths:
        current: Any = payload
        ok = True
        for part in path:
            if not isinstance(current, dict) or part not in current:
                ok = False
                break
            current = current[part]
        if ok:
            return current
    return None


def coerce_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return int(value)
    if isinstance(value, str) and value.strip().isdigit():
        return int(value.strip())
    return None


def extract_usage(payload: dict[str, Any] | None) -> dict[str, Any]:
    input_tokens = coerce_int(
        extract_nested(
            payload,
            ("input_tokens",),
            ("usage", "input_tokens"),
            ("usage", "inputTokens"),
            ("usage", "input"),
            ("usage", "prompt_tokens"),
            ("usage", "promptTokens"),
            ("result", "usage", "input_tokens"),
        )
    )
    output_tokens = coerce_int(
        extract_nested(
            payload,
            ("output_tokens",),
            ("usage", "output_tokens"),
            ("usage", "outputTokens"),
            ("usage", "output"),
            ("usage", "completion_tokens"),
            ("usage", "completionTokens"),
            ("result", "usage", "output_tokens"),
        )
    )
    cache_tokens = coerce_int(
        extract_nested(
            payload,
            ("cache_tokens",),
            ("usage", "cache_tokens"),
            ("usage", "cacheTokens"),
            ("usage", "cache_read_input_tokens"),
            ("usage", "cacheReadInputTokens"),
            ("usage", "cacheRead"),
            ("result", "usage", "cache_tokens"),
        )
    )
    cache_write_tokens = coerce_int(
        extract_nested(
            payload,
            ("usage", "cache_creation_input_tokens"),
            ("usage", "cacheCreationInputTokens"),
            ("usage", "cacheWrite"),
            ("result", "usage", "cache_creation_input_tokens"),
        )
    )
    return {
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_tokens": cache_tokens,
        "cache_write_tokens": cache_write_tokens,
    }


def extract_model(payload: dict[str, Any] | None) -> str | None:
    value = extract_nested(payload, ("model",), ("result", "model"), ("message", "model"))
    return value if isinstance(value, str) else None


def adapter_for(
    harness: str,
    repo_path: Path,
    spec: TaskSpec,
    model: str | None,
    clean_repo_path: Path,
) -> HarnessAdapter:
    if harness == "omegon":
        return OmegonAdapter(repo_path, spec, model, clean_repo_path)
    if harness == "pi":
        return PiAdapter(repo_path, spec, model, clean_repo_path)
    if harness == "claude-code":
        return ClaudeCodeAdapter(repo_path, spec, model, clean_repo_path)
    raise TaskSpecError(f"unsupported harness: {harness}")


def run_acceptance(commands: list[str], repo_path: Path) -> tuple[str, float, list[dict[str, Any]]]:
    started = time.monotonic()
    results: list[dict[str, Any]] = []
    status = "pass"
    for cmd in commands:
        proc = subprocess.run(cmd, cwd=repo_path, shell=True, check=False, capture_output=True, text=True)
        results.append(
            {
                "cmd": cmd,
                "exit": proc.returncode,
                "stdout": proc.stdout,
                "stderr": proc.stderr,
            }
        )
        if proc.returncode != 0:
            status = "fail"
            break
    return status, time.monotonic() - started, results


def compute_total_tokens(usage: dict[str, Any]) -> int | None:
    values = [
        usage.get("input_tokens"),
        usage.get("output_tokens"),
        usage.get("cache_tokens"),
        usage.get("cache_write_tokens"),
    ]
    if all(v is None for v in values):
        return None
    return int(sum(v or 0 for v in values))


def normalize_omegon_context(usage: dict[str, Any]) -> dict[str, int] | None:
    composition = usage.get("context_composition")
    if not isinstance(composition, dict):
        return None

    mapping = {
        "sys": coerce_int(composition.get("system_tokens")),
        "tools": coerce_int(composition.get("tool_schema_tokens")),
        "conv": coerce_int(composition.get("conversation_tokens")),
        "mem": coerce_int(composition.get("memory_tokens")),
        "hist": coerce_int(composition.get("tool_history_tokens")),
        "think": coerce_int(composition.get("thinking_tokens")),
        "free": coerce_int(composition.get("free_tokens")),
    }
    if all(value is None for value in mapping.values()):
        return None
    return {key: int(value or 0) for key, value in mapping.items()}


def derive_final_status(adapter: AdapterResult, acceptance_status: str) -> tuple[str, float]:
    if adapter.execution_status != "ok":
        return "error", 0.0
    if acceptance_status == "pass":
        return "pass", 1.0
    if acceptance_status == "fail":
        return "fail", 0.0
    return acceptance_status, 0.0


def build_result(
    *,
    spec: TaskSpec,
    harness: str,
    adapter: AdapterResult,
    acceptance_status: str,
    acceptance_results: list[dict[str, Any]],
    wall_clock_sec: float,
) -> dict[str, Any]:
    total_tokens = compute_total_tokens(adapter.usage)
    final_status, final_score = derive_final_status(adapter, acceptance_status)
    payload = {
        "task_id": spec.id,
        "harness": harness,
        "model": adapter.model,
        "status": final_status,
        "score": final_score,
        "wall_clock_sec": round(wall_clock_sec, 3),
        "attempts": 1,
        "benchmark_mode": {
            "clean_room": True,
            "adapter_profile": adapter.profile,
        },
        "adapter": {
            "execution_status": adapter.execution_status,
            "error_message": adapter.error_message,
        },
        "tokens": {
            "input": adapter.usage.get("input_tokens"),
            "output": adapter.usage.get("output_tokens"),
            "cache": adapter.usage.get("cache_tokens"),
            "cache_write": adapter.usage.get("cache_write_tokens"),
            "total": total_tokens,
        },
        "acceptance": {
            "commands": acceptance_results,
        },
        "artifact_paths": {
            "patch": str(adapter.patch_path) if adapter.patch_path else None,
            "log": str(adapter.log_path) if adapter.log_path else None,
        },
        "extra": adapter.extra,
    }
    omegon_context = normalize_omegon_context(adapter.usage)
    if omegon_context is not None:
        payload["omegon_context"] = omegon_context
    if adapter.usage.get("estimated_tokens") is not None:
        payload.setdefault("telemetry", {})
        payload["telemetry"]["estimated_tokens"] = adapter.usage.get("estimated_tokens")
    if adapter.usage.get("context_window") is not None:
        payload.setdefault("telemetry", {})
        payload["telemetry"]["context_window"] = adapter.usage.get("context_window")
    return payload


def write_result(out_dir: Path, spec: TaskSpec, harness: str, payload: dict[str, Any]) -> Path:
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    out_path = out_dir / f"{timestamp}-{spec.id}-{harness}.json"
    out_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return out_path


def load_result(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text())
    if not isinstance(payload, dict):
        raise TaskSpecError(f"result file must contain a JSON object: {path}")
    return payload


def fmt_tokens(value: Any) -> str:
    return "unknown" if value is None else str(value)


def fmt_seconds(value: Any) -> str:
    if isinstance(value, (int, float)):
        return f"{value:g}s"
    return "unknown"


def find_likely_excess_buckets(results: list[dict[str, Any]]) -> str | None:
    omegon = next((r for r in results if r.get("harness") == "omegon"), None)
    if not omegon:
        return None
    context = omegon.get("omegon_context")
    if not isinstance(context, dict):
        return None
    ranked = sorted(
        (
            (key, value)
            for key, value in context.items()
            if key != "free" and isinstance(value, int) and value > 0
        ),
        key=lambda item: item[1],
        reverse=True,
    )
    if not ranked:
        return None
    return " + ".join(key for key, _ in ranked[:3])


def render_report(results: list[dict[str, Any]]) -> str:
    if not results:
        raise TaskSpecError("report requires at least one result artifact")

    task_id = next((r.get("task_id") for r in results if isinstance(r.get("task_id"), str)), "unknown-task")
    lines = [f"Task: {task_id}", ""]

    for result in results:
        harness = result.get("harness", "unknown-harness")
        model = result.get("model", "unknown-model")
        status = result.get("status", "unknown")
        score = result.get("score", "unknown")
        tokens = result.get("tokens") if isinstance(result.get("tokens"), dict) else {}
        total_tokens = tokens.get("total") if isinstance(tokens, dict) else None
        wall_clock = result.get("wall_clock_sec")
        lines.append(f"- {harness} / {model}")
        lines.append(f"  status: {status}")
        lines.append(f"  score: {score}")
        lines.append(f"  total tokens: {fmt_tokens(total_tokens)}")
        lines.append(f"  wall clock: {fmt_seconds(wall_clock)}")
        omegon_context = result.get("omegon_context")
        if isinstance(omegon_context, dict):
            ordered = ["sys", "tools", "conv", "mem", "hist", "think"]
            parts = [
                f"{key} {omegon_context[key]}"
                for key in ordered
                if isinstance(omegon_context.get(key), int)
            ]
            if parts:
                lines.append(f"  omegon context: {', '.join(parts)}")
        lines.append("")

    passing = [
        r for r in results if r.get("status") == "pass" and isinstance(((r.get("tokens") or {}).get("total")), int)
    ]
    if len(passing) >= 2:
        baseline = next((result for result in passing if result.get("harness") == "omegon"), passing[0])
        challenger = next(
            (
                result
                for result in passing
                if result is not baseline and isinstance(((result.get("tokens") or {}).get("total")), int)
                and result["tokens"]["total"] > 0
            ),
            None,
        )
        if challenger is None:
            if baseline.get("harness") == "omegon" and ((baseline.get("tokens") or {}).get("total") == 0):
                lines.append("Delta")
                lines.append("- token ratio: unavailable — baseline result reported zero total tokens")
            return "\n".join(lines).rstrip() + "\n"
        base_tokens = baseline["tokens"]["total"]
        challenger_tokens = challenger["tokens"]["total"]
        if base_tokens > 0 and challenger_tokens > 0:
            ratio = base_tokens / challenger_tokens
            more_or_less = "more" if ratio >= 1.0 else "less"
            ratio_display = ratio if ratio >= 1.0 else (1 / ratio)
            lines.append("Delta")
            lines.append(
                f"- token ratio: {ratio_display:.2f}x {more_or_less} tokens for {baseline.get('harness', 'baseline')}"
            )
            likely = find_likely_excess_buckets(results)
            if likely:
                lines.append(f"- likely excess buckets: {likely}")
        elif baseline.get("harness") == "omegon" and base_tokens == 0:
            lines.append("Delta")
            lines.append("- token ratio: unavailable — baseline result reported zero total tokens")

    return "\n".join(lines).rstrip() + "\n"


def expand_report_inputs(paths: list[str]) -> list[Path]:
    expanded: list[Path] = []
    for raw_path in paths:
        path = Path(raw_path).resolve()
        if path.is_dir():
            expanded.extend(sorted(candidate for candidate in path.iterdir() if candidate.is_file() and candidate.suffix == ".json"))
        else:
            expanded.append(path)
    if not expanded:
        raise TaskSpecError("report requires at least one result artifact")
    return expanded


def group_results_for_report(results: list[dict[str, Any]]) -> Iterable[list[dict[str, Any]]]:
    grouped: dict[str, list[dict[str, Any]]] = {}
    ordered: list[str] = []
    for result in results:
        task_id = result.get("task_id") if isinstance(result.get("task_id"), str) else "unknown-task"
        if task_id not in grouped:
            grouped[task_id] = []
            ordered.append(task_id)
        grouped[task_id].append(result)
    for task_id in ordered:
        yield grouped[task_id]


def run_report_mode(paths: list[str]) -> int:
    try:
        expanded_paths = expand_report_inputs(paths)
        results = [load_result(path) for path in expanded_paths]
        report_sections = [render_report(group) for group in group_results_for_report(results)]
        print("\n".join(section.rstrip() for section in report_sections if section.strip()) + "\n", end="")
        return 0
    except (OSError, json.JSONDecodeError, TaskSpecError) as err:
        print(str(err), file=sys.stderr)
        return 1


def main() -> int:
    args = parse_args()
    if args.report:
        return run_report_mode(args.report)
    if not args.task:
        print("task is required unless --report is used", file=sys.stderr)
        return 1
    root = Path(args.root).resolve()
    task_path = Path(args.task).resolve()

    try:
        spec = load_task_spec(task_path)
        harness = select_harness(spec, args.harness)
        model = select_model(spec, args.model)
    except TaskSpecError as err:
        print(str(err), file=sys.stderr)
        return 1

    repo_path = resolve_repo_path(root, spec)
    out_dir = ensure_clean_out_dir(root, args.out_dir)
    clean_repo_path = prepare_clean_repo(repo_path, spec.base_ref)

    try:
        adapter_impl = adapter_for(harness, repo_path, spec, model, clean_repo_path)
        adapter_impl.validate_environment()
    except (TaskSpecError, AdapterError) as err:
        print(str(err), file=sys.stderr)
        return 2

    run_started = time.monotonic()
    adapter = adapter_impl.run()

    acceptance_status, acceptance_elapsed, acceptance_results = run_acceptance(
        spec.acceptance,
        clean_repo_path,
    )
    payload = build_result(
        spec=spec,
        harness=harness,
        adapter=adapter,
        acceptance_status=acceptance_status,
        acceptance_results=acceptance_results,
        wall_clock_sec=time.monotonic() - run_started,
    )
    payload.setdefault("timing", {})
    payload["timing"] = {"acceptance_wall_clock_sec": round(acceptance_elapsed, 3)}
    result_path = write_result(out_dir, spec, harness, payload)
    print(result_path)
    return 0 if payload.get("status") == "pass" else 3


if __name__ == "__main__":
    raise SystemExit(main())

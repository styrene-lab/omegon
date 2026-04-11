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
import shlex
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

# Matrix-schema aliases. The redesign doc lists `om` in the matrix example
# alongside `omegon`/`pi`/`claude-code`, but the per-cell harness only
# understands `omegon` + `--slim`. Aliases are accepted in `spec.harnesses`
# at parse time and translated at selection time so direct CLI invocation
# (`--harness om`) and matrix-orchestrated invocation both work without
# the per-cell adapter layer needing to grow a new harness identity.
HARNESS_ALIASES: dict[str, tuple[str, bool]] = {
    "om": ("omegon", True),
}


def audit(message: str) -> None:
    print(f"[benchmark] {message}", file=sys.stderr, flush=True)


class TaskSpecError(ValueError):
    pass


@dataclass
class TaskSpec:
    id: str
    kind: str
    repo: str
    base_ref: str
    prompt: str
    acceptance: list[str]
    acceptance_optional: list[str]
    acceptance_failure_if: list[str]
    harnesses: list[str]
    models: list[str]
    success_files: list[str]
    budget: dict[str, Any]
    process_expectations: dict[str, Any]
    expected_solution: dict[str, Any]
    model: str | None = None
    slim: bool = False
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

    def __init__(self, repo_path: Path, spec: TaskSpec, model: str | None, clean_repo_path: Path, slim: bool = False) -> None:
        self.repo_path = repo_path
        self.spec = spec
        self.model = model
        self.clean_repo_path = clean_repo_path
        self.slim = slim

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
    parser.add_argument("--slim", action="store_true", help="Enable Omegon slim mode for this run")
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


def _parse_acceptance(raw: Any) -> tuple[list[str], list[str], list[str]]:
    if isinstance(raw, list) and raw and all(isinstance(v, str) for v in raw):
        return list(raw), [], []
    if isinstance(raw, dict):
        required = raw.get("required") or []
        optional = raw.get("optional") or []
        failure_if = raw.get("failure_if") or []
        if not isinstance(required, list) or not required or not all(isinstance(v, str) for v in required):
            raise TaskSpecError("acceptance.required must be a non-empty list of shell commands")
        if not isinstance(optional, list) or not all(isinstance(v, str) for v in optional):
            raise TaskSpecError("acceptance.optional must be a list of shell commands")
        if not isinstance(failure_if, list) or not all(isinstance(v, str) for v in failure_if):
            raise TaskSpecError("acceptance.failure_if must be a list of shell commands")
        return list(required), list(optional), list(failure_if)
    raise TaskSpecError("acceptance must be either a non-empty list of shell commands or an object with required/optional/failure_if lists")


def _parse_matrix(raw: dict[str, Any], default_harnesses: list[str], default_model: str | None) -> tuple[list[str], list[str]]:
    matrix = raw.get("matrix")
    if matrix is None:
        models = [default_model] if default_model else []
        return default_harnesses, models
    if not isinstance(matrix, dict):
        raise TaskSpecError("matrix must be an object")
    harnesses = matrix.get("harnesses") or default_harnesses
    models = matrix.get("models") or ([default_model] if default_model else [])
    if not isinstance(harnesses, list) or not all(isinstance(v, str) for v in harnesses):
        raise TaskSpecError("matrix.harnesses must be a list of strings")
    if not isinstance(models, list) or not all(isinstance(v, str) for v in models):
        raise TaskSpecError("matrix.models must be a list of strings")
    return list(harnesses), list(models)


def load_task_spec(path: Path) -> TaskSpec:
    raw = yaml.safe_load(path.read_text())
    if not isinstance(raw, dict):
        raise TaskSpecError("task file must contain a YAML object")

    required = ["id", "repo", "base_ref", "prompt", "acceptance"]
    missing = [key for key in required if key not in raw]
    if missing:
        raise TaskSpecError(f"missing required fields: {', '.join(missing)}")

    default_harnesses = raw.get("harnesses") or ["omegon"]
    if not isinstance(default_harnesses, list) or not all(isinstance(v, str) for v in default_harnesses):
        raise TaskSpecError("harnesses must be a list of strings")

    default_model = str(raw["model"]) if raw.get("model") is not None else None
    harnesses, models = _parse_matrix(raw, list(default_harnesses), default_model)
    for harness in harnesses:
        if harness not in SUPPORTED_HARNESSES and harness not in HARNESS_ALIASES:
            raise TaskSpecError(f"unsupported harness: {harness}")

    acceptance_required, acceptance_optional, acceptance_failure_if = _parse_acceptance(raw["acceptance"])

    process_expectations = raw.get("process_expectations") or {}
    if not isinstance(process_expectations, dict):
        raise TaskSpecError("process_expectations must be an object")

    expected_solution = raw.get("expected_solution") or {}
    if not isinstance(expected_solution, dict):
        raise TaskSpecError("expected_solution must be an object")

    return TaskSpec(
        id=str(raw["id"]),
        kind=str(raw.get("kind") or "implementation"),
        repo=str(raw["repo"]),
        base_ref=str(raw["base_ref"]),
        prompt=str(raw["prompt"]),
        acceptance=acceptance_required,
        acceptance_optional=acceptance_optional,
        acceptance_failure_if=acceptance_failure_if,
        harnesses=harnesses,
        models=models,
        success_files=[str(v) for v in raw.get("success_files", [])],
        budget=raw.get("budget") or {},
        process_expectations=process_expectations,
        expected_solution=expected_solution,
        model=default_model,
        slim=bool(raw.get("slim", False)),
        notes=str(raw["notes"]) if raw.get("notes") is not None else None,
    )


def resolve_repo_path(root: Path, spec: TaskSpec) -> Path:
    repo_path = Path(spec.repo)
    if not repo_path.is_absolute():
        repo_path = (root / repo_path).resolve()
    return repo_path


def read_workspace_role(repo_path: Path) -> str | None:
    lease_path = repo_path / ".omegon" / "runtime" / "workspace.json"
    if not lease_path.exists():
        return None
    try:
        payload = json.loads(lease_path.read_text())
    except json.JSONDecodeError as err:
        raise TaskSpecError(f"could not parse workspace lease {lease_path}: {err}") from err
    role = payload.get("role")
    return role if isinstance(role, str) and role else None


def enforce_workspace_authority(repo_path: Path, spec: TaskSpec) -> None:
    role = read_workspace_role(repo_path)
    release_eval = spec.base_ref.startswith("v") or "-rc." in spec.base_ref
    if release_eval and role != "benchmark":
        raise TaskSpecError(
            f"release-evaluation benchmark runs require workspace role 'benchmark' (current: {role or 'unset'})"
        )


def ensure_clean_out_dir(root: Path, out_dir: str | None) -> Path:
    path = Path(out_dir).resolve() if out_dir else (root / "ai" / "benchmarks" / "runs").resolve()
    path.mkdir(parents=True, exist_ok=True)
    return path


def resolve_harness_alias(name: str) -> tuple[str, bool]:
    """Translate a possibly-aliased harness name into (canonical, slim_override).

    `om` → `("omegon", True)`. Unknown / non-aliased names pass through with
    `slim_override=False`. Callers must combine `slim_override` with their
    own slim source via OR — an alias never *unsets* slim that was already
    requested by the user.
    """
    if name in HARNESS_ALIASES:
        return HARNESS_ALIASES[name]
    return name, False


def select_harness(spec: TaskSpec, explicit: str | None) -> str:
    harness = explicit or spec.harnesses[0]
    if harness not in spec.harnesses:
        raise TaskSpecError(f"harness '{harness}' not declared in task harnesses")
    canonical, _slim_override = resolve_harness_alias(harness)
    return canonical


def select_model(spec: TaskSpec, explicit: str | None) -> str | None:
    return explicit or spec.model


def select_slim(spec: TaskSpec, explicit: bool, *, harness_request: str | None = None) -> bool:
    """Resolve the slim flag honoring user intent and matrix-schema aliases.

    If the requested harness is an alias that implies slim (e.g. `om`), the
    slim flag is forced on regardless of the spec/CLI state. Otherwise the
    flag is the OR of the explicit CLI flag and the spec default.
    """
    base = explicit or spec.slim
    if harness_request is not None:
        _canonical, slim_override = resolve_harness_alias(harness_request)
        if slim_override:
            return True
    return base


def normalize_model_for_harness(harness: str, model: str | None) -> str | None:
    if model is None:
        return None
    if harness in {"claude-code", "pi"} and model.startswith("anthropic:"):
        return model.split(":", 1)[1]
    return model


def ensure_model_supported_for_harness(harness: str, model: str | None) -> None:
    if model is None:
        return
    if harness in {"claude-code", "pi"} and ":" in model and not model.startswith("anthropic:"):
        raise TaskSpecError(
            f"{harness} benchmark runs do not support provider-prefixed non-Anthropic model specs: {model}"
        )


def prepare_clean_repo(repo_path: Path, base_ref: str) -> Path:
    clean_root = Path(tempfile.mkdtemp(prefix="benchmark-repo-"))
    audit(f"prepare clean repo: source={repo_path} base_ref={base_ref} target={clean_root}")
    if (repo_path / ".git").exists():
        subprocess.run(["git", "clone", "--quiet", "--no-checkout", str(repo_path), str(clean_root)], check=True)
        subprocess.run(["git", "checkout", "--quiet", base_ref], cwd=clean_root, check=True)
    else:
        copytree(repo_path, clean_root, dirs_exist_ok=True)
    audit(f"clean repo ready: {clean_root}")
    return clean_root


def benchmark_process_env(repo_path: Path, clean_repo_path: Path, harness: str, task_id: str) -> dict[str, str]:
    env = dict(os.environ)
    source_core = repo_path / "core"
    clean_core = clean_repo_path / "core"
    if source_core.exists() and clean_core.exists():
        safe_task = "".join(ch if ch.isalnum() or ch in ("-", "_") else "-" for ch in task_id)
        shared_target = source_core / "target" / "benchmark-harness" / safe_task / harness
        shared_target.mkdir(parents=True, exist_ok=True)
        env["CARGO_TARGET_DIR"] = str(shared_target.resolve())
    return env


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
        if self.slim:
            cmd.append("--slim")

        audit(
            "adapter start: "
            f"harness={self.harness_name} model={self.model or 'default'} slim={self.slim} "
            f"cwd={self.clean_repo_path} usage_json={usage_file} log={log_file}"
        )
        audit(f"adapter command: {shlex.join(cmd)}")
        with log_file.open("w") as handle:
            handle.write(f"[benchmark] adapter={self.harness_name} model={self.model or 'default'} slim={self.slim}\n")
            handle.write(f"[benchmark] cwd={self.clean_repo_path}\n")
            handle.write(f"[benchmark] command={shlex.join(cmd)}\n\n")
            handle.flush()
            proc = subprocess.run(
                cmd,
                cwd=self.clean_repo_path,
                check=False,
                stdout=handle,
                stderr=subprocess.STDOUT,
                text=True,
                env=benchmark_process_env(self.repo_path, self.clean_repo_path, self.harness_name, self.spec.id),
            )
        audit(f"adapter done: harness={self.harness_name} exit={proc.returncode} usage_json_exists={usage_file.exists()} log={log_file}")

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
        audit(
            "adapter start: "
            f"harness={self.harness_name} model={self.model or 'default'} slim={self.slim} "
            f"cwd={self.clean_repo_path} log={log_file}"
        )
        audit(f"adapter command: {shlex.join(cmd)}")
        proc = subprocess.run(
            cmd,
            cwd=self.clean_repo_path,
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        log_file.write_text(
            f"[benchmark] adapter={self.harness_name} model={self.model or 'default'} slim={self.slim}\n"
            f"[benchmark] cwd={self.clean_repo_path}\n"
            f"[benchmark] command={shlex.join(cmd)}\n\n"
            + proc.stdout
            + ("\n" if proc.stdout and proc.stderr else "")
            + proc.stderr
        )
        audit(f"adapter done: harness={self.harness_name} exit={proc.returncode} log={log_file}")

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
        audit(
            "adapter start: "
            f"harness={self.harness_name} model={self.model or 'default'} slim={self.slim} "
            f"cwd={self.clean_repo_path} log={log_file}"
        )
        audit(f"adapter command: {shlex.join(cmd)}")
        proc = subprocess.run(
            cmd,
            cwd=self.clean_repo_path,
            check=False,
            capture_output=True,
            text=True,
            env=benchmark_process_env(self.repo_path, self.clean_repo_path, self.harness_name, self.spec.id),
        )
        log_file.write_text(
            f"[benchmark] adapter={self.harness_name} model={self.model or 'default'} slim={self.slim}\n"
            f"[benchmark] cwd={self.clean_repo_path}\n"
            f"[benchmark] command={shlex.join(cmd)}\n\n"
            + proc.stdout
            + ("\n" if proc.stdout and proc.stderr else "")
            + proc.stderr
        )
        audit(f"adapter done: harness={self.harness_name} exit={proc.returncode} log={log_file}")

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
    slim: bool,
) -> HarnessAdapter:
    normalized_model = normalize_model_for_harness(harness, model)
    if harness == "omegon":
        return OmegonAdapter(repo_path, spec, normalized_model, clean_repo_path, slim=slim)
    if harness == "pi":
        return PiAdapter(repo_path, spec, normalized_model, clean_repo_path, slim=slim)
    if harness == "claude-code":
        return ClaudeCodeAdapter(repo_path, spec, normalized_model, clean_repo_path, slim=slim)
    raise TaskSpecError(f"unsupported harness: {harness}")


def run_acceptance(commands: list[str], repo_path: Path, env: dict[str, str] | None = None) -> tuple[str, float, list[dict[str, Any]]]:
    started = time.monotonic()
    results: list[dict[str, Any]] = []
    status = "pass"
    audit(f"acceptance start: cwd={repo_path} commands={len(commands)}")
    for index, cmd in enumerate(commands, start=1):
        audit(f"acceptance command {index}/{len(commands)}: {cmd}")
        proc = subprocess.run(
            cmd,
            cwd=repo_path,
            shell=True,
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        results.append(
            {
                "cmd": cmd,
                "exit": proc.returncode,
                "stdout": proc.stdout,
                "stderr": proc.stderr,
            }
        )
        audit(f"acceptance command {index} exit={proc.returncode}")
        if proc.returncode != 0:
            status = "fail"
            break
    elapsed = time.monotonic() - started
    audit(f"acceptance done: status={status} elapsed={elapsed:.3f}s")
    return status, elapsed, results


def run_optional_acceptance(
    commands: list[str],
    repo_path: Path,
    env: dict[str, str] | None = None,
) -> tuple[float, list[dict[str, Any]]]:
    """Run optional acceptance commands for diagnostic visibility.

    Optional commands are informational. Their pass/fail state is recorded but
    never gates the run's final status. All commands are executed regardless of
    earlier failures.
    """
    started = time.monotonic()
    results: list[dict[str, Any]] = []
    if commands:
        audit(f"acceptance optional start: cwd={repo_path} commands={len(commands)}")
    for index, cmd in enumerate(commands, start=1):
        audit(f"acceptance optional command {index}/{len(commands)}: {cmd}")
        proc = subprocess.run(
            cmd,
            cwd=repo_path,
            shell=True,
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        results.append(
            {
                "cmd": cmd,
                "exit": proc.returncode,
                "stdout": proc.stdout,
                "stderr": proc.stderr,
                "status": "pass" if proc.returncode == 0 else "fail",
            }
        )
        audit(f"acceptance optional command {index} exit={proc.returncode}")
    elapsed = time.monotonic() - started
    if commands:
        audit(f"acceptance optional done: elapsed={elapsed:.3f}s")
    return elapsed, results


def run_failure_if(
    commands: list[str],
    repo_path: Path,
    env: dict[str, str] | None = None,
) -> tuple[bool, float, list[dict[str, Any]]]:
    """Run failure_if predicate commands.

    Each command is treated as a forbidden-condition predicate:
      - exit 0   → the forbidden condition matched → status "triggered"
      - non-zero → the forbidden condition is clear → status "clear"

    Any triggered command causes the run to be marked as failed (failure_if
    overrides a passing required-acceptance result). All commands are executed
    regardless of earlier matches so the artifact captures every violation.
    """
    started = time.monotonic()
    results: list[dict[str, Any]] = []
    triggered = False
    if commands:
        audit(f"acceptance failure_if start: cwd={repo_path} commands={len(commands)}")
    for index, cmd in enumerate(commands, start=1):
        audit(f"acceptance failure_if command {index}/{len(commands)}: {cmd}")
        proc = subprocess.run(
            cmd,
            cwd=repo_path,
            shell=True,
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )
        cmd_status = "triggered" if proc.returncode == 0 else "clear"
        if cmd_status == "triggered":
            triggered = True
        results.append(
            {
                "cmd": cmd,
                "exit": proc.returncode,
                "stdout": proc.stdout,
                "stderr": proc.stderr,
                "status": cmd_status,
            }
        )
        audit(f"acceptance failure_if command {index} exit={proc.returncode} status={cmd_status}")
    elapsed = time.monotonic() - started
    if commands:
        audit(f"acceptance failure_if done: triggered={triggered} elapsed={elapsed:.3f}s")
    return triggered, elapsed, results


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


def derive_process_metrics(spec: TaskSpec, usage: dict[str, Any]) -> dict[str, Any]:
    turn_count = coerce_int(usage.get("turn_count"))
    dominant_phases = usage.get("dominant_phases") if isinstance(usage.get("dominant_phases"), dict) else {}
    drift_kinds = usage.get("drift_kinds") if isinstance(usage.get("drift_kinds"), dict) else {}
    progress_nudge_reasons = usage.get("progress_nudge_reasons") if isinstance(usage.get("progress_nudge_reasons"), dict) else {}
    turn_end_reasons = usage.get("turn_end_reasons") if isinstance(usage.get("turn_end_reasons"), dict) else {}
    per_turn = usage.get("per_turn") if isinstance(usage.get("per_turn"), dict) else {}

    process: dict[str, Any] = {
        "expectations": spec.process_expectations,
        "turn_count": turn_count,
        "turn_end_reasons": turn_end_reasons,
        "dominant_phases": dominant_phases,
        "drift_kinds": drift_kinds,
        "progress_nudge_reasons": progress_nudge_reasons,
    }

    derived: dict[str, Any] = {}
    if turn_count is not None:
        derived["orientation_only_turns"] = int(drift_kinds.get("orientation_churn", 0) or 0)
        derived["progress_nudge_count"] = int(sum((v or 0) for v in progress_nudge_reasons.values()))
        derived["tool_continuation_turns"] = int(turn_end_reasons.get("tool_continuation", 0) or 0)
        derived["assistant_completed_turns"] = int(turn_end_reasons.get("assistant_completed", 0) or 0)
    if per_turn:
        for key in (
            "avg_input_tokens",
            "avg_output_tokens",
            "avg_cache_tokens",
            "avg_cache_write_tokens",
            "avg_estimated_tokens",
        ):
            value = coerce_int(per_turn.get(key))
            if value is not None:
                derived[key] = value
    if derived:
        process["derived"] = derived

    process["availability"] = "full" if turn_count is not None else "none"
    process["grading"] = grade_process_expectations(spec.process_expectations, turn_count, derived)
    return process


# Process expectation keys that we know how to grade today and the
# `(actual_source, getter)` we use to resolve their actual value. Each getter
# takes (turn_count, derived) and returns an int or None.
SUPPORTED_PROCESS_EXPECTATIONS: dict[str, tuple[str, Any]] = {
    "max_turns": (
        "turn_count",
        lambda turn_count, derived: turn_count,
    ),
    "max_orientation_only_turns": (
        "derived.orientation_only_turns",
        lambda turn_count, derived: derived.get("orientation_only_turns"),
    ),
    "max_progress_nudges": (
        "derived.progress_nudge_count",
        lambda turn_count, derived: derived.get("progress_nudge_count"),
    ),
    "max_tool_continuation_turns": (
        "derived.tool_continuation_turns",
        lambda turn_count, derived: derived.get("tool_continuation_turns"),
    ),
    "max_avg_input_tokens": (
        "derived.avg_input_tokens",
        lambda turn_count, derived: derived.get("avg_input_tokens"),
    ),
}


def grade_process_expectations(
    expectations: dict[str, Any],
    turn_count: int | None,
    derived: dict[str, Any],
) -> dict[str, Any]:
    """Grade declared process expectations against the omegon adapter telemetry.

    Returns a dict of shape:
        {
          "status": "pass" | "fail" | "not_evaluated",
          "checks": [
            {
              "expectation": str,
              "threshold": Any,
              "actual": int | None,
              "actual_source": str | None,
              "status": "pass" | "fail" | "not_evaluated",
              "reason": str (only when not_evaluated)
            },
            ...
          ],
          "violations": [...subset of checks where status == "fail"...]
        }

    Numeric "max_*" expectations supported by ``SUPPORTED_PROCESS_EXPECTATIONS``
    are graded as ``actual <= threshold``. Unknown keys and keys whose required
    counter is missing emit ``not_evaluated`` with an explicit reason. Process
    grading does not gate the run's correctness status; it is a separate axis.
    """
    expectations = expectations or {}
    checks: list[dict[str, Any]] = []
    if not expectations:
        return {"status": "not_evaluated", "checks": [], "violations": []}

    derived = derived or {}
    telemetry_available = turn_count is not None
    overall_status = "pass"
    any_evaluated = False

    for key, threshold in expectations.items():
        check: dict[str, Any] = {
            "expectation": key,
            "threshold": threshold,
            "actual": None,
            "actual_source": None,
            "status": "not_evaluated",
        }
        spec_entry = SUPPORTED_PROCESS_EXPECTATIONS.get(key)
        if spec_entry is None:
            check["reason"] = "unsupported_expectation"
            checks.append(check)
            continue
        if not telemetry_available:
            check["reason"] = "process_telemetry_unavailable"
            checks.append(check)
            continue
        if not isinstance(threshold, (int, float)) or isinstance(threshold, bool):
            check["reason"] = "non_numeric_threshold"
            checks.append(check)
            continue

        source, getter = spec_entry
        actual = getter(turn_count, derived)
        check["actual_source"] = source
        if actual is None:
            check["reason"] = "actual_value_missing"
            checks.append(check)
            continue

        check["actual"] = int(actual)
        if int(actual) <= threshold:
            check["status"] = "pass"
        else:
            check["status"] = "fail"
            overall_status = "fail"
        any_evaluated = True
        checks.append(check)

    if not any_evaluated:
        overall_status = "not_evaluated"

    violations = [c for c in checks if c["status"] == "fail"]
    return {"status": overall_status, "checks": checks, "violations": violations}


# Budget keys we know how to grade today, paired with a getter that maps the
# benchmark run state to the actual value being compared. Each getter takes
# (total_tokens, input_tokens, wall_clock_sec, turn_count) and returns int|float|None.
SUPPORTED_BUDGET_KEYS: dict[str, Any] = {
    "max_turns": lambda total, inp, wall, turns: turns,
    "max_total_tokens": lambda total, inp, wall, turns: total,
    "max_input_tokens": lambda total, inp, wall, turns: inp,
    "max_wall_clock_sec": lambda total, inp, wall, turns: wall,
    "max_minutes": lambda total, inp, wall, turns: (wall / 60.0) if wall is not None else None,
}


def extract_budget_tiers(budget: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    """Return (soft, hard) budget dicts.

    Supports both the legacy flat schema (`budget: {max_turns: 20, ...}`) and
    the redesigned tiered schema (`budget: {soft: {...}, hard: {...}}`). The
    flat form is treated as soft-only (no hard ceiling).
    """
    if not isinstance(budget, dict):
        return {}, {}
    soft_raw = budget.get("soft")
    hard_raw = budget.get("hard")
    if isinstance(soft_raw, dict) or isinstance(hard_raw, dict):
        soft = soft_raw if isinstance(soft_raw, dict) else {}
        hard = hard_raw if isinstance(hard_raw, dict) else {}
        return soft, hard
    # Flat / legacy form: every key that isn't a known wrapper goes into soft.
    flat = {k: v for k, v in budget.items() if k not in {"soft", "hard"}}
    return flat, {}


def grade_efficiency_budgets(
    budget: dict[str, Any],
    total_tokens: int | None,
    input_tokens: int | None,
    wall_clock_sec: float | None,
    turn_count: int | None,
) -> dict[str, Any]:
    """Grade efficiency against the declared budget tiers.

    Per-key semantics:
        actual <= soft       → score 1.0  status "pass"
        soft  < actual ≤ hard → score 0.5 status "warn"
        actual >  hard       → score 0.0  status "fail"

    If only soft is present, "over soft" is "fail" (0.0). If only hard is
    present, the run is graded against the hard tier alone. Composite score
    is the mean of evaluated per-key scores.
    """
    soft, hard = extract_budget_tiers(budget or {})
    keys = sorted(set(soft.keys()) | set(hard.keys()))
    if not keys:
        return {"status": "not_evaluated", "score": None, "checks": []}

    checks: list[dict[str, Any]] = []
    evaluated_scores: list[float] = []
    overall_status = "pass"

    for key in keys:
        check: dict[str, Any] = {
            "key": key,
            "soft": soft.get(key),
            "hard": hard.get(key),
            "actual": None,
            "score": None,
            "status": "not_evaluated",
        }
        getter = SUPPORTED_BUDGET_KEYS.get(key)
        if getter is None:
            check["reason"] = "unsupported_budget_key"
            checks.append(check)
            continue
        soft_val = soft.get(key)
        hard_val = hard.get(key)
        if soft_val is not None and (not isinstance(soft_val, (int, float)) or isinstance(soft_val, bool)):
            check["reason"] = "non_numeric_soft"
            checks.append(check)
            continue
        if hard_val is not None and (not isinstance(hard_val, (int, float)) or isinstance(hard_val, bool)):
            check["reason"] = "non_numeric_hard"
            checks.append(check)
            continue
        actual = getter(total_tokens, input_tokens, wall_clock_sec, turn_count)
        if actual is None:
            check["reason"] = "actual_value_missing"
            checks.append(check)
            continue

        check["actual"] = actual
        if soft_val is not None and actual <= soft_val:
            check["status"] = "pass"
            check["score"] = 1.0
        elif hard_val is not None and actual <= hard_val:
            # Within hard but over (or no) soft. If no soft was declared this
            # is just "pass" rather than "warn".
            if soft_val is None:
                check["status"] = "pass"
                check["score"] = 1.0
            else:
                check["status"] = "warn"
                check["score"] = 0.5
                if overall_status == "pass":
                    overall_status = "warn"
        else:
            check["status"] = "fail"
            check["score"] = 0.0
            overall_status = "fail"

        evaluated_scores.append(check["score"])
        checks.append(check)

    if not evaluated_scores:
        return {"status": "not_evaluated", "score": None, "checks": checks}

    composite = sum(evaluated_scores) / len(evaluated_scores)
    return {"status": overall_status, "score": round(composite, 3), "checks": checks}


# Discipline scoring is intentionally a small, documented heuristic so it can
# be iterated without breaking artifact consumers. The formula version is
# emitted in the score body so any future change is visible to readers.
DISCIPLINE_FORMULA_VERSION = "v1"


def grade_discipline(turn_count: int | None, derived: dict[str, Any]) -> dict[str, Any]:
    """Heuristic discipline score derived from existing process telemetry.

    Formula v1:
        score = max(0, 1.0
                       - 0.2 * progress_nudge_count
                       - 0.2 * orientation_only_turns)

    Status thresholds (independent of the score so they degrade gracefully):
        score >= 0.8 → "pass"
        score >= 0.4 → "warn"
        score <  0.4 → "fail"

    Returns "not_evaluated" with no score when telemetry is unavailable.
    """
    if turn_count is None:
        return {
            "status": "not_evaluated",
            "score": None,
            "formula_version": DISCIPLINE_FORMULA_VERSION,
            "signals": {},
        }
    derived = derived or {}
    nudges = int(derived.get("progress_nudge_count", 0) or 0)
    churn = int(derived.get("orientation_only_turns", 0) or 0)
    raw = 1.0 - 0.2 * nudges - 0.2 * churn
    # Round before threshold comparison so the emitted score and the status
    # cannot disagree across floating-point boundaries (e.g. 1.0 - 0.4 - 0.2
    # producing 0.3999... and silently dropping to "fail" instead of "warn").
    score = round(max(0.0, raw), 3)
    if score >= 0.8:
        status = "pass"
    elif score >= 0.4:
        status = "warn"
    else:
        status = "fail"
    return {
        "status": status,
        "score": score,
        "formula_version": DISCIPLINE_FORMULA_VERSION,
        "signals": {
            "progress_nudge_count": nudges,
            "orientation_only_turns": churn,
        },
    }


def compose_scores(
    *,
    final_status: str,
    final_score: float,
    process_metrics: dict[str, Any],
    budget: dict[str, Any],
    total_tokens: int | None,
    input_tokens: int | None,
    wall_clock_sec: float | None,
    turn_count: int | None,
) -> dict[str, Any]:
    """Compose the four-axis structured score the redesign doc calls for.

    The four axes (outcome / process / efficiency / discipline) are emitted
    side-by-side. The pre-existing top-level `score` field is preserved as
    the binary outcome score; this surface adds the other three axes without
    changing existing readers.
    """
    grading = process_metrics.get("grading") if isinstance(process_metrics, dict) else None
    derived = process_metrics.get("derived") if isinstance(process_metrics, dict) else {}

    process_score: float | None
    if isinstance(grading, dict) and grading.get("status") == "pass":
        process_score = 1.0
        process_status = "pass"
    elif isinstance(grading, dict) and grading.get("status") == "fail":
        process_score = 0.0
        process_status = "fail"
    else:
        process_score = None
        process_status = "not_evaluated"

    process_axis: dict[str, Any] = {"status": process_status, "score": process_score}
    if isinstance(grading, dict):
        checks = grading.get("checks") or []
        evaluated = [c for c in checks if c.get("status") in {"pass", "fail"}]
        process_axis["supported_checks"] = len(evaluated)
        process_axis["passing_checks"] = sum(1 for c in evaluated if c.get("status") == "pass")
        process_axis["failing_checks"] = sum(1 for c in evaluated if c.get("status") == "fail")

    efficiency_axis = grade_efficiency_budgets(
        budget or {},
        total_tokens=total_tokens,
        input_tokens=input_tokens,
        wall_clock_sec=wall_clock_sec,
        turn_count=turn_count,
    )

    discipline_axis = grade_discipline(turn_count, derived if isinstance(derived, dict) else {})

    return {
        "outcome": {"status": final_status, "score": final_score},
        "process": process_axis,
        "efficiency": efficiency_axis,
        "discipline": discipline_axis,
    }


def derive_final_status(
    adapter: AdapterResult,
    acceptance_status: str,
    failure_if_triggered: bool = False,
) -> tuple[str, float]:
    if adapter.execution_status != "ok":
        return "error", 0.0
    if failure_if_triggered:
        return "fail", 0.0
    if acceptance_status == "pass":
        return "pass", 1.0
    if acceptance_status == "fail":
        return "fail", 0.0
    return acceptance_status, 0.0


def result_harness_label(harness: str, slim: bool) -> str:
    if harness == "omegon" and slim:
        return "om"
    return harness


def build_result(
    *,
    spec: TaskSpec,
    harness: str,
    slim: bool,
    adapter: AdapterResult,
    acceptance_status: str,
    acceptance_results: list[dict[str, Any]],
    optional_results: list[dict[str, Any]] | None = None,
    failure_if_results: list[dict[str, Any]] | None = None,
    failure_if_triggered: bool = False,
    wall_clock_sec: float,
) -> dict[str, Any]:
    total_tokens = compute_total_tokens(adapter.usage)
    optional_results = optional_results if optional_results is not None else []
    failure_if_results = failure_if_results if failure_if_results is not None else []
    effective_acceptance_status = acceptance_status
    if failure_if_triggered and effective_acceptance_status == "pass":
        effective_acceptance_status = "fail"
    final_status, final_score = derive_final_status(
        adapter, acceptance_status, failure_if_triggered=failure_if_triggered
    )
    payload = {
        "task_id": spec.id,
        "task_kind": spec.kind,
        "harness": result_harness_label(harness, slim),
        "model": adapter.model,
        "status": final_status,
        "score": final_score,
        "wall_clock_sec": round(wall_clock_sec, 3),
        "attempts": 1,
        "benchmark_mode": {
            "clean_room": True,
            "adapter_profile": adapter.profile,
        },
        "task": {
            "kind": spec.kind,
            "prompt": spec.prompt,
            "base_ref": spec.base_ref,
            "repo": spec.repo,
            "success_files": list(spec.success_files),
            "process_expectations": spec.process_expectations,
            "expected_solution": spec.expected_solution,
            "budgets": spec.budget,
            "matrix": {
                "harnesses": list(spec.harnesses),
                "models": list(spec.models),
            },
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
        "process": derive_process_metrics(spec, adapter.usage),
        "acceptance": {
            "status": effective_acceptance_status,
            "required_status": acceptance_status,
            "failure_if_triggered": failure_if_triggered,
            "required": acceptance_results,
            "optional": optional_results,
            "failure_if": failure_if_results,
        },
        "scores": {},  # populated below once process metrics exist
        "artifact_paths": {
            "patch": str(adapter.patch_path) if adapter.patch_path else None,
            "log": str(adapter.log_path) if adapter.log_path else None,
        },
        "extra": adapter.extra,
    }
    payload["scores"] = compose_scores(
        final_status=final_status,
        final_score=final_score,
        process_metrics=payload["process"],
        budget=spec.budget,
        total_tokens=total_tokens,
        input_tokens=adapter.usage.get("input_tokens"),
        wall_clock_sec=wall_clock_sec,
        turn_count=coerce_int(adapter.usage.get("turn_count")),
    )
    for key in ("requested_model", "requested_provider", "resolved_provider", "provider"):
        value = adapter.usage.get(key)
        if value is not None:
            payload[key] = value
    omegon_context = normalize_omegon_context(adapter.usage)
    if omegon_context is not None:
        payload["omegon_context"] = omegon_context
    if adapter.usage.get("estimated_tokens") is not None:
        payload.setdefault("telemetry", {})
        payload["telemetry"]["estimated_tokens"] = adapter.usage.get("estimated_tokens")
    if adapter.usage.get("context_window") is not None:
        payload.setdefault("telemetry", {})
        payload["telemetry"]["context_window"] = adapter.usage.get("context_window")
    if adapter.usage.get("turn_count") is not None:
        payload.setdefault("telemetry", {})
        payload["telemetry"]["turn_count"] = adapter.usage.get("turn_count")
    if isinstance(adapter.usage.get("per_turn"), dict):
        payload.setdefault("telemetry", {})
        payload["telemetry"]["per_turn"] = adapter.usage.get("per_turn")
    if isinstance(adapter.usage.get("turn_end_reasons"), dict):
        payload.setdefault("telemetry", {})
        payload["telemetry"]["turn_end_reasons"] = adapter.usage.get("turn_end_reasons")
    for key in ("dominant_phases", "drift_kinds", "progress_nudge_reasons"):
        value = adapter.usage.get(key)
        if isinstance(value, dict):
            payload[key] = value
    return payload


def _sanitize_filename_component(value: str) -> str:
    return "".join(ch if ch.isalnum() or ch in ("-", "_") else "-" for ch in value)


def write_result(out_dir: Path, spec: TaskSpec, harness: str, slim: bool, payload: dict[str, Any]) -> Path:
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    label = result_harness_label(harness, slim)
    # Include the resolved model in the filename when present so concurrent
    # matrix cells with the same (harness, slim) but different models do not
    # collide on the same second-resolution timestamp. Sanitize because model
    # ids contain ':' and other path-unsafe characters.
    model = payload.get("model")
    parts = [timestamp, spec.id, label]
    if isinstance(model, str) and model:
        parts.append(_sanitize_filename_component(model))
    out_path = out_dir / ("-".join(parts) + ".json")
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
    try:
        sys.stderr.reconfigure(line_buffering=True)
    except AttributeError:
        pass
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
        # Capture the user's pre-alias-resolution harness request so the slim
        # selector can detect alias-implied slim (e.g. `--harness om`).
        harness_request = args.harness or (spec.harnesses[0] if spec.harnesses else None)
        harness = select_harness(spec, args.harness)
        model = select_model(spec, args.model)
        slim = select_slim(spec, args.slim, harness_request=harness_request)
        ensure_model_supported_for_harness(harness, model)
    except TaskSpecError as err:
        print(str(err), file=sys.stderr)
        return 1

    repo_path = resolve_repo_path(root, spec)
    enforce_workspace_authority(repo_path, spec)
    out_dir = ensure_clean_out_dir(root, args.out_dir)
    audit(
        "benchmark start: "
        f"task={spec.id} harness={harness} model={model or 'default'} slim={slim} "
        f"repo={repo_path} out_dir={out_dir}"
    )
    clean_repo_path = prepare_clean_repo(repo_path, spec.base_ref)

    try:
        adapter_impl = adapter_for(harness, repo_path, spec, model, clean_repo_path, slim)
        adapter_impl.validate_environment()
    except (TaskSpecError, AdapterError) as err:
        print(str(err), file=sys.stderr)
        return 2

    run_started = time.monotonic()
    adapter = adapter_impl.run()

    process_env = benchmark_process_env(repo_path, clean_repo_path, harness, spec.id)
    acceptance_status, acceptance_elapsed, acceptance_results = run_acceptance(
        spec.acceptance,
        clean_repo_path,
        env=process_env,
    )
    optional_elapsed, optional_results = run_optional_acceptance(
        spec.acceptance_optional,
        clean_repo_path,
        env=process_env,
    )
    failure_if_triggered, failure_if_elapsed, failure_if_results = run_failure_if(
        spec.acceptance_failure_if,
        clean_repo_path,
        env=process_env,
    )
    payload = build_result(
        spec=spec,
        harness=harness,
        slim=slim,
        adapter=adapter,
        acceptance_status=acceptance_status,
        acceptance_results=acceptance_results,
        optional_results=optional_results,
        failure_if_results=failure_if_results,
        failure_if_triggered=failure_if_triggered,
        wall_clock_sec=time.monotonic() - run_started,
    )
    payload.setdefault("timing", {})
    payload["timing"] = {
        "acceptance_wall_clock_sec": round(acceptance_elapsed, 3),
        "acceptance_optional_wall_clock_sec": round(optional_elapsed, 3),
        "acceptance_failure_if_wall_clock_sec": round(failure_if_elapsed, 3),
    }
    result_path = write_result(out_dir, spec, harness, slim, payload)
    audit(
        "benchmark done: "
        f"task={spec.id} harness={harness} status={payload.get('status')} "
        f"wall={payload.get('wall_clock_sec')}s result={result_path}"
    )
    print(result_path)
    return 0 if payload.get("status") == "pass" else 3


if __name__ == "__main__":
    raise SystemExit(main())

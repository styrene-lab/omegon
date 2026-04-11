#!/usr/bin/env python3
"""Benchmark matrix orchestrator.

Walks the matrix declared in a task spec (matrix.harnesses × matrix.models),
fans out to ``scripts/benchmark_harness.py`` once per cell, and aggregates
the resulting per-cell JSON artifacts into a single matrix summary.

Each cell runs in its own subprocess so a single crashing cell cannot
poison the rest of the matrix. Cells run sequentially because the
per-cell harness shares ``CARGO_TARGET_DIR`` with the source tree, which
makes parallel runs unsafe today.

The redesigned matrix schema treats ``om`` as a sibling harness label
alongside ``omegon``, but the per-cell harness only knows ``omegon`` +
``--slim``. This script translates ``om`` → ``omegon`` + ``slim=True`` so
the per-cell harness does not need to grow a new harness identity.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import subprocess
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
HARNESS_SCRIPT = ROOT / "scripts" / "benchmark_harness.py"


def _load_harness_module() -> Any:
    """Import the per-cell harness so we can reuse its task-spec parser."""
    spec = importlib.util.spec_from_file_location("benchmark_harness_module", HARNESS_SCRIPT)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def audit(message: str) -> None:
    print(f"[matrix] {message}", file=sys.stderr, flush=True)


@dataclass
class Cell:
    harness: str  # one of SUPPORTED_HARNESSES in benchmark_harness.py
    model: str | None
    slim: bool
    label: str  # cell label including slim variant ("om" for omegon+slim)


def _normalize_harness(harness: str) -> tuple[str, bool]:
    """Translate matrix-level harness names into (per-cell harness, slim).

    The matrix schema uses ``om`` as a sibling label, but the per-cell
    harness only understands ``omegon`` + ``--slim``. Any other value
    passes through unchanged with ``slim=False``.
    """
    if harness == "om":
        return "omegon", True
    return harness, False


def expand_matrix(
    spec: Any,
    *,
    restrict_harnesses: set[str] | None = None,
    restrict_models: set[str] | None = None,
    include_slim: bool = False,
) -> list[Cell]:
    """Walk a task spec's declared matrix and produce a list of cells.

    ``restrict_harnesses`` / ``restrict_models``, if provided, filter the
    matrix down to a subset (matched against the *normalized* per-cell
    harness identity, so passing ``{"omegon"}`` keeps both ``omegon`` and
    ``om`` cells).

    ``include_slim`` adds an explicit slim variant alongside every regular
    omegon cell. This is useful when a task spec only declares ``omegon``
    but the operator wants to compare slim vs non-slim in the same matrix
    invocation.

    Tasks with no declared models get a single cell with ``model=None``
    (the per-cell harness will fall back to its default).
    """
    cells: list[Cell] = []
    seen: set[tuple[str, str | None, bool]] = set()
    models = list(spec.models) if spec.models else [None]

    for harness_raw in spec.harnesses:
        harness, slim = _normalize_harness(harness_raw)
        if restrict_harnesses and harness not in restrict_harnesses:
            continue
        for model in models:
            if restrict_models and (model is None or model not in restrict_models):
                continue
            label = "om" if (harness == "omegon" and slim) else harness
            key = (harness, model, slim)
            if key in seen:
                continue
            seen.add(key)
            cells.append(Cell(harness=harness, model=model, slim=slim, label=label))

            # When include_slim is set, also add the slim variant for any
            # plain omegon cell. The slim variant uses label "om" so it
            # appears as a distinct row in the summary.
            if include_slim and harness == "omegon" and not slim:
                slim_key = (harness, model, True)
                if slim_key not in seen:
                    seen.add(slim_key)
                    cells.append(Cell(harness="omegon", model=model, slim=True, label="om"))

    return cells


def run_cell(
    task_path: Path,
    root: Path,
    out_dir: Path,
    cell: Cell,
    *,
    harness_script: Path = HARNESS_SCRIPT,
    extra_env: dict[str, str] | None = None,
) -> dict[str, Any]:
    """Invoke the per-cell harness once and capture the result.

    Returns a record describing the cell, the subprocess outcome, and (if
    the per-cell harness produced a result file) the parsed result JSON.
    Cells that fail to produce a result file are recorded with the
    captured stderr tail so the operator can diagnose without re-running.
    """
    cmd: list[str] = [
        sys.executable,
        str(harness_script),
        str(task_path),
        "--root",
        str(root),
        "--out-dir",
        str(out_dir),
        "--harness",
        cell.harness,
    ]
    if cell.model:
        cmd.extend(["--model", cell.model])
    if cell.slim:
        cmd.append("--slim")

    env = dict(os.environ)
    if extra_env:
        env.update(extra_env)

    audit(f"cell start: {cell.label} model={cell.model or 'default'} slim={cell.slim}")
    started = time.monotonic()
    proc = subprocess.run(
        cmd,
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )
    elapsed = time.monotonic() - started
    audit(f"cell done: {cell.label} exit={proc.returncode} elapsed={elapsed:.3f}s")

    result_path: Path | None = None
    stdout_lines = proc.stdout.strip().splitlines() if proc.stdout else []
    if stdout_lines:
        candidate = Path(stdout_lines[-1])
        if candidate.exists():
            result_path = candidate

    record: dict[str, Any] = {
        "cell": asdict(cell),
        "exit_code": proc.returncode,
        "wall_clock_sec": round(elapsed, 3),
        "result_path": str(result_path) if result_path else None,
        "stderr_tail": (proc.stderr.strip().splitlines()[-20:] if proc.stderr else []),
    }
    if result_path is not None:
        try:
            record["result"] = json.loads(result_path.read_text())
        except (OSError, json.JSONDecodeError) as err:
            record["result_error"] = str(err)
    return record


def _row_for(cell_record: dict[str, Any]) -> dict[str, Any]:
    cell = cell_record.get("cell") or {}
    result = cell_record.get("result") or {}
    scores = result.get("scores") or {}
    tokens = result.get("tokens") or {}
    return {
        "label": cell.get("label"),
        "harness": cell.get("harness"),
        "model": cell.get("model"),
        "slim": cell.get("slim"),
        "exit_code": cell_record.get("exit_code"),
        "status": result.get("status"),
        "wall_clock_sec": result.get("wall_clock_sec"),
        "tokens_total": tokens.get("total"),
        "turn_count": (result.get("process") or {}).get("turn_count"),
        "outcome": (scores.get("outcome") or {}).get("status"),
        "outcome_score": (scores.get("outcome") or {}).get("score"),
        "process": (scores.get("process") or {}).get("status"),
        "process_score": (scores.get("process") or {}).get("score"),
        "efficiency": (scores.get("efficiency") or {}).get("status"),
        "efficiency_score": (scores.get("efficiency") or {}).get("score"),
        "discipline": (scores.get("discipline") or {}).get("status"),
        "discipline_score": (scores.get("discipline") or {}).get("score"),
        "result_path": cell_record.get("result_path"),
    }


def _cell_outcome_bucket(record: dict[str, Any]) -> str:
    """Classify a cell as 'pass' | 'fail' | 'error'.

    A cell is 'error' when the subprocess could not produce a usable
    result artifact (validation error, missing binary, parse failure).
    Cells that produced a result and reported pass/fail go into the
    matching bucket regardless of the subprocess exit code (the per-cell
    harness uses 3 to mean 'ran but failed', not 'errored').
    """
    if "result" not in record:
        return "error"
    status = (record.get("result") or {}).get("status")
    if status == "pass":
        return "pass"
    if status == "fail":
        return "fail"
    return "error"


def summarize_matrix(task_id: str, cell_records: list[dict[str, Any]]) -> dict[str, Any]:
    rows = [_row_for(rec) for rec in cell_records]
    buckets = [_cell_outcome_bucket(rec) for rec in cell_records]
    return {
        "task_id": task_id,
        "cells_total": len(cell_records),
        "cells_passed": sum(1 for b in buckets if b == "pass"),
        "cells_failed": sum(1 for b in buckets if b == "fail"),
        "cells_errored": sum(1 for b in buckets if b == "error"),
        "rows": rows,
    }


def render_summary(summary: dict[str, Any]) -> str:
    lines = [
        f"Matrix: {summary['task_id']}",
        (
            f"  cells: {summary['cells_total']}  "
            f"pass: {summary['cells_passed']}  "
            f"fail: {summary['cells_failed']}  "
            f"error: {summary['cells_errored']}"
        ),
        "",
    ]
    header = (
        f"  {'cell':<10} {'model':<32} {'out':<6} {'proc':<6} {'eff':<6} {'disc':<6} "
        f"{'turns':>6} {'tokens':>10} {'wall':>8}"
    )
    lines.append(header)
    lines.append("  " + "-" * (len(header) - 2))
    for row in summary["rows"]:
        out = row.get("outcome") or "-"
        proc = row.get("process") or "-"
        eff = row.get("efficiency") or "-"
        disc = row.get("discipline") or "-"
        tokens = row.get("tokens_total")
        tokens_s = f"{tokens:,}" if isinstance(tokens, int) else "-"
        wall = row.get("wall_clock_sec")
        wall_s = f"{wall:.1f}s" if isinstance(wall, (int, float)) else "-"
        turns = row.get("turn_count")
        turns_s = str(turns) if isinstance(turns, int) else "-"
        model = (row.get("model") or "-")[:32]
        label = (row.get("label") or "-")[:10]
        lines.append(
            f"  {label:<10} {model:<32} {out:<6} {proc:<6} {eff:<6} {disc:<6} "
            f"{turns_s:>6} {tokens_s:>10} {wall_s:>8}"
        )
    return "\n".join(lines) + "\n"


def write_matrix_artifact(
    out_dir: Path,
    spec_id: str,
    summary: dict[str, Any],
    cell_records: list[dict[str, Any]],
) -> Path:
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    safe_id = "".join(ch if ch.isalnum() or ch in ("-", "_") else "-" for ch in spec_id)
    out_path = out_dir / f"matrix-{timestamp}-{safe_id}.json"
    payload = {
        "schema_version": 1,
        "task_id": spec_id,
        "generated_at": timestamp,
        "summary": summary,
        "cells": cell_records,
    }
    out_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return out_path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Walk a task's declared matrix and aggregate per-cell results"
    )
    parser.add_argument("task", help="Path to task YAML")
    parser.add_argument("--root", default=".", help="Repo root for relative task paths")
    parser.add_argument(
        "--out-dir",
        help="Directory for per-cell and matrix artifacts (default: <root>/ai/benchmarks/runs)",
    )
    parser.add_argument(
        "--harness",
        action="append",
        help="Restrict matrix to one or more harnesses (may be passed multiple times)",
    )
    parser.add_argument(
        "--model",
        action="append",
        help="Restrict matrix to one or more models (may be passed multiple times)",
    )
    parser.add_argument(
        "--include-slim",
        action="store_true",
        help="Add an explicit omegon slim variant alongside each plain omegon cell",
    )
    parser.add_argument(
        "--summary-only",
        action="store_true",
        help="Print the summary table to stdout without writing a matrix artifact",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    harness = _load_harness_module()
    root = Path(args.root).resolve()
    task_path = Path(args.task).resolve()

    try:
        spec = harness.load_task_spec(task_path)
    except harness.TaskSpecError as err:
        print(str(err), file=sys.stderr)
        return 1

    cells = expand_matrix(
        spec,
        restrict_harnesses=set(args.harness) if args.harness else None,
        restrict_models=set(args.model) if args.model else None,
        include_slim=args.include_slim,
    )
    if not cells:
        print("matrix is empty after filters", file=sys.stderr)
        return 1

    out_dir = Path(args.out_dir).resolve() if args.out_dir else (root / "ai" / "benchmarks" / "runs").resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    audit(
        f"matrix start: task={spec.id} cells={len(cells)} root={root} out_dir={out_dir}"
    )
    cell_records: list[dict[str, Any]] = []
    for cell in cells:
        cell_records.append(run_cell(task_path, root, out_dir, cell))

    summary = summarize_matrix(spec.id, cell_records)
    if not args.summary_only:
        artifact_path = write_matrix_artifact(out_dir, spec.id, summary, cell_records)
        print(f"matrix summary: {artifact_path}")
    print(render_summary(summary), end="")

    if summary["cells_errored"] > 0:
        return 2
    if summary["cells_failed"] > 0:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Evaluate a headroom eval pack.

The runner intentionally stays stdlib-only. It materializes generated corpus data under
.tmp/headroom, runs headroom-eval, and writes a small pack result artifact.
"""
from __future__ import annotations

import json
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
TMP_PACKS = ROOT / ".tmp" / "headroom" / "packs"


def run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def require_str(obj: dict[str, Any], key: str, context: str) -> str:
    value = obj.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{context}.{key} must be a non-empty string")
    return value


def require_int(obj: dict[str, Any], key: str, context: str) -> int:
    value = obj.get(key)
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise ValueError(f"{context}.{key} must be a non-negative integer")
    return value


def load_pack(pack_dir: Path) -> dict[str, Any]:
    pack_path = pack_dir / "pack.toml"
    if not pack_path.is_file():
        raise ValueError(f"missing pack.toml: {pack_path}")
    data = tomllib.loads(pack_path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("pack.toml root must be a table")
    policy = data.get("policy")
    if not isinstance(policy, dict):
        raise ValueError("pack.toml requires [policy]")
    require_str(data, "name", "pack")
    status = require_str(data, "status", "pack")
    if status not in {"core", "community", "experimental", "deprecated"}:
        raise ValueError("pack.status must be core, community, experimental, or deprecated")
    require_str(data, "corpus_manifest", "pack")
    require_int(policy, "max_restored_facts", "policy")
    require_int(policy, "strict_max_restored_facts", "policy")
    require_int(policy, "min_evaluated_savings_percent", "policy")
    require_str(policy, "token_counter", "policy")
    return data


def evaluate_pack(pack_dir: Path, *, strict: bool) -> dict[str, Any]:
    pack = load_pack(pack_dir)
    policy = pack["policy"]
    pack_name = require_str(pack, "name", "pack")
    corpus_manifest = ROOT / require_str(pack, "corpus_manifest", "pack")
    if not corpus_manifest.is_file():
        raise ValueError(f"pack corpus_manifest does not exist: {corpus_manifest}")

    max_restored = policy["strict_max_restored_facts"] if strict else policy["max_restored_facts"]
    token_counter = policy["token_counter"]
    min_savings = policy["min_evaluated_savings_percent"]

    collect = run(["python3", "scripts/headroom_corpus_collect.py", str(corpus_manifest.relative_to(ROOT))])
    if collect.returncode != 0:
        raise RuntimeError(f"corpus collection failed:\n{collect.stderr}\n{collect.stdout}")

    generated = ROOT / ".tmp" / "headroom" / "generated-corpus.json"
    dogfood = run(["python3", "scripts/headroom_dogfood.py", str(generated.relative_to(ROOT))])
    if dogfood.returncode != 0:
        raise RuntimeError(f"dogfood fixture generation failed:\n{dogfood.stderr}\n{dogfood.stdout}")

    eval_cmd = [
        "cargo",
        "run",
        "-p",
        "omegon-headroom",
        "--bin",
        "headroom-eval",
        "--",
        "--json",
        "--fixtures",
        ".tmp/headroom/fixtures",
        "--max-restored-facts",
        str(max_restored),
        "--token-counter",
        token_counter,
    ]
    evaluated = run(eval_cmd)
    report: dict[str, Any] | None = None
    parse_error: str | None = None
    if evaluated.stdout.strip():
        try:
            loaded = json.loads(evaluated.stdout)
            if isinstance(loaded, dict):
                report = loaded.get("report") if isinstance(loaded.get("report"), dict) else loaded
            else:
                parse_error = "headroom-eval JSON root was not an object"
        except json.JSONDecodeError as exc:
            parse_error = str(exc)

    failures: list[str] = []
    if evaluated.returncode != 0:
        failures.append(f"headroom-eval exited {evaluated.returncode}")
    if parse_error:
        failures.append(f"headroom-eval JSON parse failed: {parse_error}")
    savings = None
    restored = None
    eval_passed = None
    if report is None:
        failures.append("headroom-eval report missing")
    else:
        eval_passed = bool(report.get("passed"))
        savings = report.get("evaluated_savings_percent")
        restored = report.get("restored_fact_count")
        if restored is None and isinstance(report.get("fixtures"), list):
            restored = sum(
                fixture.get("restored_fact_count", 0)
                for fixture in report["fixtures"]
                if isinstance(fixture, dict) and isinstance(fixture.get("restored_fact_count", 0), int)
            )
        if eval_passed is False:
            failures.append("headroom-eval report failed")
        if not isinstance(savings, int) or savings < min_savings:
            failures.append(f"evaluated savings {savings}% below pack minimum {min_savings}%")
        if not isinstance(restored, int) or restored > max_restored:
            failures.append(f"restored facts {restored} above pack maximum {max_restored}")

    result = {
        "pack": {
            "name": pack_name,
            "status": pack["status"],
            "path": str(pack_dir.relative_to(ROOT)),
            "strict": strict,
        },
        "policy": {
            "max_restored_facts": max_restored,
            "min_evaluated_savings_percent": min_savings,
            "token_counter": token_counter,
        },
        "status": "pass" if not failures else "fail",
        "failures": failures,
        "summary": {
            "passed": eval_passed,
            "evaluated_savings_percent": savings,
            "restored_fact_count": restored,
        },
        "commands": {
            "collect": collect.args,
            "dogfood": dogfood.args,
            "eval": eval_cmd,
        },
        "stderr": {
            "collect": collect.stderr,
            "dogfood": dogfood.stderr,
            "eval": evaluated.stderr,
        },
    }
    out_dir = TMP_PACKS / pack_name
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / ("strict-result.json" if strict else "result.json")
    out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    result["artifact_path"] = str(out_path.relative_to(ROOT))
    return result


def main() -> int:
    if len(sys.argv) not in {2, 3} or (len(sys.argv) == 3 and sys.argv[2] != "--strict"):
        print("usage: headroom_pack_eval.py PACK_DIR [--strict]", file=sys.stderr)
        return 2
    pack_dir = (ROOT / sys.argv[1]).resolve()
    if not str(pack_dir).startswith(str(ROOT)):
        print("error: PACK_DIR must be inside workspace", file=sys.stderr)
        return 2
    try:
        result = evaluate_pack(pack_dir, strict=len(sys.argv) == 3)
    except Exception as exc:  # noqa: BLE001 - CLI should report pack failures.
        print(f"error: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())

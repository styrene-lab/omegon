#!/usr/bin/env python3
"""Local sandbox smoke tests for the 0.26 evidence/substrate chain.

The suite copies the canonical evidence streams and project-rules config into a
throwaway directory, then validates that project rules and query helpers behave
as expected without mutating the working tree.
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any

CLAIM_ID = "claim:crate:omegon-tdd-savepoint:public-api-documented"


def run(cmd: list[str], *, cwd: pathlib.Path, env: dict[str, str] | None = None, check: bool = False) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, env=env, check=False)
    if check and proc.returncode != 0:
        raise AssertionError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\nSTDOUT:\n{proc.stdout}\nSTDERR:\n{proc.stderr}"
        )
    return proc


def print_step(name: str) -> None:
    print(f"\n== {name}")


def load_json(stdout: str) -> Any:
    try:
        return json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise AssertionError(f"expected JSON output, got:\n{stdout}") from exc


def copy_sandbox(root: pathlib.Path, dest: pathlib.Path) -> None:
    evidence_src = root / ".omegon" / "evidence"
    rules_src = root / ".omegon" / "project-rules.toml"
    if not evidence_src.is_dir():
        raise AssertionError(f"missing evidence source: {evidence_src}")
    if not rules_src.is_file():
        raise AssertionError(f"missing project rules config: {rules_src}")

    (dest / ".omegon").mkdir(parents=True)
    shutil.copytree(evidence_src, dest / ".omegon" / "evidence")
    shutil.copy2(rules_src, dest / ".omegon" / "project-rules.toml")


def project_rules(binary: pathlib.Path, cwd: pathlib.Path, context: str = "ci", env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return run(
        [str(binary), "--cwd", str(cwd), "project-rules", "check", "--context", context, "--json"],
        cwd=cwd,
        env=env,
    )


def assert_project_rules_pass(binary: pathlib.Path, sandbox: pathlib.Path, context: str) -> dict[str, Any]:
    proc = project_rules(binary, sandbox, context)
    if proc.returncode != 0:
        raise AssertionError(f"project-rules {context} failed unexpectedly:\n{proc.stdout}\n{proc.stderr}")
    data = load_json(proc.stdout)
    if not data.get("passed"):
        raise AssertionError(f"project-rules {context} reported passed=false:\n{json.dumps(data, indent=2)}")
    return data


def test_surfaces(binary: pathlib.Path, root: pathlib.Path) -> None:
    print_step("CLI surfaces")
    for cmd in [
        [str(binary), "--version"],
        [str(binary), "project-rules", "--help"],
        [str(binary), "project-rules", "check", "--help"],
        [str(binary), "tdd", "evidence", "--help"],
    ]:
        proc = run(cmd, cwd=root, check=True)
        first = (proc.stdout or proc.stderr).splitlines()[0]
        print(f"ok: {' '.join(cmd[1:])} -> {first}")


def test_baseline_rules(binary: pathlib.Path, sandbox: pathlib.Path) -> None:
    print_step("baseline project-rules")
    default = assert_project_rules_pass(binary, sandbox, "default")
    ci = assert_project_rules_pass(binary, sandbox, "ci")
    print(f"ok: default mode={default['mode']} passed={default['passed']}")
    print(f"ok: ci mode={ci['mode']} passed={ci['passed']}")


def test_query_helper(root: pathlib.Path, sandbox: pathlib.Path) -> None:
    print_step("evidence query helper")
    db = sandbox / ".omegon" / "evidence" / "indexes" / "evidence.sqlite"
    query = root / "scripts" / "query_evidence.py"
    for args in [
        ["claims"],
        ["search", "public-api"],
        ["get", CLAIM_ID],
        ["neighbors", CLAIM_ID],
    ]:
        proc = run([sys.executable, str(query), "--db", str(db), *args], cwd=root, check=True)
        data = load_json(proc.stdout)
        if args[0] == "claims" and not any(row.get("id") == CLAIM_ID for row in data):
            raise AssertionError(f"claim not found in claims output: {CLAIM_ID}")
        if args[0] == "neighbors" and not data.get("incoming"):
            raise AssertionError("claim has no incoming support/refutation edges")
        print(f"ok: query {' '.join(args)}")


def remove_support_edge(sandbox: pathlib.Path) -> None:
    edges = sandbox / ".omegon" / "evidence" / "edges.jsonl"
    kept: list[str] = []
    removed = 0
    for line in edges.read_text().splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        if row.get("kind") == "supports" and row.get("to") == CLAIM_ID:
            removed += 1
            continue
        kept.append(json.dumps(row, separators=(",", ":")))
    if removed == 0:
        raise AssertionError(f"no support edge found for {CLAIM_ID}")
    edges.write_text("\n".join(kept) + "\n")


def append_refutation(sandbox: pathlib.Path) -> None:
    now = int(time.time() * 1000)
    record_id = f"evidence:test-refutation:{now}"
    records = sandbox / ".omegon" / "evidence" / "records.jsonl"
    edges = sandbox / ".omegon" / "evidence" / "edges.jsonl"
    records.open("a").write(
        json.dumps(
            {
                "schema": "evidence-record/v1",
                "id": record_id,
                "provider": "sandbox-test",
                "kind": "manual-refutation",
                "status": "refutes",
                "created_at_ms": now,
                "subjects": ["crate:omegon-tdd-savepoint"],
                "claims": [CLAIM_ID],
                "artifacts": [],
                "metadata": {"reason": "synthetic refutation injected by local substrate suite"},
            },
            separators=(",", ":"),
        )
        + "\n"
    )
    edges.open("a").write(
        json.dumps(
            {
                "schema": "evidence-edge/v1",
                "from": record_id,
                "to": CLAIM_ID,
                "kind": "refutes",
                "created_at_ms": now,
            },
            separators=(",", ":"),
        )
        + "\n"
    )


def expect_rules_warns(binary: pathlib.Path, sandbox: pathlib.Path, label: str, message_fragment: str) -> None:
    proc = project_rules(binary, sandbox, "ci")
    data = load_json(proc.stdout) if proc.stdout.strip().startswith("{") else {"raw": proc.stdout}
    findings = data.get("findings", [])
    if proc.returncode != 0 or data.get("passed") is not True:
        raise AssertionError(f"expected project-rules ci to warn/pass for {label}, got rc={proc.returncode}:\n{proc.stdout}\n{proc.stderr}")
    if not any(message_fragment in finding.get("message", "") for finding in findings):
        raise AssertionError(f"expected warning containing {message_fragment!r} for {label}, got:\n{json.dumps(data, indent=2)}")
    print(f"ok: {label} produced deterministic warning while remaining non-blocking")


def expect_rules_fail(binary: pathlib.Path, sandbox: pathlib.Path, label: str) -> None:
    proc = project_rules(binary, sandbox, "ci")
    data = load_json(proc.stdout) if proc.stdout.strip().startswith("{") else {"raw": proc.stdout}
    if proc.returncode == 0 or data.get("passed") is True:
        raise AssertionError(f"expected project-rules ci to fail for {label}, got rc={proc.returncode}:\n{proc.stdout}\n{proc.stderr}")
    print(f"ok: {label} failed deterministically with rc={proc.returncode}")


def add_refuted_openspec_gate(sandbox: pathlib.Path) -> None:
    spec_dir = sandbox / "openspec" / "changes" / "sandbox-evidence-gate" / "specs"
    spec_dir.mkdir(parents=True)
    (spec_dir / "evidence.md").write_text(
        f"""# Evidence Gate — Delta Spec

## ADDED Requirements

### Requirement: Refuted evidence blocks archive

<!-- evidence-claim: {CLAIM_ID} -->

#### Scenario: Refuted claim is blocked
Given a scenario has an explicit evidence claim
When project rules evaluate active OpenSpec changes
Then refuted evidence blocks the enforced context
"""
    )
    (spec_dir.parent / "proposal.md").write_text("# Sandbox Evidence Gate\n")


def test_tamper_and_refutation(binary: pathlib.Path, root: pathlib.Path) -> None:
    print_step("tamper/refutation checks")
    with tempfile.TemporaryDirectory(prefix="omegon-evidence-unsupported-") as td:
        sandbox = pathlib.Path(td)
        copy_sandbox(root, sandbox)
        remove_support_edge(sandbox)
        expect_rules_warns(binary, sandbox, "unsupported claim", "Unsupported")
    with tempfile.TemporaryDirectory(prefix="omegon-evidence-refuted-") as td:
        sandbox = pathlib.Path(td)
        copy_sandbox(root, sandbox)
        append_refutation(sandbox)
        add_refuted_openspec_gate(sandbox)
        expect_rules_fail(binary, sandbox, "refuted OpenSpec evidence gate")


def test_auth_free_and_nex_degraded(binary: pathlib.Path, root: pathlib.Path) -> None:
    print_step("auth-free / Nex-degraded rule check")
    with tempfile.TemporaryDirectory(prefix="omegon-evidence-authfree-") as td:
        sandbox = pathlib.Path(td)
        copy_sandbox(root, sandbox)
        env = {"PATH": "/nonexistent", "HOME": os.environ.get("HOME", "")}
        for key in ["ANTHROPIC_API_KEY", "ANTHROPIC_OAUTH_TOKEN", "OPENAI_API_KEY", "CHATGPT_OAUTH_TOKEN"]:
            env.pop(key, None)
        proc = project_rules(binary, sandbox, "ci", env=env)
        if proc.returncode != 0:
            raise AssertionError(f"auth-free project-rules failed unexpectedly:\n{proc.stdout}\n{proc.stderr}")
        data = load_json(proc.stdout)
        if not data.get("passed"):
            raise AssertionError(f"auth-free project-rules did not pass:\n{json.dumps(data, indent=2)}")
        print("ok: project-rules runs without provider auth or nex on PATH")


def test_generator(root: pathlib.Path) -> None:
    print_step("generator dry-run in temporary git worktree")
    # A real generator run needs the source tree and git metadata. Use a local
    # detached worktree so generated evidence and rustdoc output do not dirty the
    # operator's active checkout.
    with tempfile.TemporaryDirectory(prefix="omegon-evidence-worktree-") as td:
        worktree = pathlib.Path(td) / "repo"
        run(["git", "worktree", "add", "--detach", str(worktree), "HEAD"], cwd=root, check=True)
        try:
            proc = run([sys.executable, "scripts/generate_rust_surface_evidence.py"], cwd=worktree)
            if proc.returncode != 0:
                raise AssertionError(f"generator failed in sandbox worktree:\n{proc.stdout}\n{proc.stderr}")
            summary = worktree / ".omegon" / "evidence" / "summaries" / "rust-doc-coverage.md"
            if not summary.is_file() or "public" not in summary.read_text().lower():
                raise AssertionError(f"generator summary missing or unexpected: {summary}")
            print("ok: generator produced rust doc coverage summary in detached worktree")
        finally:
            run(["git", "worktree", "remove", "--force", str(worktree)], cwd=root)


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", default="target/release/omegon")
    ap.add_argument("--skip-generator", action="store_true", help="Skip rustdoc generator sandbox worktree test")
    args = ap.parse_args(argv)

    root = pathlib.Path(__file__).resolve().parents[1]
    binary = (root / args.binary).resolve() if not pathlib.Path(args.binary).is_absolute() else pathlib.Path(args.binary)
    if not binary.is_file():
        raise SystemExit(f"binary not found: {binary}")

    test_surfaces(binary, root)
    with tempfile.TemporaryDirectory(prefix="omegon-evidence-baseline-") as td:
        sandbox = pathlib.Path(td)
        copy_sandbox(root, sandbox)
        test_baseline_rules(binary, sandbox)
        test_query_helper(root, sandbox)
    test_tamper_and_refutation(binary, root)
    test_auth_free_and_nex_degraded(binary, root)
    if not args.skip_generator:
        test_generator(root)
    print("\nall evidence substrate sandbox checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

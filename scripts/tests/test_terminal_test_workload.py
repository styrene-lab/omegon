#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "terminal_test_workload.py"


def run_workload(*args: str, stdin: str = "") -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        input=stdin,
        text=True,
        capture_output=True,
        timeout=5,
        check=False,
    )


def test_duration_produces_lifecycle_and_mixed_streams() -> None:
    result = run_workload(
        "--label",
        "duration-case",
        "--interval",
        "0.01",
        "--duration",
        "0.06",
        "--stderr-every",
        "2",
        "--burst-every",
        "3",
    )

    assert result.returncode == 0
    assert "kind=start label=duration-case" in result.stdout
    assert "kind=heartbeat label=duration-case" in result.stdout
    assert "kind=auto-burst label=duration-case" in result.stdout
    assert "kind=duration label=duration-case" in result.stdout
    assert "kind=stop label=duration-case" in result.stdout
    assert "kind=diagnostic label=duration-case" in result.stderr


def test_interactive_status_burst_and_quit() -> None:
    result = run_workload(
        "--interval",
        "0.01",
        "--burst-every",
        "0",
        "--stderr-every",
        "0",
        stdin="status\nburst\nquit\n",
    )

    assert result.returncode == 0
    assert "kind=status" in result.stdout
    assert "stdin=responsive" in result.stdout
    assert result.stdout.count("kind=burst") == 8
    assert "kind=command" in result.stdout
    assert "value=quit" in result.stdout


def test_fail_command_returns_distinct_exit_code() -> None:
    result = run_workload("--interval", "0.01", stdin="fail\n")

    assert result.returncode == 23
    assert "kind=command" in result.stderr
    assert "value=fail exit_code=23" in result.stderr
    assert "kind=stop" in result.stdout
    assert "exit_code=23" in result.stdout


if __name__ == "__main__":
    tests = [value for name, value in sorted(globals().items()) if name.startswith("test_")]
    for test in tests:
        test()
        print(f"ok {test.__name__}")

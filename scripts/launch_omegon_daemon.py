#!/usr/bin/env python3
"""Reference launcher for Auspex-style Omegon daemon startup.

Spawns `omegon serve`, reads the first stdout startup event, fetches
`/api/startup`, polls `/api/readyz`, and prints discovered metadata as JSON.

This is a reference implementation of docs/auspex-omegon-launch-contract.md,
not a production supervisor.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


class LaunchError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Launch a long-running Omegon daemon and emit startup metadata")
    parser.add_argument("--binary", default="omegon", help="Path to omegon binary (default: omegon on PATH)")
    parser.add_argument("--cwd", required=True, help="Working directory for the daemon instance")
    parser.add_argument("--model", help="Explicit provider:model route")
    parser.add_argument("--control-port", type=int, default=7842, help="Preferred localhost control port")
    parser.add_argument("--strict-port", action="store_true", help="Require exact control port")
    parser.add_argument("--log-level", default="info", help="Omegon log level")
    parser.add_argument("--log-file", help="Optional omegon log file path")
    parser.add_argument("--startup-timeout", type=float, default=10.0, help="Seconds to wait for startup metadata")
    parser.add_argument("--ready-timeout", type=float, default=10.0, help="Seconds to wait for readyz")
    parser.add_argument(
        "--set-env",
        action="append",
        default=[],
        metavar="KEY=VALUE",
        help="Extra env vars for the daemon process (repeatable)",
    )
    return parser.parse_args()


def build_command(args: argparse.Namespace) -> list[str]:
    command = [
        args.binary,
        "--cwd",
        args.cwd,
        "--log-level",
        args.log_level,
    ]
    if args.model:
        command.extend(["--model", args.model])
    if args.log_file:
        command.extend(["--log-file", args.log_file])
    command.extend([
        "serve",
        "--control-port",
        str(args.control_port),
    ])
    if args.strict_port:
        command.append("--strict-port")
    return command


def apply_env_overrides(base_env: dict[str, str], overrides: list[str]) -> dict[str, str]:
    env = dict(base_env)
    for item in overrides:
        if "=" not in item:
            raise LaunchError(f"invalid --set-env entry (expected KEY=VALUE): {item}")
        key, value = item.split("=", 1)
        if not key:
            raise LaunchError(f"invalid --set-env entry with empty key: {item}")
        env[key] = value
    return env


def read_startup_event(proc: subprocess.Popen[str], timeout_secs: float) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_secs
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise LaunchError(f"omegon exited before startup event (code {proc.returncode})")
        assert proc.stdout is not None
        line = proc.stdout.readline()
        if line:
            try:
                event = json.loads(line)
            except json.JSONDecodeError as exc:
                raise LaunchError(f"failed to parse startup JSON: {line!r}") from exc
            if event.get("type") != "omegon.startup":
                raise LaunchError(f"unexpected startup event type: {event}")
            return event
        time.sleep(0.05)
    raise LaunchError("timed out waiting for startup event")


def fetch_json(url: str) -> tuple[int, dict[str, Any]]:
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=5) as response:
        status = response.status
        payload = json.loads(response.read().decode("utf-8"))
        if not isinstance(payload, dict):
            raise LaunchError(f"expected JSON object from {url}, got: {payload!r}")
        return status, payload


def wait_for_startup_payload(url: str, timeout_secs: float) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_secs
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            status, payload = fetch_json(url)
            if 200 <= status < 300:
                return payload
            last_error = LaunchError(f"startup returned HTTP {status}")
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, LaunchError) as exc:
            last_error = exc
        time.sleep(0.1)
    raise LaunchError(f"timed out waiting for /api/startup: {last_error}")


def wait_for_ready(url: str, timeout_secs: float) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_secs
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            status, payload = fetch_json(url)
            if 200 <= status < 300 and payload.get("ok") is True:
                return payload
            last_error = LaunchError(f"ready probe not ready: status={status} payload={payload}")
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, LaunchError) as exc:
            last_error = exc
        time.sleep(0.1)
    raise LaunchError(f"timed out waiting for /api/readyz: {last_error}")


def terminate_process(proc: subprocess.Popen[str]) -> None:
    if proc.poll() is not None:
        return
    try:
        proc.send_signal(signal.SIGTERM)
        proc.wait(timeout=2)
    except Exception:
        proc.kill()
        proc.wait(timeout=2)


def main() -> int:
    args = parse_args()
    cwd = Path(args.cwd).expanduser().resolve()
    if not cwd.is_dir():
        raise LaunchError(f"cwd is not a directory: {cwd}")

    command = build_command(args)
    env = apply_env_overrides(os.environ, args.set_env)

    proc = subprocess.Popen(
        command,
        cwd=str(cwd),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )

    try:
        startup_event = read_startup_event(proc, args.startup_timeout)
        startup_payload = wait_for_startup_payload(startup_event["startup_url"], args.startup_timeout)
        ready_payload = wait_for_ready(startup_event["ready_url"], args.ready_timeout)

        result = {
            "pid": proc.pid,
            "command": command,
            "startup_event": startup_event,
            "startup_payload": startup_payload,
            "ready_payload": ready_payload,
        }
        json.dump(result, sys.stdout, indent=2)
        sys.stdout.write("\n")
        sys.stdout.flush()
        return 0
    except Exception:
        terminate_process(proc)
        raise


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except LaunchError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)

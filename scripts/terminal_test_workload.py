#!/usr/bin/env python3
"""Deterministic long-running workload for exercising managed terminal sessions.

The workload emits timestamped stdout/stderr heartbeats, accepts interactive
commands over stdin, handles termination signals, and can exit naturally or
fail on demand. It is intentionally dependency-free.
"""

from __future__ import annotations

import argparse
import os
import select
import signal
import sys
import time
from dataclasses import dataclass
from typing import TextIO


@dataclass
class State:
    started: float
    sequence: int = 0
    stop_requested: bool = False
    exit_code: int = 0


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", default="omegon-terminal-test", help="label included in every record")
    parser.add_argument("--interval", type=float, default=1.0, help="seconds between heartbeats")
    parser.add_argument("--duration", type=float, default=0.0, help="stop after N seconds; 0 runs until stopped")
    parser.add_argument("--burst-every", type=int, default=10, help="emit a multi-line burst every N heartbeats; 0 disables")
    parser.add_argument("--stderr-every", type=int, default=5, help="emit a stderr record every N heartbeats; 0 disables")
    args = parser.parse_args(argv)
    if args.interval <= 0:
        parser.error("--interval must be greater than zero")
    if args.duration < 0:
        parser.error("--duration cannot be negative")
    if args.burst_every < 0 or args.stderr_every < 0:
        parser.error("--burst-every and --stderr-every cannot be negative")
    return args


def emit(stream: TextIO, kind: str, label: str, state: State, detail: str) -> None:
    elapsed = time.monotonic() - state.started
    print(
        f"TERMINAL_TEST kind={kind} label={label} pid={os.getpid()} "
        f"seq={state.sequence} elapsed={elapsed:.3f} {detail}",
        file=stream,
        flush=True,
    )


def handle_command(command: str, label: str, state: State) -> None:
    normalized = command.strip().lower()
    if not normalized:
        return
    if normalized in {"quit", "exit"}:
        emit(sys.stdout, "command", label, state, "value=quit")
        state.stop_requested = True
    elif normalized == "fail":
        emit(sys.stderr, "command", label, state, "value=fail exit_code=23")
        state.exit_code = 23
        state.stop_requested = True
    elif normalized == "burst":
        for index in range(8):
            emit(sys.stdout, "burst", label, state, f"line={index + 1}/8 payload={'#' * (index + 1)}")
    elif normalized == "status":
        emit(sys.stdout, "status", label, state, "state=running stdin=responsive")
    elif normalized == "help":
        emit(sys.stdout, "help", label, state, "commands=help,status,burst,fail,quit")
    else:
        emit(sys.stderr, "unknown-command", label, state, f"value={normalized!r}")


def poll_stdin(label: str, state: State) -> None:
    try:
        readable, _, _ = select.select([sys.stdin], [], [], 0)
    except (OSError, ValueError):
        return
    if readable:
        line = sys.stdin.readline()
        if line:
            handle_command(line, label, state)


def run(args: argparse.Namespace) -> int:
    state = State(started=time.monotonic())

    def request_stop(signum: int, _frame: object) -> None:
        emit(sys.stderr, "signal", args.label, state, f"number={signum}")
        state.stop_requested = True

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)

    emit(
        sys.stdout,
        "start",
        args.label,
        state,
        f"interval={args.interval} duration={args.duration} cwd={os.getcwd()!r}",
    )
    emit(sys.stdout, "help", args.label, state, "commands=help,status,burst,fail,quit")

    next_heartbeat = state.started
    while not state.stop_requested:
        now = time.monotonic()
        if args.duration and now - state.started >= args.duration:
            emit(sys.stdout, "duration", args.label, state, "state=complete")
            break
        poll_stdin(args.label, state)
        if now >= next_heartbeat:
            state.sequence += 1
            emit(sys.stdout, "heartbeat", args.label, state, "state=running")
            if args.stderr_every and state.sequence % args.stderr_every == 0:
                emit(sys.stderr, "diagnostic", args.label, state, "stream=stderr level=notice")
            if args.burst_every and state.sequence % args.burst_every == 0:
                for index in range(4):
                    emit(sys.stdout, "auto-burst", args.label, state, f"line={index + 1}/4")
            next_heartbeat += args.interval
        time.sleep(min(0.05, args.interval / 2))

    emit(sys.stdout, "stop", args.label, state, f"exit_code={state.exit_code}")
    return state.exit_code


def main() -> int:
    return run(parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

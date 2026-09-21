#!/usr/bin/env python3
"""Drive the actual Omegon TUI through a private tmux server; no paid inference."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import uuid

FIXTURE_MODEL = "openai:omegon-tui-fixture"


def tui_command(binary, workspace, log, presentation="fullscreen", detail="active", *, unconfigured=False):
    command = [str(binary), "--cwd", str(workspace), "--no-splash", "--fresh", "--log-level", "debug", "--log-file", str(log)]
    if not unconfigured:
        command += ["--model", FIXTURE_MODEL]
    if presentation is not None:
        command += ["--tui", presentation]
    if detail is not None:
        command += ["--ui", detail]
    return command


def ready_marker(presentation, detail):
    # Composer visibility alone does not prove idle: reply_ready combines this
    # marker with authority closure, live-status absence, and publication count.
    return "Ask anything" if presentation == "inline" or detail == "full" else "ready · idle"


def authority_runtime_idle(records):
    started = {record["payload"]["turn_id"] for record in records if record["event_type"] == "turn.started"}
    closed = {record["payload"]["turn_id"] for record in records if record["event_type"] == "turn.closed"}
    return bool(started) and started <= closed


def reply_ready(current, transcript, marker, presentation, detail, *, runtime_idle):
    # The empty composer placeholder is visible during streaming too. Require
    # durable terminalization and its rendered idle state, including publication.
    return (runtime_idle and ready_marker(presentation, detail) in current
            and "Working ·" not in current
            and "Publishing completed output" not in current
            and transcript.count(marker) == 1)


@contextmanager
def fixture_provider():
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            if self.path == "/v1/models":
                body = {"data": [{"id": "gpt-5.4", "object": "model"}]}
            elif self.path == "/api/tags":
                body = {"models": [{"name": "gpt-5.4"}]}
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.end_headers()
            self.wfile.write(json.dumps(body).encode())

        def do_POST(self):
            self.connection.settimeout(3)
            length = int(self.headers.get("Content-Length", "0"))
            if self.path != "/v1/chat/completions" or not 0 <= length <= 8 * 1024 * 1024:
                self.send_error(400)
                return
            json.loads(self.rfile.read(length))
            with server.request_lock:
                server.requests += 1
                number = server.requests
            if server.stress and number == 5:
                server.cancel_waiting.set()
                if not server.release_cancel.wait(timeout=60):
                    self.send_error(504)
                    return
            tool_probe = number == 3 and server.tool_path is not None
            if tool_probe:
                server.tool_waiting.set()
                if not server.release_tool.wait(timeout=60):
                    self.send_error(504)
                    return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            reply = f"TUI_FIXTURE_REPLY_{number}"
            if server.tool_path is not None and number >= 4:
                reply += (" The operator denied the requested write. The fixture has completed its permission check "
                          "and will make no further tool calls. The requested file remains absent, the prior project "
                          "surface is preserved, and control returns to the conversation for the next operator prompt.")
            if server.stress and number == 1:
                reply = f"TUI_FIXTURE_REPLY_{number} " + "bounded-output 界é " * 5000
            deltas = [({"content": reply}, None), ({}, "stop")]
            if tool_probe:
                call = {"index": 0, "id": "fixture-denied-write", "type": "function",
                        "function": {"name": "write", "arguments": json.dumps({"path": server.tool_path, "content": "fixture only"})}}
                deltas = [({"tool_calls": [call]}, None), ({}, "tool_calls")]
            try:
                for delta, finish in deltas:
                    event = {"choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
                    self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                    self.wfile.flush()
                    if server.stress and number == 1 and finish is None:
                        server.stream_waiting.set()
                        if not server.release_stream.wait(timeout=60):
                            return
                self.wfile.write(b"data: [DONE]\n\n")
            except (BrokenPipeError, ConnectionResetError):
                # The cancellation fixture deliberately disconnects before release.
                if not (server.stress and number == 5):
                    raise


    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.stress = False
    server.stream_waiting = threading.Event()
    server.release_stream = threading.Event()
    server.cancel_waiting = threading.Event()
    server.release_cancel = threading.Event()
    server.requests = 0
    server.request_lock = threading.Lock()
    server.tool_path = None
    server.tool_waiting = threading.Event()
    server.release_tool = threading.Event()
    server.url = f"http://127.0.0.1:{server.server_port}"
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server
    finally:
        server.release_tool.set()
        server.release_stream.set()
        server.release_cancel.set()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def assert_quiet_startup(text, entry="omegon"):
    summaries = [line for line in text.splitlines() if f"{entry} · {FIXTURE_MODEL}" in line]
    assert len(summaries) == 1, f"expected one resolved route summary, got {summaries}"
    assert "/connect" in summaries[0]
    for old_output in ("(none)", "Suggested routes:", "No LLM provider detected", "Git:"):
        assert old_output not in text, f"startup still dumps bootstrap/provider diagnostics: {old_output}"


def assert_unconfigured_startup(text):
    assert "Choose a connection" in text, "startup does not expose its unconfigured state"
    for phantom in ("anthropic", "claude-sonnet", "thinking ", " context", "ctx:",
                    "No LLM provider available", "Fabricator", "Architect", "Explorator", "Devastator"):
        assert phantom not in text, f"unconfigured startup exposes stale route/setup information: {phantom}"


def assert_unconfigured_draft(text, draft):
    assert text.count(draft) == 1, "cancelled connection must preserve one unsent draft"
    assert "No LLM provider available" not in text, "disconnected draft reached the null provider"
    assert "TUI_FIXTURE_REPLY" not in text, "disconnected draft reached inference"


def assert_unconfigured_session(root):
    journals = sorted(root.rglob("*.authority.jsonl"))
    assert journals, "no session authority evidence found"
    events = [json.loads(line)["event_type"] for path in journals for line in path.read_text().splitlines() if line]
    for forbidden in ("prompt.admitted", "turn.started", "model.request_prepared"):
        assert forbidden not in events, f"unconfigured draft entered session execution: {forbidden}"
    return {"journals": [{"file": str(path.relative_to(root)), "sha256": digest(path)} for path in journals],
            "event_types": events, "inference_requests": 0, "conversation_turns": 0}


def prepare_unconfigured_workspace(root):
    workspace = root / "workspace"
    workspace.mkdir()
    return workspace


def tui_environment(root, *, fresh_install=False, unconfigured=False):
    # Never inherit operator credentials, config locations, or launcher child state.
    environment = {"PATH": os.environ["PATH"], "HOME": str(root), "OMEGON_HOME": str(root / "omegon-home"),
                   "XDG_CONFIG_HOME": str(root / ".config"), "TERM": "xterm-256color", "LANG": "en_US.UTF-8",
                   "NO_COLOR": "1"}
    if not unconfigured:
        environment.update({"OPENAI_API_KEY": "local-only",
                            "OMEGON_PROJECT_ENDPOINT_616363657074616E6365_TOKEN": "local-only"})
    if not (fresh_install or unconfigured):
        environment["OMEGON_CHILD"] = "1"
    return environment


def group_exists(group: int) -> bool:
    # Existence and signal permission are distinct. Read the process table;
    # reserve signals for cleanup of the invocation's owned group.
    groups = subprocess.check_output(["ps", "-axo", "pgid="], text=True, timeout=5)
    return str(group) in groups.split()


def prepare_fixture_workspace(root, provider):
    workspace = root / "workspace"
    config = workspace / ".omegon"
    config.mkdir(parents=True)
    provider.tool_path = str(root / "outside-project" / "denied.txt")
    (config / "profile.json").write_text(json.dumps({"permissions": {"tools": {"write": "prompt"}}}) + "\n")
    (config / "inference.toml").write_text(f'''schema_version = 1
[[endpoints]]
id = "acceptance"
adapter = "chat-completions"
secret_refs = ["OMEGON_PROJECT_ENDPOINT_616363657074616E6365_TOKEN"]
[endpoints.transport]
kind = "http"
base_url = "{provider.url}/v1"
[[offerings]]
id = "{FIXTURE_MODEL}"
endpoint = "acceptance"
native_model_id = "local"
input_modalities = ["text"]
output_modalities = ["text"]
[offerings.capabilities]
tools = true
reasoning = true
''')
    return workspace


def run(binary: Path, output: Path, presentation="fullscreen", detail="active", entry=None, stress=False, fresh_install=False, unconfigured=False):
    if unconfigured and stress:
        raise ValueError("unconfigured acceptance cannot run provider stress turns")
    binary = binary.resolve(strict=True)
    checkout = Path(__file__).resolve().parents[1]
    if output.resolve().is_relative_to(checkout):
        raise ValueError("capture evidence must be outside the checkout")
    output.mkdir(parents=True, exist_ok=False)
    socket = "omegon-acceptance-" + uuid.uuid4().hex
    ledger = {"binary": str(binary), "binary_sha256": digest(binary), "started": time.time(),
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip(),
              "dirty": subprocess.check_output(["git", "status", "--porcelain"], cwd=checkout, text=True),
              "captures": [], "passed": False, "tui": presentation, "ui": detail, "entry": entry, "stress": stress, "fresh_install": fresh_install,
              "unconfigured": unconfigured, "terminal_owner": "private-headless-tmux", "gui_windows_created": 0}

    def tmux(*args, check=True):
        return subprocess.run(["tmux", "-L", socket, *args], check=check, capture_output=True, text=True, timeout=10).stdout

    def action(*args):
        ledger.setdefault("actions", []).append({"time": time.time(), "tmux": args})
        return tmux(*args)

    def screen():
        return tmux("capture-pane", "-p", "-t", "run:0.0")

    def history():
        return tmux("capture-pane", "-p", "-S", "-", "-t", "run:0.0") if presentation == "inline" else screen()

    def capture(name, *, primary=False):
        path = output / (name + ".txt")
        path.write_text((history() if presentation == "inline" else tmux("capture-pane", "-p", "-a", "-t", "run:0.0")) if primary else screen())
        ledger["captures"].append({"name": name, "time": time.time(), "sha256": digest(path),
                                   "geometry": tmux("display-message", "-p", "-t", "run:0.0", "#{pane_width}x#{pane_height}").strip()})

    def wait_for(predicate, label):
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.05)
        capture("failure")
        raise TimeoutError(label)

    with tempfile.TemporaryDirectory(prefix="omegon-tui-") as temporary, fixture_provider() as provider:
        root = Path(temporary)
        provider.stress = stress
        workspace = prepare_unconfigured_workspace(root) if unconfigured else prepare_fixture_workspace(root, provider)
        if fresh_install and not unconfigured:
            (workspace / ".omegon/profile.json").unlink()
        log = output / "omegon.log"

        def turn_settled(number):
            records = []
            for journal in root.rglob("*.authority.jsonl"):
                contents = journal.read_text()
                # A writer may still be finishing the last record. Never infer
                # idle from a partial journal observation.
                if contents and not contents.endswith("\n"):
                    return False
                records.extend(json.loads(line) for line in contents.splitlines() if line)
            return reply_ready(screen(), history(), f"TUI_FIXTURE_REPLY_{number}", presentation, detail,
                               runtime_idle=authority_runtime_idle(records))

        executable = binary
        if entry:
            executable = root / entry
            shutil.copy2(checkout / "scripts/omegon-launcher.sh", executable)
            executable.chmod(0o755)
        command = tui_command(executable, workspace, log, None if entry else presentation, None if entry else detail, unconfigured=unconfigured)
        # Start with an explicit environment, so real credentials/plugins cannot leak into the fixture.
        environment = tui_environment(root, fresh_install=fresh_install, unconfigured=unconfigured)
        if entry:
            environment["OMEGON_BIN"] = str(binary)
        launch = ["env", "-i", *(f"{key}={value}" for key, value in environment.items()), *command]
        ledger["command"] = command
        ledger["environment_keys"] = sorted(environment)
        try:
            # A primary marker and short shell trailer establish preservation and clean exit.
            shell = "printf '%s\n' TUI_PRIMARY_BEFORE; " + shlex.join(launch) + "; result=$?; printf '\nTUI_EXIT_%s\n' \"$result\"; sleep 30"
            tmux("-f", "/dev/null", "new-session", "-d", "-s", "run", "-c", str(workspace), "-x", "120", "-y", "40", shell)
            ledger["pid"] = tmux("display-message", "-p", "-t", "run:0.0", "#{pane_pid}").strip()
            ledger["process_group"] = os.getpgid(int(ledger["pid"]))
            if ledger["process_group"] != int(ledger["pid"]):
                raise RuntimeError("terminal child must own its process group")
            ledger["process"] = subprocess.check_output(["ps", "-p", ledger["pid"], "-o", "pid=,lstart=,command="], text=True)
            if str(binary) not in ledger["process"]:
                raise RuntimeError("running process does not identify the requested binary")
            def startup_ready():
                if (fresh_install or unconfigured) and "How would you like to work?" in screen():
                    capture("legacy-first-run")
                    raise AssertionError("fresh startup exposed legacy posture wizard")
                return log.exists() and "terminal input boundary acquired" in log.read_text()
            wait_for(startup_ready, "TUI startup")
            initial_marker = "Choose a connection" if unconfigured else ("Ask anything" if presentation == "inline" else "Ready for first turn")
            wait_for(lambda: initial_marker in screen(), "initial semantic view")
            assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}").strip() == ("0" if presentation == "inline" else "1")
            if "semantic frontend is unavailable" in screen():
                raise AssertionError("startup exposed an unavailable session projection")
            capture("01-startup")
            styled = output / "01-startup-styled.ansi"
            styled.write_text(tmux("capture-pane", "-p", "-e", "-t", "run:0.0"))
            ledger["style_capture"] = {"file": styled.name, "sha256": digest(styled)}
            assert "38;2;" not in styled.read_text() and "48;2;" not in styled.read_text(), "startup overrides terminal colors"
            assert "Ask anything" in screen() and "⏎ send" in screen()
            action("send-keys", "-t", "run:0.0", "-l", "EDITOR_PLACEHOLDER_PROBE")
            wait_for(lambda: "EDITOR_PLACEHOLDER_PROBE" in screen(), "message replaces placeholder")
            assert "Ask anything" not in screen() and "⏎ send" not in screen(), "message retains editor hints"
            capture("01a-message-without-hints")
            action("send-keys", "-t", "run:0.0", "C-u")
            wait_for(lambda: "Ask anything" in screen(), "cleared editor restores placeholder")
            assert provider.requests == 0, "placeholder probe submitted a message"
            capture("00-startup-primary", primary=True)
            if unconfigured:
                startup = (output / "00-startup-primary.txt").read_text()
                assert_unconfigured_startup(startup + screen())
                draft = "UNCONFIGURED_DRAFT_" + uuid.uuid4().hex[:12]
                action("send-keys", "-t", "run:0.0", "-l", draft)
                wait_for(lambda: draft in screen(), "unconfigured draft appears")
                capture("unconfigured-01-draft")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(lambda: "Existing connections" in screen() and "Free hosted models" in screen(), "draft opens connection choices")
                assert "Local models" in screen() and "Add provider" in screen()
                capture("unconfigured-02-connections")
                action("send-keys", "-t", "run:0.0", "Escape")
                wait_for(lambda: draft in screen() and "Existing connections" not in screen(), "cancel preserves unsubmitted draft")
                assert_unconfigured_draft(history(), draft)
                capture("unconfigured-03-cancelled-draft", primary=True)
                assert provider.requests == 0, "unconfigured interaction reached fixture inference"
                for profile in (root / ".omegon/profile.json", workspace / ".omegon/profile.json",
                                root / "omegon-home/profile.json", root / ".config/omegon/auth.json"):
                    assert not profile.exists(), f"unconfigured interaction created {profile.name}"
                assert not (workspace / ".omegon/inference.toml").exists()
                assert digest(binary) == ledger["binary_sha256"], "binary changed during capture"
                ledger["unconfigured_checks"] = {"no_model_argument": "--model" not in command,
                                                 "no_child_marker": "OMEGON_CHILD" not in environment,
                                                 "no_profile_or_credentials_written": True,
                                                 "no_fixture_manifest": True, "draft_preserved": draft,
                                                 "inference_requests": provider.requests}
                action("send-keys", "-t", "run:0.0", "C-u")
                action("send-keys", "-t", "run:0.0", "-l", "/quit")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(lambda: "TUI_EXIT_0" in screen(), "clean unconfigured exit")
                assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}:#{mouse_any_flag}").strip() == "0:0"
                capture("09-shell-return")
                ledger["session_checks"] = assert_unconfigured_session(root)
                for number, journal in enumerate(ledger["session_checks"]["journals"]):
                    copy = output / f"unconfigured-session-{number}.authority.jsonl"
                    shutil.copy2(root / journal["file"], copy)
                    journal["evidence_file"] = copy.name
                ledger["passed"] = True
                return
            assert_quiet_startup((output / "00-startup-primary.txt").read_text(), entry or "omegon")
            if fresh_install:
                startup = (output / "00-startup-primary.txt").read_text()
                for legacy in ("Fabricator", "Architect", "Explorator", "Devastator", "Found existing tools:", "Choice [1]:"):
                    assert legacy not in startup, f"legacy first-run output: {legacy}"
                assert not (root / ".omegon/profile.json").exists(), "startup created a global profile"
                assert not (workspace / ".omegon/profile.json").exists(), "startup created a project profile"
                assert provider.requests == 0
                ledger["fresh_install_checks"] = {"no_child_marker": "OMEGON_CHILD" not in environment,
                                                  "no_profile_written": True, "no_setup_input": True}
                action("send-keys", "-t", "run:0.0", "-l", "/quit")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(lambda: "TUI_EXIT_0" in screen(), "clean fresh-install exit")
                capture("09-shell-return")
                ledger["passed"] = True
                return
            action("send-keys", "-t", "run:0.0", "-l", "/connect")
            action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(lambda: "Existing connections" in screen(), "Connections opens")
            capture("connect-01-existing")
            assert "OpenAI API" in screen(), "fixture connection must be configured"
            assert "OpenRouter" not in screen(), "unconfigured catalog leaked into Connections"
            action("send-keys", "-t", "run:0.0", "/")
            action("send-keys", "-t", "run:0.0", "-l", "Add provider")
            action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(lambda: "Available providers" in screen(), "Add provider opens catalog")
            action("send-keys", "-t", "run:0.0", "/")
            action("send-keys", "-t", "run:0.0", "-l", "openrouter")
            wait_for(lambda: "OpenRouter" in screen() and "Anthropic" not in screen(), "provider search filters catalog")
            capture("connect-02-search")
            action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(lambda: "OPENROUTER_API_KEY" in screen() and "Available providers" not in screen(), "hidden API-key entry owns input")
            action("send-keys", "-t", "run:0.0", "-l", "CONNECT_SECRET_CANARY")
            wait_for(lambda: "OPENROUTER_API_KEY" in screen(), "secret entry remains active")
            assert "CONNECT_SECRET_CANARY" not in screen(), "secret leaked onto terminal"
            capture("connect-03-masked-key")
            action("send-keys", "-t", "run:0.0", "Escape")
            wait_for(lambda: "Secret input cancelled" in screen(), "key entry cancels")
            assert provider.requests == 0, "connection browsing invoked inference"
            assert not (root / ".config/omegon/auth.json").exists(), "cancelled connection wrote credentials"
            assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}").strip() == ("0" if presentation == "inline" else "1")
            capture("connect-04-cancelled")
            ledger["connection_checks"] = {"quiet_startup": True, "catalog_on_demand": True,
                                            "masked_input_cancelled": True, "inference_requests": provider.requests}
            action("send-keys", "-t", "run:0.0", "-l", "fixture turn 1")
            action("send-keys", "-t", "run:0.0", "F2")
            wait_for(lambda: "Project browser" in screen(), "project browser opens")
            capture("01a-project-sessions")
            action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(lambda: "Details" in screen() and "Current session" in screen(), "current session inspection")
            capture("01b-project-session-detail")
            action("send-keys", "-t", "run:0.0", "Escape")
            action("send-keys", "-t", "run:0.0", "Tab")
            wait_for(lambda: "No active work" in screen(), "project work tab")
            capture("01c-project-work")
            action("send-keys", "-t", "run:0.0", "Escape")
            wait_for(lambda: "Project browser" not in screen() and "fixture turn 1" in screen(), "draft survives project browsing")
            capture("01d-project-return-draft")
            for number in (1, 2):
                if number != 1:
                    action("send-keys", "-t", "run:0.0", "-l", f"fixture turn {number}")
                action("send-keys", "-t", "run:0.0", "Enter")
                if stress and number == 1:
                    wait_for(provider.stream_waiting.is_set, "held streaming response")
                    action("send-keys", "-t", "run:0.0", "-l", "UNSENT_DRAFT_SURVIVES")
                    action("send-keys", "-t", "run:0.0", "F2")
                    wait_for(lambda: "Project browser" in screen(), "Project admits input during large stream")
                    capture("stress-streaming-project")
                    prior = tmux("capture-pane", "-p", "-a", "-S", "-", "-t", "run:0.0")
                    assert "TUI_FIXTURE_REPLY_1" not in prior, "unfinalized response entered primary history"
                    provider.release_stream.set()
                    action("send-keys", "-t", "run:0.0", "Escape")
                    wait_for(lambda: "UNSENT_DRAFT_SURVIVES" in screen(), "draft survives active browsing")
                    capture("stress-return-draft")
                    action("send-keys", "-t", "run:0.0", "C-u")
                wait_for(lambda: turn_settled(number), f"reply {number} published once and runtime idle")
                capture(f"0{number + 1}-turn-{number}")
            action("resize-window", "-t", "run:0", "-x", "90", "-y", "30")
            wait_for(lambda: turn_settled(2), "reply survives resize exactly once and runtime becomes idle")
            capture("04-resize")
            if presentation == "inline":
                prior = history()
                assert "TUI_PRIMARY_BEFORE" in prior, "inline startup erased primary text"
                for number in (1, 2):
                    assert prior.count(f"TUI_FIXTURE_REPLY_{number}") == 1, "automatic publication duplicated a reply"
                capture("04-primary-before-export", primary=True)
            before_modes = tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}:#{mouse_any_flag}").strip()
            action("send-keys", "-t", "run:0.0", "-l", "/session-export scrollback")
            action("send-keys", "-t", "run:0.0", "Enter")
            if stress:
                wait_for(lambda: "Transcript chunk printed" in screen(), "bounded explicit export")
                capture("stress-export-chunk")
                action("send-keys", "-t", "run:0.0", "-l", "/session-export scrollback")
                action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(lambda: "Transcript printed" in screen() and "TUI_FIXTURE_REPLY_2" in history(), "fullscreen redraw after native publication")
            after_modes = tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}:#{mouse_any_flag}").strip()
            assert before_modes.startswith("0:" if presentation == "inline" else "1:"), "unexpected active buffer"
            assert after_modes == before_modes, "native publication changed terminal mode preferences"
            ledger["terminal_modes"] = {"before_print": before_modes, "after_print": after_modes}
            capture("04a-print-return")
            capture("04b-primary-transcript", primary=True)
            primary = (output / "04b-primary-transcript.txt").read_text()
            assert "TUI_FIXTURE_REPLY_2" in primary, "native transcript missing from saved primary screen"
            action("send-keys", "-t", "run:0.0", "-l", "fixture permission probe")
            action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(provider.tool_waiting.is_set, "provider reached permission probe barrier")
            action("send-keys", "-t", "run:0.0", "-l", "/settings")
            action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(lambda: "Settings" in screen(), "Settings opens during active turn")
            capture("05-settings")
            action("send-keys", "-t", "run:0.0", "Escape")
            action("send-keys", "-t", "run:0.0", "F2")
            action("send-keys", "-t", "run:0.0", "Tab")
            wait_for(lambda: "No active work" in screen(), "project browsing during active turn")
            capture("05a-project-during-turn")
            provider.release_tool.set()
            wait_for(lambda: "Permission required" in screen(), "permission visible above the Project browser")
            capture("06-permission")
            action("send-keys", "-t", "run:0.0", "-l", "n")
            wait_for(lambda: "Project browser" in screen() and "No active work" in screen() and "Permission required" not in screen(), "return to project work tab after denial")
            capture("07-return-project-work")
            action("send-keys", "-t", "run:0.0", "Escape")
            wait_for(lambda: turn_settled(4), "denied tool turn completes")
            capture("08-denied-turn-complete")
            assert not Path(provider.tool_path).exists(), "denied write changed the filesystem"
            assert provider.requests == 4, f"unexpected inference requests: {provider.requests}"
            if stress:
                action("send-keys", "-t", "run:0.0", "-l", "fixture cancellation probe")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(provider.cancel_waiting.is_set, "cancellation provider gate")
                action("send-keys", "-t", "run:0.0", "-l", "CANCEL_DRAFT_SURVIVES")
                action("send-keys", "-t", "run:0.0", "F2")
                wait_for(lambda: "Project browser" in screen(), "Project during cancel probe")
                action("send-keys", "-t", "run:0.0", "C-c")
                action("send-keys", "-t", "run:0.0", "Escape")
                wait_for(lambda: "CANCEL_DRAFT_SURVIVES" in screen() and "Turn cancelled or revoked." in history(), "cancel preserves draft and reports terminal outcome")
                capture("stress-cancel-draft")
                provider.release_cancel.set()
                action("send-keys", "-t", "run:0.0", "C-u")
                action("send-keys", "-t", "run:0.0", "-l", "/new")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(lambda: "Context cleared" in screen() or "Conversation boundary changed" in history(), "generation replacement")
                capture("stress-new-boundary")
                action("send-keys", "-t", "run:0.0", "-l", "fixture after cancellation and reset")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(lambda: turn_settled(6), "next turn after cancel and reset")
                assert provider.requests == 6
                capture("stress-recovered")
            if digest(binary) != ledger["binary_sha256"]:
                raise RuntimeError("binary changed during capture")
            action("send-keys", "-t", "run:0.0", "-l", "/quit")
            action("send-keys", "-t", "run:0.0", "Enter")
            wait_for(lambda: "TUI_EXIT_0" in screen(), "clean TUI exit")
            assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}:#{mouse_any_flag}").strip() == "0:0"
            capture("09-shell-return")
            ledger["passed"] = True
        finally:
            ledger["provider_requests"] = provider.requests
            for number, journal in enumerate(sorted(root.rglob("*.authority.jsonl"))):
                target = output / f"authority-{number}.jsonl"
                shutil.copy2(journal, target)
                ledger.setdefault("authority_journals", []).append({"file": target.name, "sha256": digest(target)})
            ledger["finished"] = time.time()
            try:
                # Only this invocation's private terminal server is terminated.
                tmux("kill-server", check=False)
                group = ledger.get("process_group")
                if group:
                    deadline = time.monotonic() + 3
                    while group_exists(group) and time.monotonic() < deadline:
                        time.sleep(0.05)
                    if group_exists(group):
                        os.killpg(group, signal.SIGKILL)
                        ledger["cleanup_forced"] = True
                        deadline = time.monotonic() + 2
                        while group_exists(group) and time.monotonic() < deadline:
                            time.sleep(0.05)
                        if group_exists(group):
                            raise RuntimeError("owned process group survived forced cleanup")
                ledger["cleanup_verified"] = True
            except Exception as error:
                ledger["passed"] = False
                ledger["cleanup_error"] = str(error)
                raise
            finally:
                (output / "manifest.json").write_text(json.dumps(ledger, indent=2) + "\n")

    print(json.dumps({"passed": ledger["passed"], "evidence": str(output), "provider_requests": ledger["provider_requests"]}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="freshly built Omegon executable")
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory outside the checkout")
    parser.add_argument("--tui", choices=["inline", "fullscreen"], default="fullscreen")
    parser.add_argument("--ui", choices=["active", "full"], default="active")
    parser.add_argument("--entry", choices=["om", "omegon"], help="test the fixed-build launcher default without UI flags")
    parser.add_argument("--stress", action="store_true", help="gate a large stream, cancel from Project, and replace the conversation")
    parser.add_argument("--fresh-install", action="store_true", help="verify profile-free non-child startup without a posture wizard")
    parser.add_argument("--unconfigured", action="store_true", help="verify no-model, no-credential startup and draft preservation through connection cancellation")
    arguments = parser.parse_args()
    run(arguments.binary, arguments.output.resolve(), arguments.tui, arguments.ui, arguments.entry, arguments.stress, arguments.fresh_install, arguments.unconfigured)

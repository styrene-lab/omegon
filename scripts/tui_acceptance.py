#!/usr/bin/env python3
"""Drive the actual Omegon TUI through a private tmux server; no paid inference."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import re
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
STREAMING_LINE_BODY = "steady output 界é " * 12
MARKDOWN_PROSE = "persistent structured engineering execution knowledge conversation interfaces " * 8
MARKDOWN_LIVE_PREFIX = "TUI_FIXTURE_REPLY_1\n\nMD_LIVE_BEGIN\n\n**live bold emphasis** " + MARKDOWN_PROSE * 12 + "**pending emph"


def markdown_fixture(stage):
    label = ("WIDE", "NARROW", "GROWN")[stage]
    return (f"## MD_{label}_HEADING\n\n"
            f"An **intentional emphasis** and `inline_code` remain readable.\n\n"
            f"PROSE_BEGIN_{label}\n\n{MARKDOWN_PROSE}\n\nPROSE_END_{label}\n\n"
            "- First list item with **strong words**.\n- Second list item.\n\n"
            "| System | Purpose |\n| --- | --- |\n| Memory | Durable knowledge |\n"
            "| Tools | Execute work |\n\n"
            "```rust\nfn example() {\n    let preserved = 7;\n}\n```\n\n"
            f"MD_{label}_END\n\n")


def assert_markdown_rendering(physical, styled, stage, width):
    label = ("WIDE", "NARROW", "GROWN")[stage]
    heading = physical.index(f"MD_{label}_HEADING")
    start = physical.rfind("\n", 0, heading) + 1
    end = physical.index(f"MD_{label}_END", start)
    block = physical[start:end]
    for raw in ("**", "`", "## ", "| ---"):
        assert raw not in block, f"literal Markdown leaked into rendered output: {raw}"
    prose = block.split(f"PROSE_BEGIN_{label}", 1)[1].split(f"PROSE_END_{label}", 1)[0]
    assert prose.split() == MARKDOWN_PROSE.split(), "prose dropped text or wrapped inside an ordinary word"
    prose_rows = [line for line in prose.splitlines() if line.strip()]
    assert max(map(len, prose_rows)) >= width - 24, "prose uses stale/narrow viewport width"
    minimum_filled = width - max(map(len, MARKDOWN_PROSE.split())) - 3
    assert all(len(line) >= minimum_filled for line in prose_rows[:-1]), "ordinary paragraph fractured into prematurely short rows"
    assert "    let preserved = 7;" in block, "fenced code indentation was lost"
    for expected in ("First list item", "Second list item", "Memory", "Durable knowledge", "Tools", "Execute work"):
        assert expected in block, f"Markdown content lost: {expected}"
    table_columns = [next(line.index(cell) for line in block.splitlines() if cell in line)
                     for cell in ("Purpose", "Durable knowledge", "Execute work")]
    assert len(set(table_columns)) == 1, "Markdown table columns do not align"
    # Inspect the SGR state of content itself, not unrelated composer styling.
    assert_text_modifier(styled, f"MD_{label}_HEADING")
    styled_block = styled[styled.index(f"MD_{label}_HEADING"):styled.index(f"MD_{label}_END")]
    assert_text_modifier(styled_block, "intentional emphasis")
    assert_text_modifier(styled_block, "inline_code", modifier=4)


def assert_inline_working_status(viewport):
    rows = viewport.splitlines()
    tops = [index for index, row in enumerate(rows) if row.lstrip().startswith(("╭", "┌"))]
    assert tops, "inline composer frame is missing"
    top = tops[-1]
    bottoms = [index for index in range(top + 1, len(rows))
               if rows[index].lstrip().startswith(("╰", "└"))]
    assert bottoms, "inline composer bottom frame is missing"
    assert "Working" not in "\n".join(rows[:top]), "working status interrupts the answer above the composer"
    frame = "\n".join(rows[top:bottoms[0] + 1])
    assert "Working · Ctrl+C cancel" in frame, "working status is absent from the composer frame"
    assert "F2 Project" not in viewport, "legacy project helper remains in the live response surface"


def assert_text_modifier(styled, expected, modifier=1):
    enabled = False
    observed = False
    for piece in re.split(r"(\x1b\[[0-9;]*m)", styled):
        if piece.startswith("\x1b["):
            params = [int(value or 0) for value in piece[2:-1].split(";")]
            index = 0
            while index < len(params):
                param = params[index]
                if param in (0, {1: 22, 4: 24}[modifier]):
                    enabled = False
                elif param == modifier:
                    enabled = True
                elif param in (38, 48, 58) and index + 1 < len(params):
                    # Color channels can equal 1; they are not bold modifiers.
                    index += 4 if params[index + 1] == 2 else 2
                index += 1
        elif expected in piece:
            observed = enabled
    assert observed, f"content has no terminal modifier {modifier}: {expected}"


def ansi_spans(capture):
    """Decode captured terminal foreground/background roles without assuming SGR grouping."""
    foreground = background = None
    row = 0
    spans = []
    for piece in re.split(r"(\x1b\[[0-9;]*m)", capture):
        if piece.startswith("\x1b["):
            params = [int(value or 0) for value in piece[2:-1].split(";")]
            index = 0
            while index < len(params):
                value = params[index]
                if value == 0:
                    foreground = background = None
                elif value == 39:
                    foreground = None
                elif value == 49:
                    background = None
                elif value in (38, 48) and index + 2 < len(params):
                    mode = params[index + 1]
                    color = params[index + 2] if mode == 5 else tuple(params[index + 2:index + 5])
                    if value == 38:
                        foreground = color
                    else:
                        background = color
                    index += 2 if mode == 5 else 4
                elif 30 <= value <= 37 or 90 <= value <= 97:
                    foreground = value - 30 if value < 90 else value - 90 + 8
                elif 40 <= value <= 47 or 100 <= value <= 107:
                    background = value - 40 if value < 100 else value - 100 + 8
                index += 1
        else:
            for index, part in enumerate(piece.split("\n")):
                if index:
                    row += 1
                spans.append({"row": row, "text": part, "fg": foreground, "bg": background})
    return spans


def assert_control_palette(captures):
    decoded = {name: ansi_spans(value) for name, value in captures.items()}
    placeholder = [span for span in decoded["composer"] if "Ask anything" in span["text"]]
    assert placeholder and all(span["fg"] == 244 and span["bg"] is None for span in placeholder), "composer placeholder lacks neutral hint role/default canvas"
    draft = [span for span in decoded["draft"] if "CONTROLS_DRAFT_PROBE" in span["text"]]
    assert draft and all(span["fg"] is None and span["bg"] is None for span in draft), "typed operator input does not use terminal defaults"
    foregrounds = set()
    for name in ("slash", "connect", "connect-moved", "settings", "think", "help", "help-panel"):
        spans = decoded[name]
        assert any(span["bg"] == 235 for span in spans), f"{name} lacks neutral panel background"
        assert any(span["bg"] is None and len(span["text"]) >= 2 and not span["text"].strip() for span in spans), f"{name} repainted the entire default canvas"
        foregrounds.update(span["fg"] for span in spans if span["text"].strip())
    assert {244, 248, 252, 255} <= foregrounds, "controls lack distinct border/hint, secondary, text, and title roles"
    selected = []
    for name in ("connect", "connect-moved"):
        rows = {span["row"] for span in decoded[name] if span["bg"] == 240 and span["fg"] == 255 and span["text"].strip()}
        assert rows, f"{name} lacks selected-row foreground/background contrast"
        selected.append(rows)
    assert selected[0] != selected[1], "arrow key did not move the selected background to another row"
    for name, label, foreground, background in (("connect", "OpenAI API", 255, 240),
                                               ("connect-moved", "OpenAI API", 252, 235),
                                               ("connect-moved", "Free hosted models", 255, 240)):
        label_spans = [span for span in decoded[name] if label in span["text"]]
        assert label_spans and all(span["fg"] == foreground and span["bg"] == background for span in label_spans), f"{name} did not apply the correct row role to {label}"
    if "connect-narrow" in decoded:
        narrow = [span for span in decoded["connect-narrow"] if "Free hosted models" in span["text"]]
        assert narrow and all(span["fg"] == 255 and span["bg"] == 240 for span in narrow), "narrow menu lost selected-row contrast"
    descriptions = [span for span in decoded["think"] if "token budget" in span["text"]]
    assert descriptions and all(span["fg"] in (244, 248) for span in descriptions), "selector descriptions lack secondary/hint contrast"


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
            request_body = json.loads(self.rfile.read(length))
            with server.request_lock:
                server.requests += 1
                number = server.requests
                server.request_bodies.append(request_body)
            if not server.streaming and server.stress and number == 5:
                server.cancel_waiting.set()
                if not server.release_cancel.wait(timeout=60):
                    self.send_error(504)
                    return
            tool_probe = not server.streaming and number == 3 and server.tool_path is not None
            if tool_probe:
                server.tool_waiting.set()
                if not server.release_tool.wait(timeout=60):
                    self.send_error(504)
                    return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            if server.markdown:
                try:
                    for offset in range(0, len(MARKDOWN_LIVE_PREFIX), 7):
                        event = {"choices": [{"index": 0, "delta": {"content": MARKDOWN_LIVE_PREFIX[offset:offset + 7]}, "finish_reason": None}]}
                        self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                        self.wfile.flush()
                    server.markdown_prefix_waiting.set()
                    if not server.release_markdown_prefix.wait(timeout=60):
                        return
                    for stage in range(3):
                        content = markdown_fixture(stage)
                        if stage == 0:
                            content = "asis**\n\nMD_LIVE_END\n\n" + content
                        # Deliberately split syntax tokens across transport chunks.
                        for offset in range(0, len(content), 7):
                            event = {"choices": [{"index": 0, "delta": {"content": content[offset:offset + 7]}, "finish_reason": None}]}
                            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                            self.wfile.flush()
                        server.stream_stages[stage].set()
                        if not server.release_stages[stage].wait(timeout=60):
                            return
                    event = {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}
                    self.wfile.write(("data: " + json.dumps(event) + "\n\ndata: [DONE]\n\n").encode())
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    pass
                return
            if server.streaming:
                try:
                    stages = range(3) if number == 1 else [3 if number == 2 else 4]
                    label = {1: "A", 2: "B", 3: "C"}.get(number)
                    if label is None:
                        raise AssertionError(f"unexpected streaming fixture request {number}")
                    for stage in stages:
                        first = stage * 32 + 1 if number == 1 else 1
                        content = "".join(f"STREAM_{label}_{line:04} " + STREAMING_LINE_BODY + "\n"
                                          for line in range(first, first + 32))
                        if first == 1:
                            content = f"TUI_FIXTURE_REPLY_{number}\n" + content
                        event = {"choices": [{"index": 0, "delta": {"content": content}, "finish_reason": None}]}
                        self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
                        self.wfile.flush()
                        server.stream_stages[stage].set()
                        if not server.release_stages[stage].wait(timeout=60):
                            return
                    delta, finish = {}, "stop"
                    if number == 2:
                        delta = {"tool_calls": [{"index": 0, "id": "stream-read-probe", "type": "function",
                                  "function": {"name": "read", "arguments": json.dumps({"path": server.stream_read_path})}}]}
                        finish = "tool_calls"
                    event = {"choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
                    self.wfile.write(("data: " + json.dumps(event) + "\n\ndata: [DONE]\n\n").encode())
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    pass  # Failed acceptance terminates its owned client before releasing gates.
                return
            reply = f"TUI_FIXTURE_REPLY_{number}"
            if server.tool_path is not None and number >= 4:
                reply += (" The operator denied the requested write. The fixture has completed its permission check "
                          "and will make no further tool calls. The requested file remains absent, the prior project "
                          "surface is preserved, and control returns to the conversation for the next operator prompt.")
            if server.stress and number == 1:
                reply = f"TUI_FIXTURE_REPLY_{number}\n" + "bounded-output 界é " * 5000
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
    server.streaming = False
    server.markdown = False
    server.markdown_prefix_waiting = threading.Event()
    server.release_markdown_prefix = threading.Event()
    server.stream_stages = [threading.Event() for _ in range(5)]
    server.release_stages = [threading.Event() for _ in range(5)]
    server.stream_read_path = None
    server.request_bodies = []
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
        for release in server.release_stages:
            release.set()
        server.release_markdown_prefix.set()
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


def assert_streaming_history(scrollback, transcript, markers):
    for marker in markers:
        assert marker in scrollback, f"stable streamed line missing from PRIMARY scrollback before completion: {marker}"
        assert transcript.count(marker) == 1, f"streamed line overwritten or replayed: {marker}"


def assert_streaming_payload(transcript, markers):
    # Terminal hard/soft wrapping and wide-cell padding may change whitespace;
    # every other character, including combining marks, must survive exactly.
    normalized = "".join(transcript.split())
    body = "".join(STREAMING_LINE_BODY.split())
    for marker in markers:
        assert normalized.count(marker + body) == 1, f"streamed payload lost or altered nonwhitespace characters: {marker}"


def run(binary: Path, output: Path, presentation="fullscreen", detail="active", entry=None, stress=False, fresh_install=False, unconfigured=False, streaming=False, markdown=False, controls=False):
    if controls and (unconfigured or stress or streaming or markdown):
        raise ValueError("controls acceptance requires a configured layout without another scenario")
    if markdown and (presentation != "inline" or unconfigured or stress or streaming):
        raise ValueError("Markdown acceptance requires configured inline layout without another scenario")
    if streaming and (presentation != "inline" or unconfigured or stress):
        raise ValueError("streaming acceptance requires configured inline layout without stress")
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
              "unconfigured": unconfigured, "streaming": streaming, "markdown": markdown, "controls": controls, "terminal_owner": "private-headless-tmux", "gui_windows_created": 0}

    diff = subprocess.check_output(["git", "diff", "HEAD", "--binary"], cwd=checkout)
    (output / "source.diff").write_bytes(diff)
    ledger["source_diff_sha256"] = digest(output / "source.diff")
    ledger["driver_sha256"] = digest(Path(__file__))
    (output / "driver.py").write_bytes(Path(__file__).read_bytes())

    def tmux(*args, check=True):
        return subprocess.run(["tmux", "-L", socket, *args], check=check, capture_output=True, text=True, timeout=10).stdout

    def action(*args):
        ledger.setdefault("actions", []).append({"time": time.time(), "tmux": args})
        return tmux(*args)

    def screen():
        return tmux("capture-pane", "-p", "-t", "run:0.0")

    def history():
        return tmux("capture-pane", *(["-J"] if streaming else []), "-p", "-S", "-", "-t", "run:0.0") if presentation == "inline" else screen()

    def capture(name, *, primary=False):
        path = output / (name + ".txt")
        path.write_text((history() if presentation == "inline" else tmux("capture-pane", "-p", "-a", "-t", "run:0.0")) if primary else screen())
        if streaming and primary:
            # -J joins terminal soft-wrap rows for marker identity after resize;
            # retain physical rows too so the actual visual layout is reviewable.
            physical = output / (name + ".physical.txt")
            physical.write_text(tmux("capture-pane", "-p", "-S", "-", "-t", "run:0.0"))
            ledger.setdefault("physical_captures", []).append({"file": physical.name, "sha256": digest(physical)})
        ledger["captures"].append({"name": name, "time": time.time(), "sha256": digest(path),
                                   "geometry": tmux("display-message", "-p", "-t", "run:0.0", "#{pane_width}x#{pane_height}").strip()})

    def wait_for(predicate, label, seconds=60):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.05)
        capture("failure")
        capture("failure-primary", primary=True)
        raise TimeoutError(label)

    with tempfile.TemporaryDirectory(prefix="omegon-tui-") as temporary, fixture_provider() as provider:
        root = Path(temporary)
        provider.stress = stress
        provider.streaming = streaming
        provider.markdown = markdown
        workspace = prepare_unconfigured_workspace(root) if unconfigured else prepare_fixture_workspace(root, provider)
        if fresh_install and not unconfigured:
            (workspace / ".omegon/profile.json").unlink()
        if streaming:
            read_probe = workspace / "stream-probe.txt"
            read_probe.write_text("STREAM_TOOL_FILE_CONTENT\n")
            provider.stream_read_path = str(read_probe)
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
        if controls:
            environment.pop("NO_COLOR", None)
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
            if controls:
                control_captures = {}
                def control_capture(name):
                    capture(f"controls-{name}")
                    styled = output / f"controls-{name}.ansi"
                    styled.write_text(tmux("capture-pane", "-e", "-p", "-t", "run:0.0"))
                    control_captures[name] = styled.read_text()
                    ledger.setdefault("control_captures", []).append({"name": name, "file": styled.name,
                        "sha256": digest(styled), "time": time.time()})
                def open_command(command):
                    action("send-keys", "-t", "run:0.0", "-l", command)
                    action("send-keys", "-t", "run:0.0", "Enter")
                def dismiss_control(marker):
                    action("send-keys", "-t", "run:0.0", "Escape")
                    wait_for(lambda: "Ask anything" in screen() and marker not in screen(), f"{marker} dismissed")

                control_capture("composer")
                action("send-keys", "-t", "run:0.0", "-l", "CONTROLS_DRAFT_PROBE")
                wait_for(lambda: "CONTROLS_DRAFT_PROBE" in screen(), "typed control draft")
                assert "Ask anything" not in screen(), "placeholder remained behind operator input"
                control_capture("draft")
                action("send-keys", "-t", "run:0.0", "C-u")
                action("send-keys", "-t", "run:0.0", "-l", "/")
                wait_for(lambda: "registry autocomplete" in screen(), "slash autocomplete visible")
                control_capture("slash")
                action("send-keys", "-t", "run:0.0", "C-u")
                open_command("/connect")
                wait_for(lambda: "Existing connections" in screen(), "connection menu visible")
                control_capture("connect")
                before_selection = tmux("capture-pane", "-e", "-p", "-t", "run:0.0")
                action("send-keys", "-t", "run:0.0", "Down")
                wait_for(lambda: tmux("capture-pane", "-e", "-p", "-t", "run:0.0") != before_selection, "connection selection moved")
                control_capture("connect-moved")
                action("resize-window", "-t", "run:0", "-x", "36", "-y", "40")
                wait_for(lambda: any("Connections" in line and line.rstrip().endswith("╮")
                                     and len(line.rstrip()) <= 36 for line in screen().splitlines()),
                         "connection menu redrawn at narrow width")
                control_capture("connect-narrow")
                action("resize-window", "-t", "run:0", "-x", "120", "-y", "40")
                wait_for(lambda: any("Connections" in line and line.rstrip().endswith("╮")
                                     and len(line.rstrip()) > 80 for line in screen().splitlines()),
                         "connection menu restored at normal width")
                dismiss_control("Existing connections")
                open_command("/settings")
                wait_for(lambda: "Settings" in screen() and "Thinking" in screen(), "settings menu visible")
                control_capture("settings")
                dismiss_control("Settings")
                open_command("/think")
                wait_for(lambda: "Thinking level" in screen(), "thinking selector visible")
                control_capture("think")
                dismiss_control("Thinking level")
                open_command("/help")
                wait_for(lambda: "Slash command inventory" in screen(), "help command inventory visible")
                control_capture("help")
                dismiss_control("Slash command inventory")
                open_command("/help all")
                wait_for(lambda: "command · /help" in screen(), "help text command panel visible")
                control_capture("help-panel")
                dismiss_control("command · /help")
                assert provider.requests == 0, "control browsing submitted inference"
                assert_control_palette(control_captures)
                assert digest(binary) == ledger["binary_sha256"], "binary changed during controls acceptance"
                ledger["controls_checks"] = {"inference_requests": 0, "neutral_role_palette": True,
                    "selection_moved": True, "default_canvas_preserved": True, "placeholder_replaced": True,
                    "surfaces": list(control_captures)}
                open_command("/quit")
                wait_for(lambda: "TUI_EXIT_0" in screen(), "controls clean TUI exit")
                assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}:#{mouse_any_flag}").strip() == "0:0"
                capture("controls-shell-return")
                ledger["passed"] = True
                return
            if markdown:
                action("send-keys", "-t", "run:0.0", "-l", "render the deterministic Markdown fixture")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(provider.markdown_prefix_waiting.is_set, "Markdown unfinished paragraph held")
                def live_scrollback():
                    return tmux("capture-pane", "-p", "-S", "-", "-E", "-1", "-t", "run:0.0")
                wait_for(lambda: "live bold emphasis" in live_scrollback(),
                         "unfinished Markdown paragraph reaches native scrollback", seconds=12)
                capture("markdown-unfinished-paragraph-primary", primary=True)
                capture("markdown-unfinished-paragraph-viewport")
                assert_inline_working_status(screen())
                live_styled = output / "markdown-unfinished-paragraph-styled.ansi"
                live_styled.write_text(tmux("capture-pane", "-e", "-p", "-S", "-", "-E", "-1", "-t", "run:0.0"))
                assert "**live bold emphasis**" not in live_scrollback(), "completed inline span remained raw while paragraph was unfinished"
                assert_text_modifier(live_styled.read_text(), "live bold emphasis")
                live_prose = live_scrollback().split("live bold emphasis", 1)[1]
                live_words = live_prose.split()
                assert live_words == (MARKDOWN_PROSE.split() * 12)[:len(live_words)], "unfinished paragraph broke or dropped ordinary words"
                live_rows = [line for line in live_prose.splitlines() if line.strip()]
                assert all(len(line) >= 120 - max(map(len, MARKDOWN_PROSE.split())) - 3
                           for line in live_rows[1:-1]), "unfinished paragraph published prematurely short rows"
                assert not provider.release_markdown_prefix.is_set(), "paragraph observation was not live"
                ledger["markdown_live_paragraph"] = {"provider_held": True, "paragraph_newline_sent": False,
                    "open_emphasis_at_tail": True, "styled_file": live_styled.name, "sha256": digest(live_styled)}
                provider.release_markdown_prefix.set()
                for stage, width in enumerate((120, 72, 160)):
                    label = ("WIDE", "NARROW", "GROWN")[stage]
                    wait_for(provider.stream_stages[stage].is_set, f"Markdown stage {stage} held")
                    wait_for(lambda: f"MD_{label}_END" in history(), f"Markdown stage {stage} published")
                    capture(f"markdown-{stage}-primary", primary=True)
                    capture(f"markdown-{stage}-viewport")
                    assert_inline_working_status(screen())
                    styled = output / f"markdown-{stage}-styled.ansi"
                    styled.write_text(tmux("capture-pane", "-e", "-p", "-S", "-", "-t", "run:0.0"))
                    ledger.setdefault("markdown_checkpoints", []).append({"stage": stage, "width": width,
                        "provider_held": not provider.release_stages[stage].is_set(),
                        "styled_file": styled.name, "sha256": digest(styled)})
                    assert_markdown_rendering(history(), styled.read_text(), stage, width)
                    if stage < 2:
                        action("resize-window", "-t", "run:0", "-x", str((72, 160)[stage]), "-y", "40")
                        next_width = (72, 160)[stage]
                        wait_for(lambda: any(line.startswith(("╰", "└")) and line.endswith(("╯", "┘"))
                                             and len(line) >= next_width - 3 for line in screen().splitlines()),
                                 "Markdown composer redrawn at resized width")
                    provider.release_stages[stage].set()
                wait_for(lambda: turn_settled(1), "Markdown turn completed")
                complete = history()
                for label in ("WIDE", "NARROW", "GROWN"):
                    assert complete.count(f"MD_{label}_HEADING") == 1, "Markdown block replayed on completion/resize"
                assert provider.requests == 1, "unexpected Markdown inference requests"
                assert digest(binary) == ledger["binary_sha256"], "binary changed during Markdown acceptance"
                capture("markdown-complete-primary", primary=True)
                ledger["markdown_checks"] = {"live_checkpoints": 4, "widths": [120, 72, 160],
                    "word_wrapping": True, "terminal_styles": True, "code_indentation": True,
                    "working_status_inside_composer": True,
                    "local_requests": provider.requests, "paid_requests": 0}
                action("send-keys", "-t", "run:0.0", "-l", "/quit")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(lambda: "TUI_EXIT_0" in screen(), "Markdown clean TUI exit")
                assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}:#{mouse_any_flag}").strip() == "0:0"
                capture("markdown-shell-return")
                ledger["passed"] = True
                return
            if streaming:
                def scrollback():
                    assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}").strip() == "0"
                    return tmux("capture-pane", "-J", "-p", "-S", "-", "-E", "-1", "-t", "run:0.0")

                def authority_records():
                    return [json.loads(line) for journal in root.rglob("*.authority.jsonl")
                            for line in journal.read_text().splitlines() if line]

                committed = []
                def paused_stage(stage, label, first):
                    wait_for(provider.stream_stages[stage].is_set, f"provider streaming barrier {stage}")
                    markers = [f"STREAM_{label}_{line:04}" for line in range(first, first + 8)]
                    # The provider is held before finish_reason/[DONE]. Current-screen
                    # visibility does not count: require real rows above the viewport.
                    wait_for(lambda: all(marker in scrollback() for marker in markers),
                             f"live stable prefix {label}/{first} reaches PRIMARY scrollback", seconds=12)
                    committed.extend(markers)
                    assert_streaming_history(scrollback(), history(), committed)
                    assert not provider.release_stages[stage].is_set()
                    assert not authority_runtime_idle(authority_records()), "streaming observation happened after turn completion"
                    capture(f"stream-{stage}-paused-primary", primary=True)
                    saved = output / f"stream-{stage}-scrollback-only.txt"
                    saved.write_text(scrollback())
                    ledger.setdefault("streaming_checkpoints", []).append({"stage": stage,
                        "requests": provider.requests, "markers": list(committed), "provider_held": True,
                        "runtime_idle": False, "scrollback_file": saved.name, "sha256": digest(saved)})

                action("send-keys", "-t", "run:0.0", "-l", "stream fixture first turn")
                action("send-keys", "-t", "run:0.0", "Enter")
                paused_stage(0, "A", 1)
                provider.release_stages[0].set()
                paused_stage(1, "A", 33)
                action("resize-window", "-t", "run:0", "-x", "72", "-y", "24")
                wait_for(lambda: all(marker in scrollback() for marker in committed), "stream prefix survives narrower resize")
                assert_streaming_history(scrollback(), history(), committed)
                capture("stream-resize-paused-primary", primary=True)
                provider.release_stages[1].set()
                paused_stage(2, "A", 65)
                provider.release_stages[2].set()
                wait_for(lambda: turn_settled(1), "first streamed turn completes without replay")
                all_first = [f"STREAM_A_{line:04}" for line in range(1, 97)]
                first_complete = history()
                for marker in all_first:
                    assert first_complete.count(marker) == 1, f"first turn lost or replayed {marker}"
                capture("stream-first-complete-primary", primary=True)
                action("send-keys", "-t", "run:0.0", "-l", "stream fixture second turn with read tool")
                action("send-keys", "-t", "run:0.0", "Enter")
                paused_stage(3, "B", 1)
                provider.release_stages[3].set()
                paused_stage(4, "C", 1)
                # The third local request proves the real read tool completed and
                # its result reached the provider before the continued answer.
                tool_messages = [message for message in provider.request_bodies[2]["messages"]
                                 if message.get("role") == "tool"]
                assert any("STREAM_TOOL_FILE_CONTENT" in json.dumps(message) for message in tool_messages), "read tool result missing from continuation"
                continued = history()
                for marker in all_first:
                    assert continued.count(marker) == 1, f"second turn overwrote or replayed {marker}"
                provider.release_stages[4].set()
                wait_for(lambda: turn_settled(3), "second streamed turn and read tool complete")
                complete = history()
                all_markers = all_first + [f"STREAM_{label}_{line:04}" for label in ("B", "C") for line in range(1, 33)]
                for marker in all_markers:
                    assert complete.count(marker) == 1, f"final transcript lost or replayed {marker}"
                positions = [complete.index(marker) for marker in all_markers]
                assert positions == sorted(positions), "streamed lines were reordered"
                assert_streaming_payload(complete, all_markers)
                assert provider.requests == 3, f"unexpected local requests: {provider.requests}"
                assert digest(binary) == ledger["binary_sha256"], "binary changed during streaming acceptance"
                capture("stream-second-tool-complete-primary", primary=True)
                ledger["streaming_checks"] = {"unique_lines": len(all_markers), "local_requests": provider.requests,
                    "paid_requests": 0, "live_checkpoints": 5, "read_tool_completed": True,
                    "nonwhitespace_payload_preserved": True, "resize": "120x40 to 72x24"}
                action("send-keys", "-t", "run:0.0", "-l", "/quit")
                action("send-keys", "-t", "run:0.0", "Enter")
                wait_for(lambda: "TUI_EXIT_0" in screen(), "streaming clean TUI exit")
                assert tmux("display-message", "-p", "-t", "run:0.0", "#{alternate_on}:#{mouse_any_flag}").strip() == "0:0"
                capture("stream-shell-return")
                ledger["passed"] = True
                return
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
                    if presentation == "inline":
                        wait_for(lambda: history().count("TUI_FIXTURE_REPLY_1") == 1,
                                 "stable streaming prefix published before Project takes fullscreen")
                        capture("stress-prefix-before-project", primary=True)
                    action("send-keys", "-t", "run:0.0", "-l", "UNSENT_DRAFT_SURVIVES")
                    action("send-keys", "-t", "run:0.0", "F2")
                    wait_for(lambda: "Project browser" in screen(), "Project admits input during large stream")
                    capture("stress-streaming-project")
                    provider.release_stream.set()
                    action("send-keys", "-t", "run:0.0", "Escape")
                    wait_for(lambda: "UNSENT_DRAFT_SURVIVES" in screen(), "draft survives active browsing")
                    capture("stress-return-draft")
                    if presentation == "inline":
                        # tmux -a cannot access saved primary history. Check only
                        # after returning to primary, where history is available.
                        wait_for(lambda: history().count("TUI_FIXTURE_REPLY_1") == 1,
                                 "Project return preserves stable primary prefix")
                        capture("stress-prefix-after-project", primary=True)
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
        except Exception as error:
            ledger["error"] = str(error)
            if ledger.get("pid") and not (output / "failure.txt").exists():
                try:
                    capture("failure")
                    capture("failure-primary", primary=True)
                except (subprocess.SubprocessError, OSError) as capture_error:
                    ledger["failure_capture_error"] = str(capture_error)
            raise
        finally:
            ledger["provider_requests"] = provider.requests
            if streaming or markdown:
                ledger["streaming_fixture"] = {"stages_sent": [event.is_set() for event in provider.stream_stages],
                    "stages_released": [event.is_set() for event in provider.release_stages]}
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
    parser.add_argument("--controls", action="store_true", help="gate neutral control hierarchy and selection without inference")
    parser.add_argument("--markdown", action="store_true", help="gate live Markdown styles, word wrapping, code indentation, and resize")
    parser.add_argument("--streaming", action="store_true", help="gate stable streamed lines in primary scrollback before completion, resize, and interleave a read tool")
    parser.add_argument("--stress", action="store_true", help="gate a large stream, cancel from Project, and replace the conversation")
    parser.add_argument("--fresh-install", action="store_true", help="verify profile-free non-child startup without a posture wizard")
    parser.add_argument("--unconfigured", action="store_true", help="verify no-model, no-credential startup and draft preservation through connection cancellation")
    arguments = parser.parse_args()
    run(arguments.binary, arguments.output.resolve(), arguments.tui, arguments.ui, arguments.entry, arguments.stress, arguments.fresh_install, arguments.unconfigured, arguments.streaming, arguments.markdown, arguments.controls)

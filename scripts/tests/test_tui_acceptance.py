"""Contract tests for the captured TUI runner's local provider."""
import importlib.util
import json
import tempfile
import tomllib
from pathlib import Path
from urllib.request import Request, urlopen

spec = importlib.util.spec_from_file_location("tui_acceptance", Path(__file__).parents[1] / "tui_acceptance.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


def test_provider_streams_distinct_turns_without_external_inference():
    with runner.fixture_provider() as server:
        for number in (1, 2):
            request = Request(server.url + "/v1/chat/completions", data=json.dumps({"messages": []}).encode(), headers={"Content-Type": "application/json"})
            with urlopen(request, timeout=2) as response:
                body = response.read().decode()
            assert f"TUI_FIXTURE_REPLY_{number}" in body
            assert '"finish_reason": "stop"' in body
            assert "data: [DONE]" in body
        assert server.requests == 2


def test_provider_rejects_unknown_routes():
    from urllib.error import HTTPError
    with runner.fixture_provider() as server:
        try:
            urlopen(server.url + "/unexpected", timeout=2)
        except HTTPError as error:
            assert error.code == 404
        else:
            raise AssertionError("unknown route accepted")

def test_provider_can_request_a_bounded_permission_probe():
    with runner.fixture_provider() as server:
        server.tool_path = "/tmp/fixture-only/denied.txt"
        server.requests = 2
        server.release_tool.set()
        request = Request(server.url + "/v1/chat/completions", data=b'{"messages": []}', headers={"Content-Type": "application/json"})
        with urlopen(request, timeout=2) as response:
            body = response.read().decode()
        assert '"name": "write"' in body
        assert '"finish_reason": "tool_calls"' in body
        assert "denied.txt" in body


def test_quiet_startup_gate_rejects_catalog_and_duplicate_summary():
    summary = f"omegon · {runner.FIXTURE_MODEL} · /connect · /settings\n"
    runner.assert_quiet_startup(summary)
    for invalid in [summary * 2, summary + "  ⚠ OpenRouter (none)\n", "No LLM provider detected\n", summary + "Git: main\n"]:
        try:
            runner.assert_quiet_startup(invalid)
        except AssertionError:
            pass
        else:
            raise AssertionError("startup output violation was accepted")


def test_unconfigured_launch_has_no_implied_provider():
    with tempfile.TemporaryDirectory() as folder:
        root = Path(folder)
        workspace = runner.prepare_unconfigured_workspace(root)
        assert list(workspace.iterdir()) == [], "unconfigured workspace must have no fixture configuration"
        command = runner.tui_command(Path("/binary"), workspace, Path("/log"), unconfigured=True)
        assert "--model" not in command
        environment = runner.tui_environment(root, unconfigured=True)
        assert environment["HOME"] == str(root)
        for key in ("OMEGON_CHILD", "OPENAI_API_KEY", "ANTHROPIC_API_KEY",
                    "OMEGON_PROJECT_ENDPOINT_616363657074616E6365_TOKEN"):
            assert key not in environment, f"unconfigured launch has {key}"


def test_unconfigured_startup_gate_rejects_phantom_route_metadata():
    clean = "om · /connect · /settings\nChoose a connection\nAsk anything  ⏎ send\n"
    runner.assert_unconfigured_startup(clean)
    for stale in ("anthropic", "claude-sonnet-4-6", "thinking minimal", "0% of 1.0M context",
                  "No LLM provider available", "Fabricator"):
        try:
            runner.assert_unconfigured_startup(clean + stale)
        except AssertionError:
            pass
        else:
            raise AssertionError(f"unconfigured startup accepted {stale}")


def test_unconfigured_cancel_gate_requires_one_unsent_draft():
    draft = "UNCONFIGURED_DRAFT_PROBE"
    runner.assert_unconfigured_draft(f"Choose a connection\n{draft}\n", draft)
    for invalid in ("Choose a connection", f"{draft}\n{draft}",
                    f"{draft}\nNo LLM provider available", f"{draft}\nTUI_FIXTURE_REPLY_1"):
        try:
            runner.assert_unconfigured_draft(invalid, draft)
        except AssertionError:
            pass
        else:
            raise AssertionError("cancel accepted missing, submitted, or duplicated draft")


def test_unconfigured_session_gate_rejects_admitted_prompts_and_model_requests():
    with tempfile.TemporaryDirectory() as folder:
        root = Path(folder)
        journal = root / "session.authority.jsonl"
        journal.write_text(json.dumps({"event_type": "session.created"}) + "\n")
        result = runner.assert_unconfigured_session(root)
        assert result["inference_requests"] == 0 and result["conversation_turns"] == 0
        for event in ("prompt.admitted", "turn.started", "model.request_prepared"):
            journal.write_text(json.dumps({"event_type": event}) + "\n")
            try:
                runner.assert_unconfigured_session(root)
            except AssertionError:
                pass
            else:
                raise AssertionError(f"unconfigured session accepted {event}")

def test_reply_readiness_rejects_active_placeholder_and_publication_overlap():
    reply = "TUI_FIXTURE_REPLY_2"
    idle = "Ask anything  / commands  ⏎ send"
    assert runner.reply_ready(idle, reply, reply, "inline", "active", runtime_idle=True)
    assert not runner.reply_ready(idle, reply, reply, "inline", "active", runtime_idle=False)
    assert not runner.reply_ready("Working · Ctrl+C cancel\n" + idle, reply, reply, "inline", "active", runtime_idle=True)
    assert not runner.reply_ready("Publishing completed output\n" + idle, reply, reply, "inline", "active", runtime_idle=True)
    assert not runner.reply_ready(idle, reply + "\n" + reply, reply, "inline", "active", runtime_idle=True)


def test_runtime_idle_requires_matching_terminal_fact_for_each_started_turn():
    started = {"event_type": "turn.started", "payload": {"turn_id": "first"}}
    closed = {"event_type": "turn.closed", "payload": {"turn_id": "first"}}
    second = {"event_type": "turn.started", "payload": {"turn_id": "second"}}
    assert not runner.authority_runtime_idle([])
    assert not runner.authority_runtime_idle([started])
    assert runner.authority_runtime_idle([started, closed])
    assert not runner.authority_runtime_idle([started, closed, second])
    assert runner.authority_runtime_idle([started, closed, second, {"event_type": "turn.closed", "payload": {"turn_id": "second"}}])


def test_streaming_fixture_stages_hold_before_response_completion():
    import queue
    import threading
    events = queue.Queue()
    with runner.fixture_provider() as server:
        server.streaming = True
        def consume():
            request = Request(server.url + "/v1/chat/completions", data=b'{"messages": []}', headers={"Content-Type": "application/json"})
            with urlopen(request, timeout=5) as response:
                for line in response:
                    if line.startswith(b"data: "):
                        events.put(line.decode())
        reader = threading.Thread(target=consume)
        reader.start()
        try:
            for stage in range(3):
                assert server.stream_stages[stage].wait(2)
                event = events.get(timeout=2)
                assert f"STREAM_A_{stage * 32 + 1:04}" in event
                assert f"STREAM_A_{(stage + 1) * 32:04}" in event
                assert "[DONE]" not in event and '"finish_reason": null' in event
                assert reader.is_alive(), "provider completed before release"
                server.release_stages[stage].set()
            assert '"finish_reason": "stop"' in events.get(timeout=2)
            assert "[DONE]" in events.get(timeout=2)
            reader.join(2)
            assert not reader.is_alive()
        finally:
            for release in getattr(server, "release_stages", []):
                release.set()
            reader.join(5)


def test_streaming_fixture_interleaves_read_tool_and_followup_response():
    with runner.fixture_provider() as server:
        server.streaming = True
        server.requests = 1
        server.stream_read_path = "/fixture/stream-probe.txt"
        server.tool_path = "/fixture/legacy-denied.txt"
        server.release_stages[3].set()
        request = Request(server.url + "/v1/chat/completions", data=b'{"messages": []}', headers={"Content-Type": "application/json"})
        with urlopen(request, timeout=2) as response:
            body = response.read().decode()
        assert "STREAM_B_0001" in body and "STREAM_B_0032" in body
        assert '"name": "read"' in body and "stream-probe.txt" in body
        assert '"finish_reason": "tool_calls"' in body
        server.release_stages[4].set()
        with urlopen(request, timeout=2) as response:
            body = response.read().decode()
        assert "STREAM_C_0001" in body and "STREAM_C_0032" in body
        assert '"finish_reason": "stop"' in body
        assert server.requests == 3


def test_streaming_payload_preserves_nonwhitespace_across_wraps():
    marker = "STREAM_A_0001"
    wrapped = marker + " " + "steady out\nput 界 é " * 12
    runner.assert_streaming_payload(wrapped, [marker])
    for corrupted in [wrapped.replace("put 界", "p", 1),
                      wrapped.replace("界", "X界", 1),
                      wrapped.replace("é", "e", 1)]:
        try:
            runner.assert_streaming_payload(corrupted, [marker])
        except AssertionError:
            pass
        else:
            raise AssertionError("streaming payload accepted dropped or altered nonwhitespace characters")


def test_streaming_history_gate_rejects_viewport_only_and_replay():
    markers = ["STREAM_A_0001", "STREAM_A_0002"]
    runner.assert_streaming_history("STREAM_A_0001\nSTREAM_A_0002", "STREAM_A_0001\nSTREAM_A_0002", markers)
    for scrollback, transcript in [("", "STREAM_A_0001\nSTREAM_A_0002"),
                                  ("STREAM_A_0001", "STREAM_A_0001"),
                                  ("STREAM_A_0001\nSTREAM_A_0002", "STREAM_A_0001\nSTREAM_A_0002\nSTREAM_A_0001")]:
        try:
            runner.assert_streaming_history(scrollback, transcript, markers)
        except AssertionError:
            pass
        else:
            raise AssertionError("viewport-only, missing, or replayed streaming text accepted")


def test_markdown_capture_gate_rejects_raw_markup_broken_words_and_missing_style():
    import textwrap
    prose = textwrap.fill(runner.MARKDOWN_PROSE, width=120)
    clean = ("MD_WIDE_HEADING\nintentional emphasis inline_code\nPROSE_BEGIN_WIDE\n"
             + prose + "\nPROSE_END_WIDE\nFirst list item\nSecond list item\n"
             "System  Purpose\nMemory  Durable knowledge\nTools   Execute work\n    let preserved = 7;\nMD_WIDE_END")
    styled = "\x1b[1mMD_WIDE_HEADING\x1b[0m\n\x1b[1mintentional emphasis\x1b[0m\n\x1b[4minline_code\x1b[0m\nMD_WIDE_END"
    runner.assert_markdown_rendering(clean, styled, 0, 120)
    for invalid, style in ((clean.replace("MD_WIDE_HEADING", "## MD_WIDE_HEADING"), styled),
                           (clean.replace("intentional emphasis", "**intentional emphasis**"), styled),
                           (clean.replace("persistent", "persis\ntent", 1), styled),
                           (clean.replace("persistent structured", "persistent\nstructured", 1), styled),
                           (clean.replace("    let preserved", "let preserved"), styled),
                           (clean.replace("Tools   Execute", "Tools Execute"), styled),
                           (clean, "MD_WIDE_HEADING"),
                           (clean, styled.replace("\x1b[4m", "")),
                           (clean.replace(prose, textwrap.fill(runner.MARKDOWN_PROSE, width=72)), styled)):
        try:
            runner.assert_markdown_rendering(invalid, style, 0, 120)
        except AssertionError:
            pass
        else:
            raise AssertionError("Markdown capture accepted unrendered, corrupted, unstyled, or stale-width output")


def test_inline_working_status_rejects_response_interruption_and_missing_status():
    clean = "Published answer\nLive answer tail\n╭ model ─╮\n│ Ask anything │\n╰ Working · Ctrl+C cancel ─╯"
    runner.assert_inline_working_status(clean)
    for invalid in (clean.replace("Live answer tail", "Working · Ctrl+C cancel · F2 Project\nLive answer tail"),
                    clean.replace("Live answer tail", "Working · Ctrl+C cancel\nLive answer tail"),
                    clean.replace("Working · Ctrl+C cancel", ""),
                    clean + "\nF2 Project",
                    clean.replace("╭", " ")):
        try:
            runner.assert_inline_working_status(invalid)
        except AssertionError:
            pass
        else:
            raise AssertionError("inline status accepted response interruption or missing composer status")


def test_markdown_fixture_holds_all_blocks_before_completion():
    import queue
    import threading
    events = queue.Queue()
    with runner.fixture_provider() as server:
        server.markdown = True
        def consume():
            request = Request(server.url + "/v1/chat/completions", data=b'{"messages": []}', headers={"Content-Type": "application/json"})
            with urlopen(request, timeout=5) as response:
                for line in response:
                    if line.startswith(b"data: "):
                        events.put(line.decode())
        reader = threading.Thread(target=consume)
        reader.start()
        try:
            assert server.markdown_prefix_waiting.wait(2)
            prefix = ""
            while len(prefix) < len(runner.MARKDOWN_LIVE_PREFIX):
                event = json.loads(events.get(timeout=2)[6:])
                prefix += event["choices"][0]["delta"]["content"]
                assert event["choices"][0]["finish_reason"] is None
            assert prefix == runner.MARKDOWN_LIVE_PREFIX
            assert prefix.endswith("**pending emph") and reader.is_alive()
            server.release_markdown_prefix.set()
            for stage, label in enumerate(("WIDE", "NARROW", "GROWN")):
                assert server.stream_stages[stage].wait(2)
                content = ""
                while f"MD_{label}_END\n\n" not in content:
                    event = json.loads(events.get(timeout=2)[6:])
                    content += event["choices"][0]["delta"]["content"]
                    assert event["choices"][0]["finish_reason"] is None
                assert runner.markdown_fixture(stage) in content
                assert reader.is_alive(), "Markdown provider completed before release"
                server.release_stages[stage].set()
            assert '"finish_reason": "stop"' in events.get(timeout=2)
            assert "[DONE]" in events.get(timeout=2)
        finally:
            server.release_markdown_prefix.set()
            for release in server.release_stages:
                release.set()
            reader.join(5)
        assert not reader.is_alive()


def test_control_palette_rejects_missing_roles_stale_selection_and_colored_draft():
    panel = ("\x1b[0m  \n\x1b[38;5;255;48;5;235mPanel\n"
             "\x1b[38;5;252mLabel\n\x1b[38;5;255;48;5;240mSelected\n"
             "\x1b[38;5;248;48;5;235m~2k token budget\n\x1b[38;5;244mEsc closes\x1b[0m\n")
    captures = {name: panel for name in ("slash", "connect", "settings", "think", "help", "help-panel")}
    captures.update({"composer": "\x1b[38;5;244mAsk anything\x1b[0m",
                     "draft": "\x1b[0mCONTROLS_DRAFT_PROBE", "connect-moved": "\n" + panel})
    captures["connect"] = panel.replace("Selected", "OpenAI API")
    captures["connect-moved"] = ("\x1b[38;5;252;48;5;235mOpenAI API\n" + panel.replace("Selected", "Free hosted models"))
    runner.assert_control_palette(captures)
    for name, invalid in (("composer", "Ask anything"),
                          ("draft", "\x1b[38;5;244mCONTROLS_DRAFT_PROBE"),
                          ("connect-moved", panel),
                          ("connect", panel.replace("48;5;240", "48;5;235")),
                          ("think", panel.replace("38;5;248", "38;5;252")),
                          ("help", panel.replace("48;5;235", "48;5;234")),
                          ("help-panel", panel.replace("\x1b[0m  ", "\x1b[48;5;235m  "))):
        altered = dict(captures, **{name: invalid})
        try:
            runner.assert_control_palette(altered)
        except AssertionError:
            pass
        else:
            raise AssertionError(f"controls accepted broken palette/selection in {name}")


def test_ansi_spans_preserves_grouped_and_split_color_state():
    spans = runner.ansi_spans("\x1b[38;5;255m\x1b[48;5;240mSelected\n\x1b[39;49mCanvas")
    selected = next(span for span in spans if span["text"] == "Selected")
    canvas = next(span for span in spans if span["text"] == "Canvas")
    assert selected == {"row": 0, "text": "Selected", "fg": 255, "bg": 240}
    assert canvas == {"row": 1, "text": "Canvas", "fg": None, "bg": None}


if __name__ == "__main__":
    command = runner.tui_command(Path("/binary with spaces"), Path("/workspace"), Path("/log"), "inline", "full")
    assert "--tui" in command and "--ui" in command, "fixture must select both axes explicitly"
    assert command[command.index("--tui") + 1] == "inline"
    assert command[command.index("--ui") + 1] == "full"
    with tempfile.TemporaryDirectory() as folder, runner.fixture_provider() as provider:
        workspace = runner.prepare_fixture_workspace(Path(folder), provider)
        manifest = tomllib.loads((workspace / '.omegon/inference.toml').read_text())
        offering = manifest['offerings'][0]
        assert command[command.index('--model') + 1] == offering['id']
        assert offering['id'] == 'openai:omegon-tui-fixture', 'fixture must use a distinct offering with a supported provider prefix'
    test_provider_streams_distinct_turns_without_external_inference()
    test_provider_rejects_unknown_routes()
    test_provider_can_request_a_bounded_permission_probe()
    test_quiet_startup_gate_rejects_catalog_and_duplicate_summary()
    test_unconfigured_launch_has_no_implied_provider()
    test_unconfigured_startup_gate_rejects_phantom_route_metadata()
    test_unconfigured_cancel_gate_requires_one_unsent_draft()
    test_unconfigured_session_gate_rejects_admitted_prompts_and_model_requests()

    test_reply_readiness_rejects_active_placeholder_and_publication_overlap()
    test_runtime_idle_requires_matching_terminal_fact_for_each_started_turn()

    test_streaming_fixture_stages_hold_before_response_completion()
    test_streaming_history_gate_rejects_viewport_only_and_replay()

    test_streaming_fixture_interleaves_read_tool_and_followup_response()

    test_streaming_payload_preserves_nonwhitespace_across_wraps()

    test_markdown_capture_gate_rejects_raw_markup_broken_words_and_missing_style()
    test_markdown_fixture_holds_all_blocks_before_completion()

    test_inline_working_status_rejects_response_interruption_and_missing_status()

    test_control_palette_rejects_missing_roles_stale_selection_and_colored_draft()
    test_ansi_spans_preserves_grouped_and_split_color_state()

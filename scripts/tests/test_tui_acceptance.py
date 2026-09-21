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

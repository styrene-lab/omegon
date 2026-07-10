+++
title = "Operator Shell Observations and Terminal Rendering"
tags = ["tui","conversation","shell","provenance"]
+++

# Operator Shell Observations and Terminal Rendering

# Operator Shell Observations and Terminal Rendering

## Overview

Operator-prefixed `!` commands currently execute through the same Bash runtime and emit the same `ToolStart`/`ToolUpdate`/`ToolEnd` events as harness-invoked tools, but those events are only presentation/runtime events. The execution is not committed to canonical conversation state, so later model turns and restored sessions do not reliably retain the command or its result.

Completed Bash results also pass through source-syntax highlighting even though stdout/stderr is terminal output. The live renderer already parses ANSI SGR output with `ansi_to_tui`; completed output should use the same terminal-output projection.

## Decisions

### Preserve operator provenance

An operator-run command is canonical evidence, but it is not an assistant-authored tool call. Store it as an operator tool observation and project it to the model as a structured user-role observation. Do not fabricate assistant `tool_use` records or orphaned provider `tool_result` blocks.

### Use one canonical execution record

The record contains execution id, tool name, arguments, working directory, result content, exit status/error state, duration, and origin (`operator` / `bang_shell`). TUI, model-context, session, ACP, and WebSocket projections derive from this record or its provenance-bearing events.

### Separate command source from terminal output

The command is Bash source and may use syntax highlighting. stdout/stderr is terminal output: parse ANSI SGR styling, sanitize unsupported control sequences, and fall back to neutral monospace text. Reuse one renderer for live and completed output.

### Avoid implicit PTY semantics

The first implementation preserves colors already emitted by commands. It does not force TTY detection or introduce a PTY. PTY execution is a separate change because it alters buffering, terminal sizing, input, and cancellation behavior.

## Constraints

- Provider replay must remain structurally valid for Anthropic, OpenAI, and Gemini dialects.
- Model-visible text must identify the operator as initiator.
- Large command output must participate in normal context decay/truncation.
- ANSI and OSC/control bytes must never leak literally into rendered output or model context.
- Existing harness-invoked tool-call/result behavior must remain unchanged.
- Session snapshots created before this change must continue to load.

## Open Questions

- None for the first implementation slice. Rich PTY emulation and forced color are explicitly out of scope.

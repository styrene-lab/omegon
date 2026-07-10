+++
title = "Operator Shell Observations Design"
tags = ["openspec","design","shell","conversation"]
+++

# Operator Shell Observations Design

# Operator shell observations — Design

## Data model

Add an `OperatorToolObservation` canonical conversation entry rather than reusing `ToolResultEntry`. The latter is protocol-coupled to an assistant tool-call id and is repaired/stripped as part of provider tool-call pairing.

The observation carries:

- execution id
- tool name (`bash` for this slice)
- structured arguments
- working directory
- result content blocks
- error flag and exit code
- duration
- origin enum (`bang_shell` initially)

The entry uses the current conversation turn for decay accounting. Its LLM projection is a clearly attributed user-role observation with sanitized, bounded output. Decay produces a compact skeleton retaining command, cwd, status, and result summary.

## Runtime integration

The `RunShellCommand` handler currently spawns execution while the interactive command loop owns mutable `InteractiveAgentState`. Introduce a completion path back to the owning loop rather than mutating conversation from the spawned task. The worker continues emitting live events, then sends a completion command/event containing the canonical observation. The owner commits it, persists the session through the existing session path, and emits the final presentation event with provenance.

This preserves single-owner mutation and avoids placing conversation state behind a new lock.

## Semantic provenance

Extend the tool execution event/projection metadata with an execution origin. Existing harness tool events default to agent/harness origin; bang-shell execution is operator origin. Surface adapters consume the explicit field rather than infer provenance from `shell-*` ids.

Compatibility cost: changing shared event variants affects TUI, ACP, WebSocket serialization, audit logging, and tests. Prefer a backward-compatible optional/defaulted metadata structure where serialization boundaries require it.

## Rendering

Extract terminal-output conversion from the live tool-card path into a shared helper:

1. sanitize unsupported CSI/OSC/control sequences;
2. parse supported ANSI SGR through `ansi_to_tui`;
3. apply card background while preserving explicit ANSI foreground/modifiers;
4. use theme-muted foreground where ANSI provides none;
5. fall back to sanitized neutral text.

Completed Bash results call this helper before generic syntax/Markdown rendering. `try_highlight` no longer assigns Bash syntax based only on `tool_name == "bash"`; command arguments retain source highlighting separately.

## Persistence compatibility

Update session snapshot conversion rather than directly serializing `AgentMessage`. Add an optional/tagged persisted observation variant with serde defaults. Existing snapshots continue to deserialize unchanged. Round-trip tests cover both old and new shapes.

## Security and context limits

- Strip terminal controls before model projection.
- Preserve existing output limits from the Bash executor and conversation decay.
- Do not interpolate the command into executable content during replay; it is evidence text only.
- Clearly delimit command/output fields so command output cannot masquerade as harness instructions.

## Validation

Focused tests cover canonical projection, decay, provider-shape repair, session round-trip, event provenance, and live/completed renderer parity. Final gates are `cargo test -p omegon --locked`, `just lint`, `just test-rust`, and `just link`.

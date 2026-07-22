+++
id = "a206e447-140b-44f8-bc65-72f75499111a"
kind = "design_node"
title = "Universal Managed Execution Sessions and Process Viewer"
tags = ["execution", "process", "terminal", "shell", "tui", "architecture"]

[data]
status = "exploring"
issue_type = "architecture"
priority = 1
parent = "tui-surface-substrate-reevaluation"
dependencies = []
open_questions = [
  "[assumption] Existing terminal session state can be projected safely before the registry becomes instance-owned.",
  "What is the canonical raw event-log format for future terminal replay: bounded bytes, JSONL events, or asciicast-like records?",
  "Which terminal-state engine best satisfies alternate-screen, mouse, truecolor, and graphics requirements without coupling core semantics to Ratatui?",
  "Which operations should survive as specialized facades (serve, package installation) rather than converge on a public generic execution API?",
  "What transcript retention and redaction defaults are acceptable for secret-bearing commands?",
]
+++

# Universal Managed Execution Sessions and Process Viewer

## Overview

Define and implement Omegon core's universal managed execution substrate: process lifecycle, pipe- or PTY-backed I/O, bounded raw and text output, stable session handles, semantic process-viewer projections, and host-owned placement/controller affordances.

The governing model is:

```text
managed execution session
├── launch dialect: argv | shell
├── transport: pipes | PTY
├── lifecycle: queued | running | exited | failed | stopped
├── retained output: raw bytes + derived text viewport
├── interaction capabilities: read | write | resize | signal
└── presentation: conversation reference | process viewer | terminal viewport | external pane
```

Shell is a launch dialect, not a runtime substrate. Terminal is a PTY-backed capability, not a separate process universe. Extensions such as Reader own domain intent and structured argv but do not own process hosting, policy enforcement, transcript retention, terminal emulation, or host rendering.

## Boundary

### Omegon core owns

- Process creation, identity, lifecycle, cancellation, limits, and cleanup.
- Argv versus shell launch classification and permission policy.
- Pipe and PTY transports.
- Stable session handles and an instance-owned registry.
- Raw output retention, derived sanitized text, truncation metadata, and transcript policy.
- Input, signal, resize, and stop operations gated by capabilities.
- Semantic process-viewer snapshots and actions shared by TUI, ACP, and web clients.
- Placement negotiation among background, embedded, side-pane, tab, and external backends.
- Future terminal-state emulation and graphics capability reporting.

### Extensions own

- Domain-specific intent and executable selection.
- Structured argv and logical session/reuse identity.
- Minimum dependency versions and domain diagnostics.
- Required/preferred interaction, graphics, and placement capabilities.
- Domain degradation policy. Reader may choose Bookokrat print/extract, interactive PTY, external pane, or GUI fallback.

Extensions declare executable and host-action permissions. Core validates and executes them.

## Vocabulary

- **Command**: one execution request.
- **Process**: the operating-system child lifecycle.
- **Session**: a durable core handle around one process.
- **Shell command**: a command interpreted by a shell.
- **Terminal session**: a session backed by a PTY.
- **Process viewer**: read-only or lightly interactive presentation of session state.
- **Terminal viewport**: terminal-cell presentation backed by an emulator.
- **External pane**: a terminal surface delegated to another host or substrate.

## Iteration Plan

### Iteration 1 — semantic foundation

- Add renderer-neutral execution session and process-viewer projection types.
- Project the existing background terminal registry into stable session summaries and detail snapshots.
- Preserve current terminal tool and `terminal.create@1` behavior.
- Add focused tests for state, capabilities, output, provenance, and truncation metadata.

### Iteration 2 — read-only process viewer

- Add a centered scrollable TUI detail surface using existing modal geometry.
- Open it from terminal/session results and shell tool cards.
- Support follow, scroll, copy, operation switching, transcript open, and stop.
- Render explicit empty-output and truncation states.

### Iteration 3 — unified execution service

- Replace the static terminal registry with an instance-owned service.
- Introduce pipe- and PTY-backed transports.
- Give completed and live shell operations stable execution references.
- Route `bash`, `terminal`, `serve`, and HostAction terminal creation through shared lifecycle primitives where semantics align.

### Iteration 4 — controlled interactivity

- Add explicit input mode, input escape/release affordance, resize, and signal actions.
- Evaluate a terminal-state engine behind the same session contract.
- Keep graphics support capability-driven and permit external-pane fallback.

## Initial Decisions

1. Core owns managed execution and process viewing; Reader remains a thin domain extension.
2. Do not add a third model-facing `process` tool in the first iteration. Keep `bash` and `terminal` as façades over a converging internal service.
3. Canonical retention must not destructively strip PTY control bytes. Sanitized text is a derived projection.
4. Conversation cards reference execution sessions; they are not canonical transcript storage.
5. The first viewer is read-only. Interactive terminal semantics are an explicit later capability, not implied by a scrollable text modal.
6. Session persistence across Omegon process restarts is out of scope until a daemon or external substrate owns the child lifecycle.


+++
id = "extension-side-process-substrate-api"
kind = "document"
title = "Extension Side-Process Substrate API"
status = "exploring"
tags = ["extension", "tui", "workspace", "substrate", "process", "api"]
aliases = ["extension-side-process-substrate-api", "side-process-substrate-api", "extension-pane-substrate"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["tui-surface-substrate-reevaluation"]
issue_type = "epic"
open_questions = [
  "[assumption] Extension-requested side processes should be mediated by Omegon core rather than exposing Zellij, Cockpit, Kitty, or par-term directly to extensions.",
  "Should the first API be reader-specific (`ReaderPane`) or generic-but-constrained (`SideProcess`)?",
  "What capability negotiation contract lets extensions adapt to Zellij, Cockpit, Kitty, or fallback backends without depending on backend names?",
  "What command/path policy prevents this API from becoming arbitrary unreviewed process execution?",
  "What lifecycle guarantees are required: open-only, replace, close, focus, resize, status, persistence after Omegon exit?",
  "How should extension manifests declare side-process capabilities and command allowlists?",
  "What operator consent or install/setup flow is required when the preferred substrate is unavailable?"
]
related = [
  "tui-surface-substrate-reevaluation",
  "managed-reader-workspace",
  "reader-workspace-substrate-adapter",
  "cockpit-par-term-substrate-analysis",
  "tui-pane-focus-input-model",
  "tui-surface-migration-plan",
  "side-process-backend-terminal-compatibility-matrix"
]
+++

# Extension Side-Process Substrate API

## Overview

Design a substrate-swappable API for extensions that need adjacent or embedded side-process panes.

The immediate motivating extension is `omegon-reader`, which wants to open Bookokrat beside an active Omegon session. The broader architectural need is to avoid binding extension authors to a specific pane substrate such as Zellij, Cockpit, Kitty remote control, or par-term.

Extensions should request a capability. Omegon core should choose and police the substrate.

```text
Extension
  └── request side process / reader pane
      ↓
Omegon core capability boundary
  └── validates policy, paths, command, lifecycle intent
      ↓
Substrate adapter
  ├── Zellij backend
  ├── Cockpit backend
  ├── Kitty backend
  └── Fallback/setup instructions
```

## Design goal

Keep the extension API stable while substrate research continues.

The API must not leak backend-specific concepts like Zellij pane IDs, Cockpit `PaneWidget`s, or Kitty remote-control commands into extension code. Backend details belong in core adapters.

## Non-goals

- Do not expose a generic shell API to extensions.
- Do not let extensions pass shell fragments.
- Do not require extensions to know whether Omegon is running inside Zellij, Kitty, Ghostty, or an embedded Cockpit layout.
- Do not block v1 on proving every backend.
- Do not design a full remote terminal control plane before the reader workflow is validated.

## Candidate API levels

### Level 1: Reader-specific capability

Narrowest and safest initial surface.

```rust
ReaderPaneRequest {
    path: PathBuf,
    mode: ReaderMode,
    placement: PanePlacement,
    reuse: ReusePolicy,
}
```

Core owns translation to a command such as:

```text
bookokrat --zen-mode <path>
```

This keeps Bookokrat invocation policy in core or a reviewed adapter, not arbitrary extension text.

### Level 2: Declared side-process capability

A controlled generalization for extensions that declare allowed commands in their manifest.

```rust
SideProcessRequest {
    capability: CapabilityId,
    argv: Vec<OsString>,
    title: Option<String>,
    placement: PanePlacement,
    reuse: ReusePolicy,
    io_policy: IoPolicy,
}
```

The request is valid only if the extension manifest declares the capability and command policy.

### Level 3: Workspace-pane API

Future surface for richer lifecycle operations:

- open;
- replace;
- close;
- focus;
- query status;
- send signal;
- attach logs;
- persist or clean up on exit.

Do not expose this until Level 1/2 semantics are proven.

## Backend capability vocabulary

Extensions should reason about capabilities, not backend names.

Potential capability flags:

- `host_process` — can start an argv-based child process.
- `adjacent_pane` — can display beside Omegon.
- `embedded_pane` — pane is inside Omegon's TUI process.
- `external_workspace` — pane is managed by an external mux/workspace.
- `replace_named_pane` — can replace an existing pane by logical name.
- `close_pane` — can close a pane programmatically.
- `focus_pane` — can move operator focus.
- `preserve_after_exit` — child pane can outlive Omegon.
- `graphics_passthrough` — expected to preserve actual terminal graphics protocols.
- `mouse_passthrough` — can route child mouse mode reliably.
- `resize_propagation` — child PTY receives pane size changes.

## Substrate priority model

Initial policy can be simple and explicit:

1. If a managed external workspace is active and satisfies requested capabilities, use it.
2. Else if embedded panes are enabled and satisfy requested capabilities, use Cockpit/par-term backend.
3. Else if a terminal-specific backend is explicitly configured, use it.
4. Else return a structured unavailable result with setup instructions.

The policy should be configurable, but the extension should not choose a backend directly unless debugging/development mode explicitly allows it.

## Security posture

The API is a process-launch boundary. Treat it as security-sensitive.

Required constraints:

- All commands are argv arrays, never shell strings.
- Paths are validated before launch.
- Extension manifests declare side-process permissions.
- Core may restrict commands to named capabilities rather than arbitrary binaries.
- Operator-facing errors avoid leaking sensitive paths unnecessarily.
- Backends must not require `sh -c` for normal operation.
- Environment inheritance is explicit and minimal.

## Decision target

Produce an ADR answering:

```text
What substrate-swappable side-process API should extensions call,
and which backend should Omegon use first for the reader workflow?
```

Expected likely answer:

- Ship a narrow reader/side-process capability first.
- Default to Zellij or external workspace where available.
- Keep Cockpit/par-term as an experimental embedded backend.
- Hide backend choice behind core capability negotiation.

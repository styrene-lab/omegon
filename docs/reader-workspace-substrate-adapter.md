+++
id = "reader-workspace-substrate-adapter"
kind = "document"
title = "Reader Workspace Substrate Adapter"
status = "seed"
tags = ["terminal", "reader", "workspace", "adapter", "architecture"]
aliases = ["reader-workspace-substrate-adapter"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["managed-reader-workspace"]
open_questions = [
  "[assumption] A small substrate adapter is enough for v1 and avoids exposing arbitrary terminal-control power to extensions.",
  "Should this adapter be a core API, an omegon-reader-local module, or a reusable helper crate for future terminal workspace features?",
  "What state must persist across turns: session name, pane ID, pane name, command, last opened path, or nothing?",
  "How should errors be represented to extensions so operator-facing messages remain actionable but do not leak internal paths unnecessarily?"
]
parent = "managed-reader-workspace"
related = ["reader-workspace-zellij-spike", "reader-workspace-security-licensing"]
+++

# Reader Workspace Substrate Adapter

## Overview

Define the minimal Omegon-facing abstraction for managed terminal workspace operations. This adapter should let reader workflows request a side pane without binding the product API directly to Zellij command syntax.

The adapter is intentionally narrow. It is not a general remote terminal-control API.

## Candidate interface

Conceptual shape:

```rust
trait ReaderWorkspace {
    fn detect(&self) -> WorkspaceStatus;
    fn ensure_session(&self) -> Result<SessionHandle>;
    fn open_reader(&self, path: &Path) -> Result<PaneHandle>;
    fn replace_reader(&self, path: &Path) -> Result<PaneHandle>;
    fn close_reader(&self) -> Result<()>;
}
```

Possible status model:

```rust
enum WorkspaceStatus {
    Unsupported,
    SupportedButNotInstalled,
    InstalledButOutsideSession,
    InsideManagedSession { session: SessionHandle },
    InsideUnmanagedSession,
}
```

Possible pane state model:

```rust
enum ReaderPaneState {
    NotOpen,
    Open { pane: PaneHandle },
    Exited { status: Option<i32> },
    Unknown,
}
```

## Research points

- Identify the smallest operation set needed by `omegon-reader` v1.
- Determine whether pane replacement requires stable pane IDs or can use names/layout conventions.
- Determine whether session/pane handles should be persisted in Omegon state, extension state, or derived dynamically.
- Define command construction rules that avoid shell interpolation.
- Define a testable command-builder layer separate from terminal integration behavior.

## Initial constraints

- Do not expose an arbitrary `run shell command in pane` extension API in v1.
- Prefer command plus argv over string commands.
- Treat paths as paths, not shell fragments.
- Keep Bookokrat-specific behavior above the generic substrate adapter where possible.

## Decisions

### Decision: Adapter API must be capability-specific for v1

**Status:** proposed

**Rationale:** A broad terminal-control API would create unnecessary security and support burden. The v1 need is specifically adjacent reader panes, so the adapter should expose reader/workspace operations rather than arbitrary pane command execution.

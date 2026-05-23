+++
id = "managed-reader-workspace"
kind = "document"
title = "Managed Reader Workspace"
status = "exploring"
tags = ["terminal", "reader", "workspace", "zellij", "pty"]
aliases = ["managed-reader-workspace", "terminal-pane-reader-workspace"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
issue_type = "epic"
open_questions = [
  "[assumption] Zellij can open a Bookokrat side pane from an active Omegon pane while preserving interactivity in the original pane.",
  "[assumption] Bookokrat works acceptably inside Zellij under both Ghostty and Kitty for the document formats Omegon Reader targets.",
  "Should the first product branch be an external managed workspace, an embedded Cockpit pane inside Omegon, or should both be kept as separate modes?",
  "Should v1 require an explicit `omegon reader session` entrypoint, or may `omegon reader open <path>` automatically bootstrap/re-exec into a managed workspace?",
  "Can the substrate provide stable enough pane identity for close/replace, or is v1 allowed to be open-only with documented duplicate-pane behavior?",
  "Should Zellij be user-installed, managed-installed by Omegon, or bundled/pinned as an Omegon-managed dependency?",
  "What is the minimum safe extension/core API boundary for requesting adjacent reader panes without exposing arbitrary shell execution?"
]
related = ["omegon-native-terminal-pane-crate-analysis", "reader-workspace-substrate-adapter", "reader-workspace-zellij-spike", "reader-workspace-ux-contract", "reader-workspace-security-licensing", "reader-workspace-embedded-pty-alternatives", "tui-surface-substrate-reevaluation", "cockpit-par-term-substrate-analysis", "par-term-emu-core-rust-reader-pane-analysis", "extension-side-process-substrate-api", "reader-extension-side-pane-contract"]
+++

# Managed Reader Workspace

## Overview

Define Omegon's architecture for opening an adjacent live terminal pane running an external reader such as Bookokrat while keeping the original Omegon CLI pane interactive.

This node owns the product and architecture decision for the managed workspace substrate. The immediate use case is `omegon-reader`: open `bookokrat <path>` beside the current Omegon session without requiring the operator to manually orchestrate terminal panes.

The original preferred direction was a narrow managed-workspace model:

```text
Ghostty or Kitty
  └── managed workspace substrate, likely Zellij
        ├── Omegon CLI pane
        └── Bookokrat reader pane
```

Kitty and Ghostty are outer terminal emulators, not v1 pane-control backends. Bookokrat remains an external executable, not linked or vendored into Omegon.

A second branch is now credible after the Cockpit smoke prototype:

```text
Ghostty or Kitty
  └── Omegon TUI
        ├── Omegon conversation/input region
        └── embedded Cockpit PTY pane running Bookokrat
```

This embedded branch changes ownership. Omegon would own child PTY lifecycle, terminal emulation/rendering, focus routing, layout, and graphics-protocol limitations. It should be evaluated as a separate product mode, not as a drop-in replacement for the Zellij workspace model.

## Boundaries

### Owned here

- Reader/workspace product contract.
- Supported v1 substrate decision.
- Workspace bootstrap/attach flow.
- Pane lifecycle expectations: open, replace, close, observe failure.
- Extension/core boundary for requesting reader panes.
- Research gates for Zellij and embedded PTY alternatives.

### Not owned here

- Terminal conversation rendering inside Omegon's TUI. That belongs to `conversation-rendering-engine`.
- Browser display/Auspex artifact rendering.
- Bookokrat internals or source integration.
- Generic terminal emulator implementation.
- A broad backend matrix across tmux, Kitty remote control, Ghostty-specific APIs, WezTerm, and Zellij.

## Proposed v1 contract

`omegon reader session` starts or attaches to a supported managed workspace. Inside that workspace, `omegon reader open <path>` opens or replaces a named side pane running `bookokrat <path>`. The original Omegon pane remains usable.

If v1 cannot safely implement replacement, the v1 contract may be weakened to open-only, but that limitation must be explicit in UI and docs.

## Research points required before decision

### Product/UX

- Decide whether v1 uses explicit session bootstrap (`omegon reader session`) or automatic bootstrap from `omegon reader open <path>`.
- Define operator-facing behavior when Zellij is missing.
- Define operator-facing behavior when Bookokrat is missing.
- Define behavior when not currently inside a managed workspace.
- Decide whether duplicate reader panes are acceptable in v1.
- Define how the operator exits reader mode or closes the side pane.

### Substrate control

- Verify Zellij can start/attach a named session suitable for Omegon Reader.
- Verify Omegon can detect whether it is already inside a Zellij session.
- Verify Zellij can open a side pane with command argv without shell interpolation.
- Verify pane naming/identity sufficient for close/replace.
- Verify pane resize behavior reaches Bookokrat.
- Verify Bookokrat exiting does not kill or corrupt the Omegon pane.

### Terminal behavior

- Test inside Ghostty.
- Test inside Kitty.
- Test alternate-screen behavior.
- Test keyboard routing and focus behavior.
- Test mouse routing if Bookokrat relies on mouse input.
- Test truecolor and relevant graphics protocols/fallbacks.

### Security and licensing

- Confirm Zellij license and redistribution/managed-install compatibility.
- Confirm Bookokrat AGPL boundary remains process-only.
- Avoid shell interpolation for paths.
- Define path validation and error handling.
- Review any daemon/control socket authorization model used by the substrate.

### Implementation architecture

- Define `ReaderWorkspace`/substrate adapter contract.
- Decide whether workspace support lives in core, an extension helper crate, or `omegon-reader` only.
- Define state persistence for session/pane handles.
- Define test strategy: unit tests for command construction and integration/manual spike for terminal behavior.

## Child design nodes

- `reader-workspace-substrate-adapter` — the minimal Omegon-facing abstraction for workspace/pane control.
- `reader-workspace-zellij-spike` — evidence collection for Zellij under Ghostty and Kitty.
- `reader-workspace-ux-contract` — operator command flow, bootstrap behavior, and failure UX.
- `reader-workspace-security-licensing` — process-spawn, path, control-protocol, and license boundaries.
- `reader-workspace-embedded-pty-alternatives` — timeboxed research into embedded Rust PTY/multiplexer crates.

## Decisions

### Decision: Treat managed reader panes as workspace orchestration, not conversation rendering

**Status:** proposed

**Rationale:** The target is a live sibling terminal pane running an arbitrary TUI subprocess. That is PTY/workspace orchestration, not Omegon conversation segment rendering. Keeping this separate prevents the conversation rendering engine from inheriting terminal multiplexer responsibilities.

### Decision: Zellij is the primary v1 candidate

**Status:** proposed

**Rationale:** Zellij most closely matches the desired boundary: an external Rust-native workspace/multiplexer that owns real PTYs, pane layout, input routing, resize propagation, and alternate-screen behavior. This avoids building terminal hosting into Omegon and avoids a broad Kitty/Ghostty/tmux backend matrix.

### Decision: Bookokrat remains an external process

**Status:** proposed

**Rationale:** Bookokrat is AGPL-3.0-or-later. Keeping it as an external executable preserves a cleaner licensing and architectural boundary. Omegon should pass a file path to `bookokrat` rather than linking or vendoring Bookokrat code.

### Decision: Cockpit embedded pane is a credible architecture branch

**Status:** proposed

**Rationale:** A scratch prototype using `cockpit = "0.2.2"` successfully hosted `/usr/bin/vi` as a full-screen child TUI in a PTY pane. The operator validated focus switching, keyboard routing, file editing, save, and exit. This proves Cockpit is feasible enough for an embedded-reader branch. It does not prove Bookokrat compatibility, image/PDF behavior, mouse behavior, or integration with Omegon's production TUI layout.

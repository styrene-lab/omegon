+++
id = "tui-surface-substrate-reevaluation"
kind = "document"
title = "TUI Surface Substrate Re-evaluation"
status = "exploring"
tags = ["tui", "architecture", "ratatui", "cockpit", "par-term", "surface"]
aliases = ["tui-surface-substrate-reevaluation", "tui-substrate-reevaluation"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
issue_type = "epic"
open_questions = [
  "[assumption] Omegon's current TUI surface problems are substrate/architecture problems, not only local layout polish problems.",
  "Which surfaces should remain first-class Ratatui widgets, and which should become managed panes or embedded terminal surfaces?",
  "Can Cockpit or its par-term upstream provide a better primitive for operator-controlled panes than hand-built Ratatui sidebars/footers?",
  "Does adopting a pane substrate reduce complexity, or does it introduce a second terminal runtime that fights Omegon's existing event loop?",
  "Should dashboard/sidebar/footer surfaces be persistent chrome, demand-driven panes, or command-opened workspaces?",
  "What compatibility guarantees are required for Ghostty, Kitty, SSH, and non-graphics terminals?"
]
related = [
  "conversation-rendering-engine",
  "tui-hud-redesign",
  "tui-surface-pass",
  "tui-design-tree-widget",
  "tui-footer-engine-display",
  "managed-reader-workspace",
  "reader-workspace-embedded-pty-alternatives",
  "omegon-native-terminal-pane-crate-analysis",
  "extension-side-process-substrate-api"
]
+++

# TUI Surface Substrate Re-evaluation

## Overview

Re-evaluate Omegon's terminal UI architecture from the substrate upward.

The working concern is that Omegon has been using Ratatui layouts as a hammer: sidebars, dashboard panels, footer cards, design-node lists, git trees, and status areas are all hand-composed inside one monolithic TUI layout. That approach works for bounded widgets, but it may be the wrong primitive for surfaces that behave more like independent panes, tools, inspectors, or terminal workspaces.

This node asks whether Omegon should keep treating every UI region as an in-process Ratatui widget, or introduce a pane/workspace substrate such as Cockpit/par-term for surfaces that need independent lifecycle, focus, scrolling, process hosting, or richer terminal behavior.

## Problem statement

Current TUI surfaces are coupled through shared layout, focus, and render loops:

- Conversation stream.
- Editor/input area.
- Dashboard/sidebar.
- Design tree widget.
- Git tree widget.
- Footer/status engine.
- Tool/artifact displays.
- Future reader/Bookokrat pane.

The result is pressure to solve every UX need by adding another Ratatui region, footer row, sidebar section, selector overlay, or mode flag. This creates local wins but increases global complexity.

## Hypothesis

Some Omegon surfaces are not widgets. They are panes.

A widget is appropriate when the surface is small, passive, and directly coupled to the active conversation state. A pane may be appropriate when the surface has its own focus model, scrollback, lifecycle, child process, keyboard routing, or long-lived state.

## Candidate substrate split

```text
Omegon TUI shell
  ├── Core Ratatui surfaces
  │   ├── conversation stream
  │   ├── editor/input
  │   └── compact status line
  │
  ├── Managed in-process panes
  │   ├── design tree inspector
  │   ├── git/worktree inspector
  │   ├── lifecycle dashboard
  │   └── tool/artifact viewport
  │
  └── Embedded terminal/process panes
      ├── Bookokrat reader
      ├── shell/task runners
      └── future interactive tools
```

The re-evaluation should determine whether Cockpit/par-term can own the middle or bottom layers, or whether Ratatui should remain the only in-process rendering substrate.

## Initial position

Do not assume Cockpit is automatically better. The architectural question is whether it gives Omegon a cleaner primitive than hand-built Ratatui layout regions.

Known evidence so far:

- Cockpit can embed a real PTY child pane in a Ratatui app.
- A scratch prototype hosted `vi` successfully.
- A two-column prototype hosted Bookokrat EPUB text successfully.
- `par-term-emu-core-rust` appears more relevant than Cockpit for deeper graphical terminal behavior.
- Half-block fallback is not equivalent to actual terminal image/graphics protocol rendering.

## Evaluation dimensions

### Architecture

- Does the substrate reduce coupling in `tui/mod.rs`, or move complexity elsewhere?
- Can panes be composed without fighting Ratatui's ownership of the terminal frame?
- Can existing surfaces migrate incrementally?
- Does it preserve Omegon's current slim/full UI modes?

### Interaction

- Can focus move predictably between conversation, editor, panes, and overlays?
- Can keyboard shortcuts remain reliable across Kitty/Ghostty/SSH?
- Can mouse routing work without fragile coordinate translation?
- Can panes expose discoverable close/replace/toggle behavior?

### Rendering

- Does the substrate support normal Ratatui widgets, child PTYs, or both?
- Does it preserve alternate-screen child apps?
- Does it support terminal graphics protocols or only text-cell approximations?
- Can it render design/lifecycle surfaces with better scrollback and containment than ad hoc sidebars?

### Operations

- What dependencies become part of Omegon core?
- Are Cockpit/par-term licenses compatible?
- Are upstream APIs stable enough?
- Can failures in one pane be isolated from the main agent loop?

## Child design nodes

- `tui-surface-inventory-taxonomy` — classify current TUI surfaces as widgets, panes, overlays, or external workspaces.
- `cockpit-par-term-substrate-analysis` — evaluate Cockpit and par-term as substrate candidates for Omegon-owned panes.
- `tui-pane-focus-input-model` — define focus, keyboard, mouse, and lifecycle rules for multi-pane TUI operation.
- `tui-chrome-vs-workspace-boundary` — decide what remains persistent chrome versus what becomes demand-driven panes.
- `tui-surface-migration-plan` — define safe incremental migration experiments and rollback criteria.

## Decision target

Produce an ADR answering:

```text
Should Omegon continue using pure Ratatui layout composition for all TUI surfaces,
or should it introduce a pane/workspace substrate for selected surfaces?
```

The answer may be hybrid. The goal is not to replace Ratatui blindly; it is to stop using the same widget/layout primitive for every operator surface if a better system boundary exists.

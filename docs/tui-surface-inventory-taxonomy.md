+++
id = "tui-surface-inventory-taxonomy"
kind = "document"
title = "TUI Surface Inventory and Taxonomy"
status = "seed"
tags = ["tui", "architecture", "inventory", "surfaces"]
aliases = ["tui-surface-inventory-taxonomy"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "tui-surface-substrate-reevaluation"
dependencies = ["tui-surface-substrate-reevaluation"]
open_questions = [
  "Which existing surfaces are true widgets versus panes versus overlays?",
  "Which surfaces require independent scrollback, focus, lifecycle, or child-process hosting?",
  "Which surfaces are currently visible because they are important, and which are visible only because persistent chrome was the available layout primitive?",
  "Which current dashboard/sidebar/footer elements should become on-demand inspectors instead of always-present regions?"
]
related = ["tui-surface-pass", "tui-design-tree-widget", "tui-footer-engine-display", "conversation-rendering-engine"]
+++

# TUI Surface Inventory and Taxonomy

## Overview

Classify Omegon's current and planned terminal surfaces by the primitive they actually need.

The intent is to prevent category errors: not every surface should be a sidebar, footer card, dashboard section, selector overlay, or conversation message.

## Candidate taxonomy

### Core shell

Surfaces that must remain tightly coupled to the main agent loop:

- Conversation stream.
- Editor/input box.
- Minimal status line for model/context/busy/error state.

### Widgets

Small passive regions that render state and do not own meaningful interaction:

- Compact context gauge.
- Current model/provider indicator.
- Git dirty badge.
- Tool-call counter.

### Overlays

Short-lived modal choices or command outputs:

- Model selector.
- Persona/tone selector.
- Auth status table.
- Memory/source picker.

### Inspectors

Interactive surfaces that need navigation, scrollback, search, expand/collapse, or focus:

- Design tree.
- Git tree/worktree view.
- OpenSpec/lifecycle dashboard.
- Memory graph or fact browser.
- Tool result/artifact browser.

### Embedded terminal/process panes

Surfaces that host another TUI or process:

- Bookokrat reader.
- Shell/task runner.
- Local server logs.
- Long-running command monitor.

### External workspace panes

Surfaces delegated to a terminal multiplexer/workspace outside Omegon:

- Zellij-managed Bookokrat pane.
- Future managed task shells.

## Research tasks

1. Inventory current surfaces from `core/crates/omegon/src/tui/` and existing design docs.
2. Assign each surface to the taxonomy.
3. Mark surfaces whose current implementation primitive appears mismatched.
4. Identify which mismatches create operator-visible pain versus only code complexity.
5. Feed candidates into the migration plan.

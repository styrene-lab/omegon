+++
id = "tui-chrome-vs-workspace-boundary"
kind = "document"
title = "TUI Chrome versus Workspace Boundary"
status = "seed"
tags = ["tui", "ux", "chrome", "workspace", "dashboard"]
aliases = ["tui-chrome-vs-workspace-boundary"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "tui-surface-substrate-reevaluation"
dependencies = ["tui-surface-substrate-reevaluation", "tui-surface-inventory-taxonomy"]
open_questions = [
  "What information must be persistent chrome versus available on demand?",
  "Should the design tree and lifecycle dashboard be always-visible sidebars, toggleable inspectors, or separate panes?",
  "Should the footer remain a dense status surface or collapse into a minimal command/status line?",
  "How does slim mode relate to a pane/workspace model?",
  "What surfaces should be hidden by default to preserve conversation focus?"
]
related = ["tui-footer-engine-display", "tui-hud-redesign", "tui-surface-pass"]
+++

# TUI Chrome versus Workspace Boundary

## Overview

Decide what belongs in persistent TUI chrome and what should become an on-demand workspace surface.

Omegon has accumulated status panels, sidebars, dashboard sections, and footer cards because those were the available primitives. This node reconsiders whether persistent visibility is actually the right UX.

## Candidate rule

Persistent chrome should answer only:

- Am I busy?
- Which engine/context am I using?
- Is anything broken or blocked?
- Where can I type next?

Everything else should justify why it is always visible instead of being an inspector, overlay, or pane.

## Surfaces to reassess

- Footer engine/status display.
- Dashboard sidebar.
- Design tree widget.
- Git tree widget.
- Lifecycle/OpenSpec progress.
- Memory/mind indicators.
- Tool/artifact displays.
- Fractal/status ornamentation.

## Research tasks

1. Identify persistent chrome currently visible in slim and full modes.
2. Separate operationally critical status from ambient telemetry.
3. Define on-demand inspector candidates.
4. Define how operators discover hidden surfaces.
5. Propose a minimal default chrome contract.

+++
id = "tui-surface-migration-plan"
kind = "document"
title = "TUI Surface Migration Plan"
status = "seed"
tags = ["tui", "migration", "architecture", "experiment"]
aliases = ["tui-surface-migration-plan"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "tui-surface-substrate-reevaluation"
dependencies = [
  "tui-surface-substrate-reevaluation",
  "tui-surface-inventory-taxonomy",
  "cockpit-par-term-substrate-analysis",
  "tui-pane-focus-input-model",
  "tui-chrome-vs-workspace-boundary"
]
open_questions = [
  "What is the smallest reversible experiment that tests a pane substrate inside Omegon?",
  "Which surface is the safest first migration candidate?",
  "What rollback criteria prove the substrate is not worth adopting?",
  "Should experiments live behind a feature flag, slash command, or separate prototype binary?",
  "What tests can cover pane focus/layout behavior without brittle terminal snapshots?"
]
related = ["tui-surface-substrate-reevaluation", "cockpit-par-term-substrate-analysis"]
+++

# TUI Surface Migration Plan

## Overview

Define incremental experiments for moving selected Omegon TUI surfaces away from ad hoc Ratatui layout composition if the substrate re-evaluation supports it.

The migration must be reversible. Omegon's main TUI should not be destabilized by a substrate experiment.

## Candidate experiments

### Experiment 0: Native-scrollback conversation shell

Historical proposal from 2026-08-11, retained from the native-inline branch.
The production presentation contract now lives in `docs/tui-presentations.md` and
`openspec/changes/tui-dual-presentation/`. The proposal below records the earlier
experiment, not the current implementation status.

Promote the validated standalone inline-viewport probe into a production-shaped
shell behind an explicit experimental entry point. Publish only finalized
conversation segments into native terminal scrollback; keep the active composer
and compact status line in a small Ratatui `Viewport::Inline` region.

The next experiment must reuse production conversation projections rather than
mock strings and must cover:

- wrapped-height calculation at the current terminal width;
- publication of user, assistant, and completed tool segments exactly once;
- resize behavior before and after publication;
- inline artifacts and overlays that cannot become immutable scrollback;
- startup/restore behavior when cursor-position queries are unavailable;
- native interaction checks in macOS Terminal, Ghostty, Kitty, and SSH.

Keep this behind a separate binary or explicit experimental mode until transcript
ordering and resize invariants have automated coverage. Do not reintroduce a
right-side drawer as part of this experiment: changing conversation width forces
historical reflow and undermines the stable native-scrollback model being tested.

### Experiment A: Prototype binary

Create a standalone prototype that hosts:

```text
left: Omegon conversation/editor mock
right: one inspector pane or child PTY pane
```

Use this to test focus, resize, and event routing without touching production TUI code.

### Experiment B: Embedded reader pane behind feature flag

Use the existing Cockpit/Bookokrat spike as the first real process-pane candidate. Keep it separate from the default Zellij/external workspace path until image/PDF and input behavior are known.

### Experiment C: Design tree inspector as on-demand pane

Move design tree from persistent dashboard/sidebar posture into a toggleable inspector pane. This tests non-PTY pane value without graphics/Bookokrat complexity.

### Experiment D: Footer collapse

Replace current dense footer with minimal status chrome plus on-demand status inspector. This tests the chrome/workspace boundary rather than the terminal substrate itself.

## Rollback criteria

Reject or defer substrate adoption if:

- Input routing becomes less predictable than current Ratatui focus handling.
- The prototype requires invasive rewrites of the main TUI event loop before value is proven.
- Child PTY panes cannot coexist with Omegon shortcuts safely.
- Graphics/image behavior is worse than external workspace delegation.
- The dependency/API surface is unstable or poorly maintained.

## Decision output

The migration plan should end with one of:

- Continue pure Ratatui, but simplify chrome.
- Adopt Cockpit/par-term only for embedded process panes.
- Adopt pane substrate for inspectors and process panes.
- Defer substrate adoption; use Zellij/external workspace for process panes.

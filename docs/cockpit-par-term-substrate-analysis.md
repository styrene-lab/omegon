+++
id = "cockpit-par-term-substrate-analysis"
kind = "document"
title = "Cockpit and par-term Substrate Analysis"
status = "seed"
tags = ["tui", "cockpit", "par-term", "pty", "terminal", "substrate"]
aliases = ["cockpit-par-term-substrate-analysis", "par-term-substrate-analysis"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "tui-surface-substrate-reevaluation"
dependencies = ["tui-surface-substrate-reevaluation", "reader-workspace-embedded-pty-alternatives"]
open_questions = [
  "What exact responsibilities does Cockpit own versus par-term-emu-core-rust?",
  "Can Cockpit/par-term host Omegon-owned inspector panes, or only child PTY panes?",
  "Does par-term preserve real terminal graphics protocols, decode them, drop them, or approximate them as text cells?",
  "Can the substrate coexist cleanly with Omegon's current Ratatui/crossterm event and render loop?",
  "Are Cockpit/par-term APIs stable enough for core adoption or only experimental branches?",
  "What license and maintenance posture apply to both Cockpit and par-term?"
]
related = ["reader-workspace-embedded-pty-alternatives", "omegon-native-terminal-pane-crate-analysis", "managed-reader-workspace"]
+++

# Cockpit and par-term Substrate Analysis

## Overview

Evaluate Cockpit and its par-term upstream as possible TUI substrate primitives for Omegon.

This is broader than the Bookokrat reader question. The design question is whether Cockpit/par-term can help Omegon stop hand-building every surface as a Ratatui layout region.

## Known evidence

Cockpit smoke work has already shown:

- A Ratatui prototype can create Cockpit panes.
- Cockpit can create real PTY-backed child panes.
- A full-screen `vi` child can run, receive keyboard input, write a file, and exit.
- Omegon can own the outer two-column layout and render a Cockpit `PaneWidget` inside one region.
- Bookokrat EPUB text mode can render in the embedded pane.
- Bookokrat `--zen-mode` improves embedded-pane usability by removing its own sidebar.

Memory/recent-session evidence also indicates:

- `par-term-emu-core-rust` is more relevant than Cockpit for graphical embedded terminal work.
- Half-block fallback is not equivalent to actual pixel/raster imagery through terminal graphics protocols.

## Key distinction

Cockpit may be the pane manager. par-term may be the terminal-emulation/rendering core.

That distinction matters because Omegon has two separate needs:

1. Pane/workspace composition: focus, lifecycle, sizing, child PTYs.
2. Terminal rendering fidelity: alternate screen, mouse, colors, image/graphics protocols, raster output behavior.

A successful `vi` or EPUB-text prototype validates part of (1), but not all of (2).

## Research tasks

1. Map the dependency relationship between Cockpit and par-term.
2. Identify public APIs Omegon would depend on.
3. Determine whether Cockpit can host non-PTY Omegon inspector panes cleanly, or whether it is only useful for child processes.
4. Determine how par-term handles:
   - Kitty graphics protocol;
   - Sixel;
   - iTerm2 inline images;
   - hyperlinks/OSC sequences;
   - mouse reporting;
   - alternate screen;
   - resize propagation.
5. Verify whether graphics are passed through, decoded into an internal image model, rendered as half-block fallback, or dropped.
6. Compare integration complexity against continuing pure Ratatui composition.
7. Produce a small recommendation matrix:
   - use now;
   - spike only;
   - avoid for core;
   - use only for reader/process panes.

## Acceptance criteria

This node is ready to decide when it can answer:

```text
Is Cockpit/par-term a general Omegon TUI substrate,
a narrow embedded-process-pane substrate,
or only a useful research prototype?
```

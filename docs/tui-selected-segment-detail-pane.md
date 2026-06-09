+++
title = "TUI Selected Segment Detail Pane"
tags = ["tui","ratatui","conversation","widgets"]
+++

# TUI Selected Segment Detail Pane

---
title: TUI Selected Segment Detail Pane
status: seed
tags: [tui, ratatui, conversation, widgets]
---

# TUI Selected Segment Detail Pane

## Overview

Implement a first native Ratatui selected-segment detail pane using existing primitives and the newly added semantic conversation segment actions.

This is the recommended first target from [[tui-ui-landscape-widget-map]]. It gives tool-card args/results and long assistant/user/system text a dedicated surface without replacing the main conversation stream or adding a new widget dependency before the UX is proven.

## Why this first

- It exercises `UiAction::SelectConversationSegment` and `UiAction::OpenConversationSegmentDetail` in real UI work.
- It improves the highest-friction segment type: `ToolCard` with long args/results/progress/errors.
- It creates a bounded place to evaluate `tui-scrollview` later.
- It avoids MSRV risk from new dependencies in the first pass.
- It keeps the mixed conversation transcript custom while moving deep inspection into a pane.

## First implementation scope

### In scope

- Add a small detail renderer module, likely `core/crates/omegon/src/tui/segment_detail.rs` or `core/crates/omegon/src/tui/segment_components/detail.rs`.
- Render a detail pane when `conversation.timeline_expanded_segment()` points at a valid segment.
- Populate the pane from the opened segment:
  - Tool cards: name, id, status, args summary/detail, result summary/detail, live progress/error state.
  - Assistant/user/system/lifecycle: text body and basic metadata.
  - Image: path/alt metadata and placeholder; rich image rendering can follow later.
- Use existing Ratatui `Block`, `Paragraph`, `Wrap`, and layout primitives.
- Keep scroll/focus traversal local.
- Add tests around state/render text where stable; avoid brittle full-frame snapshots unless existing test helpers make that safe.

### Out of scope for v1

- Adding `tui-scrollview`.
- Replacing the main conversation renderer.
- External/ACP/TS protocol exposure.
- Full artifact/image rendering in the detail pane.
- Embedded terminal/process panes.
- Reworking `ToolCard.expanded` globally.

## Proposed layout

First pass should prefer a bottom detail pane in focus/full modes because it avoids competing with the existing dashboard/sidebar on narrow terminals.

Potential rule:

```text
if detail target open and terminal height >= threshold:
  conversation area shrinks vertically
  detail pane renders below conversation/editor boundary or as a bounded panel above editor
else:
  existing inline expansion behavior remains the fallback
```

The exact placement should be verified against current `layout_projection.rs` before implementation.

## Acceptance criteria

- Focus-mode Enter opens/toggles a visible detail pane for the focused segment.
- Tool-card detail pane includes full args/result when available.
- Invalid/stale segment index degrades gracefully.
- Existing `/ui`, focus traversal, permission lane, and editor tests continue to pass.
- No new dependency is added in v1.
- Changelog is updated.

## Follow-up candidates

1. Evaluate `tui-scrollview` for pane body scrolling if local rendering becomes awkward and MSRV allows.
2. Add `tui-markdown` rendering for assistant/tool detail bodies.
3. Add `ratatui-image` detail rendering for image segments.
4. Add tachyonfx open/selection pulse once the pane interaction is stable.

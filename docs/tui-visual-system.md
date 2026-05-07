+++
id = "704bf110-83bd-4879-b4c3-bfa3f4377a7b"
kind = "document"
title = "TUI visual system — conversation view, event cards, widget primitives"
status = "implemented"
tags = ["tui", "ratatui", "visual", "conversation"]
aliases = ["tui-visual-system"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "harness-honest-assessment"
+++

# TUI visual system — conversation view, event cards, widget primitives

## Overview

Design a consistent visual component system for the Omegon TUI. The conversation view needs proper rendering of all event types (tool calls, tool results, streaming text, errors, system messages). Widget primitives must be DRY and themeable via the Alpharius palette. Cards/panels for dashboard, footer, and inline conversation elements should share layout primitives.

## Research

### Current state audit — 1,871 LoC across 7 TUI modules

**What exists:**
- `theme.rs` (125 LoC): Trait-based Alpharius palette with semantic colors and derived styles. Solid foundation.
- `conversation.rs` (291 LoC): Flat enum of User/System/Assistant/Tool messages. Renders to Vec<Line> via `render_themed()`. No markdown parsing, no code blocks, no inline structure. Tool cards are single-line `✓ edit  first line of result...`.
- `footer.rs` (263 LoC): 4-card HUD (context/model/memory/system) with gauge bar. Works well for compact mode. No raised/boxed mode.
- `dashboard.rs` (138 LoC): Right panel with focused node, openspec changes, session stats. Bare minimum.
- `editor.rs` (154 LoC): Single-line text input. No multi-line, no paste.
- `selector.rs` (129 LoC): Popup overlay for /model and /think. Functional.
- `mod.rs` (771 LoC): App struct, draw(), event handling, slash commands. Monolithic — layout, event handling, command dispatch, and state management all in one file.

**What the TS dashboard has that we don't:**
1. Boxed raised mode with `╭─╮│╰─╯` chrome
2. Branch tree visualization
3. Design-tree section with node listings, status badges, spec badges
4. OpenSpec section with change cards, progress bars, stage badges
5. Cleave section with child status, wave display
6. Recovery/fallback section
7. Model topology (driver/extraction/embeddings/fallback roles)
8. Directive indicators
9. Responsive 3-tier layout (narrow/medium/wide)
10. `mergeColumns`, `leftRight`, `padRight` — column layout primitives
11. Section dividers as first-class pattern
12. Priority-aware text truncation (high-priority segments preserved)

**Conversation view gaps:**
- No markdown rendering (headers, bold, italic, code blocks, lists)
- Tool events are single-line — no expandable cards with args summary + result preview
- No visual grouping of assistant text + its tool calls as a "turn"
- No thinking block collapse/expand
- No lifecycle event cards (phase change, decomposition, child status)
- No file path highlighting or clickable references
- No error block styling (red border, error icon)

**Shared patterns needed (DRY):**
- Section divider: `── label ────────` (used in footer, dashboard, conversation)
- Card/block: bordered region with title and content
- Badge: icon + colored text for status
- Gauge bar: `▐▓▓██░░░▌ 43%` (footer, could be reused for progress)
- Column layout: `mergeColumns` equivalent for side-by-side rendering
- Responsive breakpoints: functions that choose layout based on available width
- Text truncation with ellipsis at column boundary

## Decisions

### Decision: Structural highlighting over full markdown parsing

**Status:** decided
**Rationale:** pulldown-cmark → styled spans is high effort, marginal payoff in TUI. Instead: detect code fences (surface_bg), headers (accent_bright+bold), bold/italic via regex. 80% visual value at 10% effort. Full markdown deferred.

### Decision: Lightweight turn grouping with left-border gutter, not collapsible trees

**Status:** decided
**Rationale:** Tool calls within a turn get a subtle left-border gutter (│ ✓ read...) that visually groups them under the assistant response. Turn boundary is a blank line. No expand/collapse — keeps scroll position predictable and implementation simple.

### Decision: Priority order: widgets.rs → conversation restructure → raised dashboard

**Status:** decided
**Rationale:** widgets.rs (shared primitives) unblocks everything. Conversation restructure is the most visible daily-use improvement. Raised dashboard uses widgets and is less urgent since the compact footer works.

## Open Questions

*No open questions.*

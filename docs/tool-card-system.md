---
id: tool-card-system
title: Tool Card Visual System — Omegon-owned card rendering with borders, state colors, and glyphs
status: implemented
tags: [ux, tui, tools, rendering]
open_questions: []
---

# Tool Card Visual System — Omegon-owned card rendering with borders, state colors, and glyphs

## Overview

The pi-mono Box component is a plain padding+background container with no visual identity. Every Omegon tool that renders to the TUI needs cards with state-aware borders, glyph identity, and the Alpharius color language. Rather than patching Box upstream, Omegon should own a ToolCard component that wraps Box (or replaces it) and provides the visual system for all tool rendering — built-in and extension alike.\n\nThis is the rendering equivalent of what sci-ui.ts does for call/result lines, but for the full card body.

## Research

### Architecture options

Three paths for card visual identity:\n\n**Option A: Omegon wrapper component** — Build a ToolCard in extensions/lib/ that wraps content with borders. Problem: can't wrap built-in tool rendering because ToolExecutionComponent owns the Box/Text internally. Extension renderCall/renderResult return Components that get added as children inside the existing Box. We'd need to replace the entire ToolExecutionComponent, which is 750+ lines of battle-tested rendering.\n\n**Option B: Add border support to Box in vendor** — Minimal change to Box (add borderFn/borderGlyph, ~15 lines). Then ToolExecutionComponent sets border color based on tool state. This gives ALL tools (built-in and custom) consistent visual identity through the same code path. The theme controls colors. Legitimate upstream improvement.\n\n**Option C: Hybrid** — Add border to Box (Option B), AND build an Omegon ToolCard helper in extensions/lib/ that extension renderers use for their content. The helper provides consistent padding, glyph placement, and state-aware styling. Box border handles the left-edge accent. ToolCard handles the semantic layout inside.\n\n**Recommendation: Option B** for the border (it's a Box-level concern), with Alpharius theme colors driving the state. The existing sci-ui.ts already handles the semantic layout for extension tools. Built-in tools get the border for free.

## Decisions

### Decision: Add left-border support to Box, wire state colors from ToolExecutionComponent

**Status:** decided
**Rationale:** Box is the right abstraction level for a left-border accent. It's a generic container concern, not tool-specific. ToolExecutionComponent already switches bgFn based on state (pending/success/error) — adding a parallel borderFn switch is ~5 lines. Theme colors control the border tint. All tools get consistent visual identity through the same code path. No new component needed.

### Decision: Border state colors: dim while pending, accent on success, error-red on failure

**Status:** decided
**Rationale:** Three states need visual distinction:\n- **Pending**: borderMuted/dim — the card is in-flight, no attention needed\n- **Success**: accent (teal) — resolved normally, the glow of completion\n- **Error**: error (red) — demands attention. The current wine-dark error bg (#1a0a10) is too subtle. A red left-border makes errors immediately scannable even when the bg is barely visible.\n\nThe left border is a thin-block glyph (▎ U+258E) which is narrower than a full cell — it reads as an accent line, not a wall. The Text component also needs border support since non-bash built-in tools use Text not Box.

## Open Questions

*No open questions.*

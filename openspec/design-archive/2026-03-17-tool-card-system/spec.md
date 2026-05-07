+++
id = "1bad22ff-5c06-412b-a02c-9969cc5e74d9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Tool Card Visual System — Omegon-owned card rendering with borders, state colors, and glyphs — Design Spec (extracted)

> Auto-extracted from docs/tool-card-system.md at decide-time.

## Decisions

### Add left-border support to Box, wire state colors from ToolExecutionComponent (decided)

Box is the right abstraction level for a left-border accent. It's a generic container concern, not tool-specific. ToolExecutionComponent already switches bgFn based on state (pending/success/error) — adding a parallel borderFn switch is ~5 lines. Theme colors control the border tint. All tools get consistent visual identity through the same code path. No new component needed.

### Border state colors: dim while pending, accent on success, error-red on failure (decided)

Three states need visual distinction:\n- **Pending**: borderMuted/dim — the card is in-flight, no attention needed\n- **Success**: accent (teal) — resolved normally, the glow of completion\n- **Error**: error (red) — demands attention. The current wine-dark error bg (#1a0a10) is too subtle. A red left-border makes errors immediately scannable even when the bg is barely visible.\n\nThe left border is a thin-block glyph (▎ U+258E) which is narrower than a full cell — it reads as an accent line, not a wall. The Text component also needs border support since non-bash built-in tools use Text not Box.

## Research Summary

### Architecture options

Three paths for card visual identity:\n\n**Option A: Omegon wrapper component** — Build a ToolCard in extensions/lib/ that wraps content with borders. Problem: can't wrap built-in tool rendering because ToolExecutionComponent owns the Box/Text internally. Extension renderCall/renderResult return Components that get added as children inside the existing Box. We'd need to replace the entire ToolExecutionComponent, which is 750+ lines of battle-tested rendering.\n\n**Option B: Add border support to…

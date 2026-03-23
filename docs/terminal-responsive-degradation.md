---
id: terminal-responsive-degradation
title: Terminal responsive degradation — graceful layout collapse on resize
status: implemented
parent: rust-agent-loop
tags: [tui, layout, responsive, ux, 0.15.0]
open_questions: []
jj_change_id: zwpyutkzknmqptxxutqpkzvwuotkuywu
issue_type: feature
priority: 2
---

# Terminal responsive degradation — graceful layout collapse on resize

## Overview

Handle terminal resizing dynamically. As the terminal shrinks: sidebar disappears first (already at <120 cols), then footer collapses (instruments → engine-only → gone), then conversation fills the screen with input bar. Below a minimum viable size (~40×10?), show a 'terminal too small' message instead of a broken layout. Each breakpoint should be a clean transition, not a jarring jump. The operator should never see rendering artifacts or panics from undersized areas.

## Research

### Current layout breakpoints and guards

**Current state** — layout is mostly static with ad-hoc guards:

| Component | Current behavior | Width/height check |
|-----------|-----------------|-------------------|
| Sidebar | Shows at ≥120 cols, 40 col wide | `area.width >= 120` |
| Footer | Always 9 rows (0 in focus mode) | None — always rendered |
| Footer narrow | 4-card → render_narrow fallback | `width < 60` |
| Engine panel | Ultra-narrow 1-line fallback | `area.height < 4 \|\| area.width < 20` |
| Engine inner | Early return | `inner.width < 15 \|\| inner.height < 3` |
| Instruments | Early return (blank) | `area.width < 20 \|\| area.height < 4` |
| Inference inner | Early return | `inner.width < 10 \|\| inner.height < 3` |
| Tools inner | Early return | `inner.width < 15 \|\| inner.height < 2` |
| Dashboard inner | Early return | `inner.width < 4 \|\| inner.height < 4` |
| Editor | Always 3 rows | None |
| Conversation | `Constraint::Min(3)` | Always gets remaining space |
| Tutorial overlay | Width/position clamping | Various |

**Problems:**
1. No progressive degradation — sidebar is all-or-nothing at 120
2. Footer is always 9 rows even if terminal is only 20 rows tall
3. No minimum terminal size enforcement — at 10×5 it renders garbage
4. Height is never checked at the top level
5. Individual widgets silently early-return, leaving blank areas instead of reallocating space

## Decisions

### Decision: Five-tier responsive layout: full → compact footer → no footer → minimal → too small

**Status:** decided
**Rationale:** Progressive degradation with clear breakpoints based on available height and width. Each tier removes a layer of chrome while keeping the core (conversation + input) intact.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/mod.rs` (modified) — Main draw() method — responsive tier calculation and layout constraint adjustment
- `core/crates/omegon/src/tui/footer.rs` (modified) — Add compact 4-row footer mode for Tier 3

### Constraints

- No new files — this is layout logic changes to existing draw path
- Must not break focus mode (operator toggle overrides automatic collapse)
- Tier transitions should not flicker — use hysteresis if needed
- All existing tests must pass with no snapshot changes at current test terminal sizes

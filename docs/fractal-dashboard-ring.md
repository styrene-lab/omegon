+++
id = "dae8cdfa-3716-420e-9d98-0beae88206d7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fractal dashboard ring — living AI state visualization in the dashboard header

## Overview

The fractal state surface (tui/fractal.rs, 335 lines) exists but has never been rendered anywhere. The dashboard header (top-right panel, 36 columns wide) is a stable, always-visible area. Place the fractal there as a living 'AI ring' — a constant visual heartbeat showing the agent's state.

The fractal occupies a fixed region (e.g. 36×8 cells = 36×16 half-block pixels) at the top of the dashboard panel. Below it, the existing text content (node counts, funnel, focused node, etc.) continues unchanged.

The fractal is NOT decorative — every visual property encodes real telemetry. An experienced operator reads the fractal the way a pilot reads instruments: deep zoom + amber = agent working through dense context; shallow ocean = idle; split palette = cleave running children; Julia set = persona active.

## Research

### Implementation approach

**Layout change in dashboard.rs:**

The dashboard currently renders as one vertical block of text lines inside a `Block` with a left border. The change:

```
Before:                         After:
┌─ Ω Dashboard ──┐              ┌─ Ω Dashboard ──┐
│ 213 nodes       │              │ ▀▄█▓▒░▀▄█▓▒░  │  ← fractal (8 rows)
│ ⚙1 ●5 ◐9 ✓177  │              │ ▄▀░▒▓█▄▀░▒▓█  │
│ ░░▒▒▓▓████████  │              │ ▀▄█▓▒░▀▄█▓▒░  │
│                 │              │ ▄▀░▒▓█▄▀░▒▓█  │
│ ── nodes ──     │              │ ▀▄█▓▒░▀▄█▓▒░  │
│ ○ rust-provider │              │ ▄▀░▒▓█▄▀░▒▓█  │
│ ○ memory-opt    │              │ ▀▄█▓▒░▀▄█▓▒░  │
│ ...             │              │ ▄▀░▒▓█▄▀░▒▓█  │
│                 │              │ 213 nodes       │
│ ── openspec ──  │              │ ⚙1 ●5 ◐9 ✓177  │
│ ○ kql 0/8       │              │ ░░▒▒▓▓████████  │
│                 │              │ ── nodes ──     │
│ ── session ──   │              │ ○ rust-provider │
│ 2 turns · 4 tc  │              │ ...             │
└─────────────────┘              └─────────────────┘
```

**Rendering:**
1. Split the dashboard inner area: `[Constraint::Length(8), Constraint::Min(3)]`
2. Top chunk → `FractalWidget::render(area, buf)` — already implemented
3. Bottom chunk → existing text content (unchanged)

**Tick-driven animation:**
The TUI already has a tick interval (for spinner animation). Each tick:
1. Compute `dt` from elapsed time
2. Call `fractal.update_from_status(context_pct, thinking, active, persona, cleave, dt)`
3. The fractal's `center` drifts slowly, creating a living animation

**Cost:** The fractal renderer iterates each pixel (36×16 = 576 pixels) per frame. At the default tick rate (~10fps), that's 5760 fractal iterations per second with max_iter=50-500. On any modern CPU this is sub-millisecond — negligible.

**Update transition:** During `/update`, the fractal parameters can be driven by download progress instead of agent telemetry — zoom accelerates, palette shifts to a bright transition color, then exec() fires and the new binary's splash takes over.

### Extended telemetry mapping

Beyond what's already in `update_from_status`, additional state to encode:

| State | Visual | How |
|-------|--------|-----|
| Token rate (tokens/sec output) | Animation speed | Faster drift = higher throughput |
| Error/retry state | Color flicker to red | Brief red pulse on transient errors |
| Memory fact count | Background intensity | More facts = richer background texture |
| Tool call in progress | Bright accent pulse | Tool name's first letter hash → brief Julia perturbation |
| Compaction | Zoom OUT sharply then drift back | Visual "reset" matching context shrinkage |
| Session age (turns) | Iteration depth floor | Longer sessions → more visual complexity |
| Model tier (gloriana/victory/retribution) | Base palette warmth | Gloriana=warm gold, Victory=ocean, Retribution=cool gray |

The fractal becomes a dense information display that rewards attention. A glance tells you: "deep amber, fast drift, Julia mode — the agent is thinking hard under a persona with full context."

## Decisions

### Decision: Fixed 36×8 region at the top of the dashboard panel, tick-driven animation

**Status:** decided
**Rationale:** 8 rows of half-block characters (= 16 pixel rows) in the 36-column dashboard panel gives a ~2:1 aspect ratio fractal viewport. Always visible, always animating. The existing FractalWidget::render() handles the math; we just need to split the dashboard layout, feed telemetry per tick, and let it draw. Sub-millisecond render cost. The fractal sits ABOVE the existing text content, which scrolls below it unchanged.

## Open Questions

*No open questions.*

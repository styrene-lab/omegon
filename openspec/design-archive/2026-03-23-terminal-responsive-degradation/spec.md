+++
id = "0bbab83a-97ba-4095-8c3e-9cbfc53fe0d3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Terminal responsive degradation — graceful layout collapse on resize — Design Spec (extracted)

> Auto-extracted from docs/terminal-responsive-degradation.md at decide-time.

## Decisions

### Five-tier responsive layout: full → compact footer → no footer → minimal → too small (decided)

Progressive degradation with clear breakpoints based on available height and width. Each tier removes a layer of chrome while keeping the core (conversation + input) intact.

## Research Summary

### Current layout breakpoints and guards

**Current state** — layout is mostly static with ad-hoc guards:

| Component | Current behavior | Width/height check |
|-----------|-----------------|-------------------|
| Sidebar | Shows at ≥120 cols, 40 col wide | `area.width >= 120` |
| Footer | Always 9 rows (0 in focus mode) | None — always rendered |
| Footer narrow | 4-card → render_narrow fallback | `width < 60` |
| Engine panel | Ultra-narrow 1-line fallback | `area.height < 4 \|\| area.width < 20` |
| Engine inner | Early return | `in…

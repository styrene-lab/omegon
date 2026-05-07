+++
id = "7929a4da-1a0c-40e0-bf72-91f4ad72f2f5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fractal dashboard ring — living AI state visualization in the dashboard header — Design Spec (extracted)

> Auto-extracted from docs/fractal-dashboard-ring.md at decide-time.

## Decisions

### Fixed 36×8 region at the top of the dashboard panel, tick-driven animation (decided)

8 rows of half-block characters (= 16 pixel rows) in the 36-column dashboard panel gives a ~2:1 aspect ratio fractal viewport. Always visible, always animating. The existing FractalWidget::render() handles the math; we just need to split the dashboard layout, feed telemetry per tick, and let it draw. Sub-millisecond render cost. The fractal sits ABOVE the existing text content, which scrolls below it unchanged.

## Research Summary

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
│ ── nodes ──     │          …

### Extended telemetry mapping

Beyond what's already in `update_from_status`, additional state to encode:

| State | Visual | How |
|-------|--------|-----|
| Token rate (tokens/sec output) | Animation speed | Faster drift = higher throughput |
| Error/retry state | Color flicker to red | Brief red pulse on transient errors |
| Memory fact count | Background intensity | More facts = richer background texture |
| Tool call in progress | Bright accent pulse | Tool name's first letter hash → brief Julia perturbation |
| Compacti…

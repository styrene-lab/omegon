+++
id = "36a3de46-2210-469b-b086-071bbc7540e1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fractal status surface — multi-dimensional state visualization via generative fractal rendering — Design Spec (extracted)

> Auto-extracted from docs/fractal-status-surface.md at decide-time.

## Decisions

### Render in a dedicated viewport at the bottom-right of the dashboard sidebar (decided)

The fractal lives at the base of the sidebar panel — below lifecycle status, above the footer. It's ambient and always visible when the dashboard is raised, but doesn't compete with conversation or tool output. The sidebar already has variable-height content (design tree focus, openspec changes, cleave progress) — the fractal fills whatever vertical space remains at the bottom, naturally growing when there's less lifecycle content and shrinking when there's more. Minimum viable size: ~20×8 cells (enough for recognizable fractal structure). If the sidebar is collapsed (/dash toggle), the fractal is hidden — zero rendering cost.

### 256-color fallback with half-block rendering — true color preferred, not required (decided)

True color (24-bit RGB) gives smooth gradients. 256-color gives banded but still recognizable fractal structure — the palette just quantizes to the nearest xterm-256 color. The widget detects terminal capability at render time (COLORTERM=truecolor env check) and selects the palette accordingly. No dithering needed — the fractal's own iteration banding provides visual structure even at 256 colors. Half-block characters (▀▄) work in both modes since they only need fg+bg color, not additional color depth.

### Self-contained time parameter for v1 — tachyonfx integration deferred until value is proven (decided)

The widget manages its own animation state via a `time: f64` field incremented on each render tick. Palette transitions are linear interpolation over ~500ms (self-contained lerp, not tachyonfx). This keeps the widget dependency-free and testable in isolation. If the fractal proves its worth as a status surface, tachyonfx integration for richer transitions (dissolve, wipe, glow) can be added as a follow-on. Don't overengineer before we know this is warranted.

## Research Summary

### rsfrac analysis — what's reusable vs what we build

**rsfrac** (github.com/SkwalExe/rsfrac) is a full terminal application, not a widget library. It's GPL-3.0 licensed, 18 stars, built on ratatui. It renders Mandelbrot, Burning Ship, and Julia fractals with interactive navigation (pan, zoom, iterate).

**What's relevant from rsfrac:**
- Demonstrates that terminal-resolution fractal rendering is viable in ratatui — half-block characters (▀▄) give 2x vertical resolution
- Uses `crossterm` color output (true color where available, 256-color fallback…

### Signal-to-visual mapping — how harness state drives the fractal

The fractal viewport is a function of `HarnessStatus` + `SessionStats` + time. Every visual property maps to an observable signal:

**Continuous signals (smooth animation):**

| Visual property | Harness signal | Mapping |
|---|---|---|
| Zoom depth | Context utilization % | 0% → zoom 1.0 (wide view), 100% → zoom 1e6 (deep spiral) |
| Center X drift | Session elapsed time | Slow rightward drift along real axis (~0.001/minute) |
| Center Y | Turn number (mod period) | Sinusoidal wobble, amplitude…

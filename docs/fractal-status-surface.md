---
id: fractal-status-surface
title: Fractal status surface — multi-dimensional state visualization via generative fractal rendering
status: implemented
parent: tui-visual-system
tags: [tui, ux, visualization, fractal, ratatui, generative, status, aesthetic]
open_questions: []
issue_type: feature
priority: 3
---

# Fractal status surface — multi-dimensional state visualization via generative fractal rendering

## Overview

Replace conventional loading bars and status indicators with a living fractal viewport that encodes multi-dimensional harness state into visual properties. Instead of reading \"72% context used\" as text, the operator sees a Mandelbrot region whose zoom depth, color palette, animation speed, and structural features all correspond to real system state.\n\nThe fractal is not decorative — each visual dimension maps to a harness signal:\n- **Zoom depth** → context utilization (deeper = fuller)\n- **Color palette** → cognitive mode (design = cool blues, coding = warm ambers, cleave = split complementary)\n- **Animation speed** → agent activity (fast iteration during tool calls, slow drift during thinking)\n- **Center coordinates** → session progression (drifts through the fractal space over time)\n- **Brightness/contrast** → health (high contrast = all systems nominal, washed out = degraded)\n- **Fractal type** → persona (Mandelbrot = default, Burning Ship = aggressive, Julia = creative)\n\nInspiration: rsfrac (github.com/SkwalExe/rsfrac) demonstrates fractal rendering in ratatui at terminal resolution. The approach here is different — not an explorer, but a generative status surface driven by harness telemetry.

## Research

### rsfrac analysis — what's reusable vs what we build

**rsfrac** (github.com/SkwalExe/rsfrac) is a full terminal application, not a widget library. It's GPL-3.0 licensed, 18 stars, built on ratatui. It renders Mandelbrot, Burning Ship, and Julia fractals with interactive navigation (pan, zoom, iterate).

**What's relevant from rsfrac:**
- Demonstrates that terminal-resolution fractal rendering is viable in ratatui — half-block characters (▀▄) give 2x vertical resolution
- Uses `crossterm` color output (true color where available, 256-color fallback)
- Iteration-to-color mapping via configurable palettes
- Proves the math is cheap enough for real-time rendering at terminal resolutions (typically 80-200 columns × 24-50 rows = ~10k pixels max)

**What we'd build fresh (not fork rsfrac):**
- rsfrac is GPL-3.0 — incompatible with our MIT/Apache dual license
- rsfrac is an interactive explorer — we need a headless renderer driven by external parameters
- The fractal math is trivial (~30 lines for Mandelbrot iteration) — no reason to depend on a crate for it

**Implementation approach — FractalWidget:**
```rust
struct FractalWidget {
    // Fractal parameters (driven by HarnessStatus)
    fractal_type: FractalType, // Mandelbrot, BurningShip, Julia
    center: (f64, f64),        // viewport center in complex plane
    zoom: f64,                 // zoom level
    max_iter: u32,             // iteration depth
    palette: ColorPalette,     // maps iteration count → Color
    time: f64,                 // animation time (for smooth transitions)
}

impl Widget for FractalWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // For each cell in the render area:
        // 1. Map (col, row) → complex coordinate based on center + zoom
        // 2. Iterate the fractal function up to max_iter
        // 3. Map iteration count → color via palette
        // 4. Use half-block characters for 2x vertical resolution
    }
}
```

**Performance budget:**
- At 100×50 terminal cells with half-blocks = 100×100 = 10,000 pixel computations
- Mandelbrot at 100 iterations = 1M multiplications worst case
- Modern CPU: ~1ms for the full frame
- Re-render on every TUI tick (16ms) is trivially achievable
- Julia sets are even cheaper (no inner loop escape check variance)

**Color palette mapping — the key design decision:**
The palette is how the fractal becomes a status display. Not arbitrary pretty colors — each palette maps to a cognitive mode:

| Mode | Palette | Visual character |
|---|---|---|
| Idle / waiting | Alpharius ocean (deep blue → teal → white) | Calm, deep water |
| Coding / execution | Amber → gold → white | Forge heat, productive warmth |
| Design / exploration | Violet → cyan → white | Ethereal, open-ended |
| Cleave / parallel | Split complementary (two hue families) | Deliberate tension/duality |
| Error / degraded | Desaturated, low contrast | Washed out, unhealthy |
| Compaction | Brief inversion/negative | Visual "reset" moment |

Transitions between palettes should be smooth (interpolate over ~500ms) via tachyonfx or manual lerp.

### Signal-to-visual mapping — how harness state drives the fractal

The fractal viewport is a function of `HarnessStatus` + `SessionStats` + time. Every visual property maps to an observable signal:

**Continuous signals (smooth animation):**

| Visual property | Harness signal | Mapping |
|---|---|---|
| Zoom depth | Context utilization % | 0% → zoom 1.0 (wide view), 100% → zoom 1e6 (deep spiral) |
| Center X drift | Session elapsed time | Slow rightward drift along real axis (~0.001/minute) |
| Center Y | Turn number (mod period) | Sinusoidal wobble, amplitude decreasing as session ages |
| Animation speed | Tool calls/minute | 0 calls = frozen, high activity = smooth glide |
| Brightness | Provider health | All authenticated = full brightness, degraded = dim |
| Iteration depth | Thinking level | off=50, low=100, medium=200, high=500 |

**Discrete signals (palette/type switches):**

| Visual property | Harness signal | Mapping |
|---|---|---|
| Color palette | Cognitive mode (idle/coding/design/cleave) | Palette swap with 500ms crossfade |
| Fractal type | Active persona | Default=Mandelbrot, persona badge drives Julia c-parameter |
| Inversion flash | Compaction event | 200ms palette inversion on BusEvent::Compacted |
| Border color | MCP server count | 0=dim, 1-3=accent, 4+=bright |

**Julia set personalization:**
Each persona maps to a unique Julia set c-parameter, derived from hashing the persona ID:
```rust
fn persona_to_julia_c(persona_id: &str) -> (f64, f64) {
    let hash = hash64(persona_id);
    let real = (hash & 0xFFFF) as f64 / 65536.0 * 1.5 - 0.75; // [-0.75, 0.75]
    let imag = (hash >> 16 & 0xFFFF) as f64 / 65536.0 * 1.5 - 0.75;
    (real, imag)
}
```
This means each persona has a visually distinct fractal signature — the operator can glance at the status surface and know which persona is active by the fractal's shape, before reading any text.

**Where it renders:**
- **Dashboard panel** — replaces the blank space below lifecycle status with a small fractal viewport (~30×15 cells)
- **Splash screen** — the fractal zooms in during startup, settling at the initial context state
- **Background of thinking indicator** — during extended thinking, the fractal slowly evolves behind the spinner text
- **NOT in the conversation area** — the fractal is ambient, not intrusive

## Decisions

### Decision: Render in a dedicated viewport at the bottom-right of the dashboard sidebar

**Status:** decided
**Rationale:** The fractal lives at the base of the sidebar panel — below lifecycle status, above the footer. It's ambient and always visible when the dashboard is raised, but doesn't compete with conversation or tool output. The sidebar already has variable-height content (design tree focus, openspec changes, cleave progress) — the fractal fills whatever vertical space remains at the bottom, naturally growing when there's less lifecycle content and shrinking when there's more. Minimum viable size: ~20×8 cells (enough for recognizable fractal structure). If the sidebar is collapsed (/dash toggle), the fractal is hidden — zero rendering cost.

### Decision: 256-color fallback with half-block rendering — true color preferred, not required

**Status:** decided
**Rationale:** True color (24-bit RGB) gives smooth gradients. 256-color gives banded but still recognizable fractal structure — the palette just quantizes to the nearest xterm-256 color. The widget detects terminal capability at render time (COLORTERM=truecolor env check) and selects the palette accordingly. No dithering needed — the fractal's own iteration banding provides visual structure even at 256 colors. Half-block characters (▀▄) work in both modes since they only need fg+bg color, not additional color depth.

### Decision: Self-contained time parameter for v1 — tachyonfx integration deferred until value is proven

**Status:** decided
**Rationale:** The widget manages its own animation state via a `time: f64` field incremented on each render tick. Palette transitions are linear interpolation over ~500ms (self-contained lerp, not tachyonfx). This keeps the widget dependency-free and testable in isolation. If the fractal proves its worth as a status surface, tachyonfx integration for richer transitions (dissolve, wipe, glow) can be added as a follow-on. Don't overengineer before we know this is warranted.

## Open Questions

*No open questions.*

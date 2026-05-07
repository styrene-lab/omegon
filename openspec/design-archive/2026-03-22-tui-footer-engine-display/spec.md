+++
id = "14461faf-7b82-4556-ad8f-8510fd98aa57"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Footer redesign — engine display + linked minds — Design Spec (extracted)

> Auto-extracted from docs/tui-footer-engine-display.md at decide-time.

## Decisions

### Footer grows to 10-12 rows, conversation absorbs the loss (exploring)

Conversation is scroll history — compressible. The instrument panel is the operator's persistent situational awareness surface. Allocating 20-24% of vertical space to instruments (vs current 10%) follows the CIC pattern where instruments dominate and the viewport is one element among many. Compact fallback at terminal heights under 35 rows.

### Four simultaneous fractal instruments, not one switching display (exploring)

A submarine CIC runs sonar, radar, thermal, and signal analysis simultaneously — different instruments showing different dimensions of the same environment. Each of our four algorithms maps to a distinct telemetry source: Perlin=context health, Lissajous=tool activity, Plasma=thinking state, Clifford=memory activity. Running all four simultaneously gives the operator peripheral awareness of all system dimensions at once. The pattern of which instruments are active/calm IS the situational awareness.

### Unified color language: idle navy → stormy blue → amber at maximum (decided)

All four instruments share the same color ramp: idle navy (near-black teal), increasing activity shifts toward brighter blue, maximum intensity shifts hue toward amber. This keeps every instrument visually consistent with one another and with the theme's existing color meanings (teal=normal, amber=warning). The operator reads intensity across all four instruments as a unified signal — no need to learn per-instrument color vocabularies. Shape (algorithm) differentiates the instruments, color (intensity) differentiates the state.

### Color ramp: dark navy (idle) → teal (normal) → amber (maximum) (decided)

Teal is the Alpharius brand color — it belongs at the center of the ramp as the steady-state "everything is nominal" reading. Dark navy below it for idle/resting. Amber above it for high load/attention needed. The operator's eye calibrates to teal as normal; darker means quieter, warmer means hotter. This matches the existing theme semantics where teal=accent (normal) and amber=warning.

### Split-panel layout: inference (left 40%) / system state (right 60%) (decided)

Left half = what is inferencing and what is being inferenced about (engine config + linked minds). Right half = what is the state of the system driving the inference (four fractal instruments + operational stats). Maps to CIC station separation. High-frequency glance target (inference) on the reading side for LTR.

### Footer grows to 10-12 rows with focus mode toggle (decided)

Conversation is compressible scroll history. Instruments are the persistent situational awareness surface. Default is instrument-heavy (10-12 rows). Focus mode (hotkey or /focus) hides the instrument panel entirely for full-height conversation — useful for reading long outputs, viewing rendered images, or working in alternate tabs. Compact fallback at terminal heights under 35 rows.

### Four simultaneous fractal instruments in 2×2 grid (decided)

CIC pattern — multiple instruments running simultaneously, each showing a different dimension of system state. Perlin=context health, Plasma=thinking/inference, Lissajous=tool activity, Clifford=memory activity. The pattern across all four IS the situational awareness. Shape differentiates instruments, unified color ramp differentiates intensity.

### All instruments share unified navy→teal→amber ramp, differentiate by shape only (decided)

Answered: teal is the center (normal), navy is idle, amber is hot. Shape (algorithm) differentiates instruments. No per-instrument hue.

### Dashboard header collapses — fractal moves to system panel, sidebar gains design tree space (decided)

The fractal's home is the system panel in the footer. The dashboard sidebar header (previously 36×8 fractal) becomes available for the design tree to use as additional vertical space.

### CA waterfall replaces Clifford attractor for memory instrument, with per-mind columns (decided)

Clifford attractor was unreadable at 22×5. 1D CA waterfall with CRT noise glyphs is visually distinct (digital/crisp vs smooth), has history (scrolling), and maps to memory semantics (patterns from discrete operations). Per-mind independent columns show which minds are linked (column count) and which are active (brightness). Each mind runs its own CA rule based on operation type.

## Research Summary

### Current footer anatomy and waste analysis

**Current layout**: 4 bordered cards in a horizontal row, each Ratio(1,4). Total height: 5 rows (1 border top + 2 content + 1 padding + 1 border bottom).

**Space budget at 160 cols wide**: 4 × 40 cols = 160. Each card loses 4 cols to borders + 2 to padding = 34 usable per card. Total usable: ~136 chars across 2 content lines = ~272 characters of content capacity.

**Actual content at startup**:
- context: `█░░░░░░░ 0% / 1.0M` (~20 chars) — wastes 48 chars
- model: `☁ claude-opus-4-6 · Legion` +…

### Proposed layout — fighting game status bar

Drop the bordered cards entirely. Use a 3-row dense status area with no borders — just color-coded segments separated by dim `│` dividers. Every row is a continuous line of text.

```
 ▸ opus-4-6 · victory · ◎ low │ ████░░░░ 12% / 1.0M · T·3 · ⚙ 7  │ main ±3 · ~/workspace/ai/omegon
 ● anthropic · Legion · native │ ⌗ 2565 · inj 25 · wm 4 · ~3.7k   │ MCP 2(14t) · 🔓 3 · ↻ 0
                                │ ⬡ project · ⬡ working · ⬡ episodes│
```

**Row 1 — Engine + Context + System**:
- Left: mod…

### Submarine CIC / ops room design principles

**Ecological Interface Design (EID)** — the key framework from submarine/nuclear control room research. Core principle: "make visible the invisible." Three levels:

1. **Skill-based** — direct perception, no thinking required. Gauges, colors, spatial position. A submariner glances at the depth gauge — they don't compute depth from pressure readings. The gauge IS the understanding.

2. **Rule-based** — familiar patterns trigger learned responses. "When this gauge reaches this zone, do this." Colo…

### Split-panel CIC layout — inference vs system state

**Operator's framing**: The footer is not 3-4 equal cards. It's two conceptual halves:

**Left half — "What is inferencing, what is being inferenced about"**
- Engine: model, tier, thinking, context mode, auth
- Memory/Minds: what knowledge is loaded, how much is injected, token budget
- Context gauge: how much runway remains
- This is the SONAR OPERATOR's station — "what are we tracking, what do we know"

**Right half — "What is the current state of the system driving the inference"**  
- Git t…

### Vertical space reallocation — conversation is compressible

**The conversation area is scroll history.** Once you've read a response, it scrolls up. Every row dedicated to conversation is a row of already-processed text. The INSTRUMENTS (footer, sidebar) are what the operator is actively monitoring while the AI works.

**Current allocation (50-row terminal)**:
- Conversation: 41 rows (82%)
- Editor: 3 rows
- Hint line: 1 row
- Footer: 5 rows (10%)

**Proposed reallocation**:
- Conversation: 33-35 rows (66-70%) — still shows 15+ lines of current response
…

### Focus mode, conversation tabs, and fractal state mapping

**Focus mode — toggle between instruments and content:**

The operator can toggle between two modes:
- **Normal**: 10-12 row instrument panel visible, conversation gets remaining space
- **Focus**: instrument panel disappears entirely, conversation gets full height. Toggle via hotkey or `/focus`. Useful for reading long responses, viewing rendered images/diagrams, or working in alternate tabs.

This eliminates the height budget concern entirely. The default is instrument-heavy. When you need the…

### Multi-instrument display — four simultaneous fractals

**CIC analogy**: A submarine CIC has sonar waterfall, bearing plot, frequency analysis, AND tactical overlay — all running simultaneously, each showing a different dimension of the same acoustic environment. We should do the same.

**Four instruments in a 2×2 grid in the system panel:**

| Position | Name | Algorithm | Telemetry source | Visual signature |
|---|---|---|---|---|
| Top-left | **Sonar** | Perlin flow | Context utilization % | Speed/turbulence increases with context fill. Calm=low, …

### Cross-instrument visual features — linked minds and injection band

**Linked minds as waterfall columns:**

The waterfall's 22-column width divides into segments per active mind. The structural change (column count) IS the mind count — no number reading needed.

- 1 mind (project only): full 22-column waterfall
- 2 minds (project + working): two 10-column waterfalls with 2-col gap
- 3 minds (+ episodes): three 6-column waterfalls with 2-col gaps
- 4 minds (+ archive): four 4-column waterfalls with 2-col gaps

When a mind activates, a new column segment appears. …

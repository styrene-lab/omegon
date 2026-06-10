+++
id = "d99120a7-3be1-4ceb-9bd3-a1720f6f5a61"
kind = "document"
title = "Engine panel redesign — visual hierarchy and information density"
status = "implemented"
tags = []
aliases = ["engine-panel-redesign"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "footer-idle-state"
+++

# Engine panel redesign — visual hierarchy and information density

## Overview

The engine panel currently dumps 6 lines of text info into a bordered box. The values are correct but the presentation doesn't communicate importance, relationships, or state. Need to think about what each value IS, how to visually encode it, and how to organize them within the 40%-width, 12-row box.

## Research

### Current values inventory and operator meaning

The engine panel currently shows 6 lines of text. Here's what each value IS and what it MEANS to the operator:

### Layout proposal — grouped sections with visual hierarchy



## Decisions

### Decision: Grouped layout with visual hierarchy — model headline, context gauge, tier+thinking, counters at bottom

**Status:** decided
**Rationale:** The current 6-line text dump has no visual hierarchy — every value has the same weight. The redesign groups by operator importance: model identity (brightest, top), context gauge (visual bar, middle), tuning parameters (tier+thinking), session counters (dimmest, bottom). Context gets a ▰▱ gauge bar because it's the most dynamically changing value and deserves spatial treatment. Provider+auth merge onto one line. Context class right-aligns with model name.

### Decision: Remove context gauge from engine — inference panel owns fill visualization. Engine shows capacity only.

**Status:** decided
**Rationale:** Context fill is displayed in two places: the inference gradient bar and the engine ▰▱ gauge. The inference bar is the primary visualization (spatial, color-coded, thinking glitch overlay). The engine gauge is a redundant text duplicate. Engine should show capacity (200k window, native/adaptive mode) on one compact line, not fill. This frees 2 rows in the engine panel for better spacing or additional info.

## Open Questions

*No open questions.*

## Proposed layout (10 inner rows, ~60 cols)

```
┌─ engine ──────────────────────────┐
│                                    │
│  claude-opus-4-6           Massive  │  ← MODEL: bright, bold. Class: right-aligned
│  ☁ Anthropic · ● subscription      │  ← provider + auth: muted supporting info
│                                    │
│  ▰▰▰▰▰▰▰▰▰▰▰▰▰▱▱▱▱▱▱▱  12%     │  ← CONTEXT BAR: visual gauge, not just text
│  200k window · adaptive            │  ← context details: dim
│                                    │
│  Victory · ◎ Medium                │  ← tier + thinking: prominent
│  T·3 · ⚙ 12 · ↻ 0                │  ← session counters: dimmest
│                                    │
└────────────────────────────────────┘
```

## Key changes from current

1. **Model name is the headline** — largest visual weight, top of panel. Context class right-aligned on same line (saves a row, shows they're related).

2. **Context gets a visual gauge** — a mini bar chart (▰▱ or block chars) showing fill percentage. The number is beside it, not a standalone line. This is the most dynamically changing value — it deserves spatial treatment, not just "12%".

3. **Grouped by semantic tier** — identity at top, context in middle, tuning below, counters at bottom. Empty rows separate groups.

4. **Provider + auth merged** — "☁ Anthropic · ● subscription" on one line. These are both "where/how" supporting info.

5. **Tier + thinking on one line** — they're both tuning parameters.

6. **Counters remain at bottom** — lowest importance, dimmest color.

7. **Persona replaces auth line when active** — if a persona is loaded, show "🗡 Alpharius" instead of auth info (auth can be inferred).

## Context gauge characters

Options:
- Block: ▰▰▰▱▱▱ (clean, geometric)
- Bar: ████░░░ (heavy, high contrast)
- Braille: ⣿⣿⣿⠀⠀⠀ (fine-grained but hard to read at small sizes)
- Thin: ─────── with color (minimal)

Block (▰▱) is best — distinct filled/empty glyphs, not too heavy, works at any terminal size.

## Values by importance to the operator

**Tier 1 — What am I talking to? (Identity)**
- Model name (claude-opus-4-6) — which brain is behind this session
- Provider source (☁ cloud / ⚡ local) — where the compute is running
- Context class (Massive/Extended/Compact/Scout/Battalion) — how much context the model can see

These answer: "What is the engine?"

**Tier 2 — How is it configured? (Tuning)**
- Tier (Victory/Gloriana/Retribution) — capability level
- Thinking level (Off/Minimal/Low/Medium/High) — reasoning depth
- Context mode (native/adaptive) — how context is managed

These answer: "How is it tuned?"

**Tier 3 — What's the session state? (Status)**
- Context % (0-100) — how full is the context window
- Context window size (200k tokens) — absolute capacity
- Auth type (subscription/api key) — billing relationship
- Active persona — if a persona is loaded

These answer: "What's the current state?"

**Tier 4 — Session counters (Telemetry)**
- Turn count (T·0)
- Tool calls (⚙ 0)
- Compactions (↻ 0)

These answer: "How much work has been done?"

## The problem

All 6 lines have the same visual weight. Model name, auth type, and compaction count all look equally important. The operator's eye has no hierarchy to follow. The most important info (what model, what tier) doesn't pop. The least important info (compaction count) takes the same space.

## Available space

The engine panel is ~40% of the footer width. On a 160-col terminal, that's ~64 cols. With border, ~62 inner cols. Height is 12 rows, inner ~10 rows. That's 620 cells — plenty of space for ~15 values.

## Design direction

Group by tier. Use visual weight (brightness, boldness, size) to encode importance. Use spatial position (top = most important) to reinforce hierarchy. Use a compact layout — key:value pairs can be denser than one-per-line.

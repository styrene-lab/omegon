+++
id = "690b3842-32ab-44b3-a96e-374924f588a7"
kind = "document"
title = "TUI HUD redesign — game-inspired operator interface"
status = "implemented"
tags = ["tui", "ux", "design"]
aliases = ["tui-hud-redesign"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = ["What video game HUDs are good reference points? (EVE Online overview, Factorio, Rimworld colony stats, Dwarf Fortress, Diablo resource orbs?)", "How should focus switching work? Dedicated hotkeys per panel, or a tab-cycle through focusable regions?", "What is the tri-axis display? Provider (Anthropic/OpenAI/Local) × Tier (retribution/victory/gloriana) × Thinking (off→high) — how to visualize three dimensions compactly?", "Should the footer be a single dense status line (fighting game style) or keep the multi-card layout but merge/reorganize the cards?"]
parent = "rust-tui-bridge"
related = []
+++

# TUI HUD redesign — game-inspired operator interface

## Overview

Fundamental rethink of the Omegon TUI layout. The current footer/sidebar are TS harness holdovers — flat lists and separate boxes for related data. The redesign takes video game HUD inspiration: information density, spatial meaning, interactive focus, and visual hierarchy that communicates state at a glance.

Three major areas:

1. **Footer → Status Bar**: Merge context+model into a unified "engine" display reflecting the tri-axis (provider/tier/thinking). Memory becomes "linked minds" — which memory systems are active, their sizes, injection state. Fact count is a detail, not the headline.

2. **System card → Git tree**: The system card is underutilized. Git branch topology is critical operational context — show it as an actual interactive tree widget with color-coded branches (cleave=cyan, feature=green, fix=amber, etc). Scrollable via mouse, focusable via hotkey.

3. **Dashboard sidebar → Design tree**: Currently a flat node list. The design tree IS a tree — render it as one with expand/collapse, indentation, parent-child relationships visible. Same interaction model as the git tree.

## Research

### Game HUD patterns that map to terminal constraints

**Key insight**: game HUDs solve the same problem we have — maximum information in minimum space while the player's attention is on the main action (conversation). The best HUDs are read at a glance, not studied.

**Patterns worth stealing:**

1. **Dwarf Fortress / Rimworld — sidebar status panels**: Dense, scrollable, color-coded status lists. Each item is one line with an icon + short text. Status changes cause the line to flash or change color. The sidebar is the "world state at a glance." Maps directly to our design tree + git tree.

2. **EVE Online — overview + resource bars**: The overview is a sortable, filterable table of everything in range. Resource bars (shield/armor/hull) are stacked horizontal gauges. Maps to: context gauge stacked with memory gauge, model/tier as a "loadout" display.

3. **Factorio — production statistics**: Tiny inline sparklines showing rate of change over time. A fact count that's going UP is different from one that's STABLE. Maps to: memory injection rate, context fill rate, tool call frequency.

4. **Diablo — resource orbs**: Two large visual indicators (health/mana) that are always visible and instantly readable. The fractal surface IS our version of this — the ambient state indicator you read without thinking.

5. **Fighting games — frame data**: Tiny, dense, numeric readouts in a bar. "60fps | 12ms | P1: 100% | P2: 87%". Maps to our footer: compress everything into a single dense status line rather than 4 bordered cards with padding.

**Anti-patterns to avoid:**
- Bordered cards with lots of padding (wastes 40%+ of space on chrome)
- Hiding data at zero (the shape of the display should be stable — values change, layout doesn't)
- Separate boxes for related data (context and model are one thing — "what engine am I running")

**gitui reference**: Already in the ratatui showcase. Shows git branches as a real tree with colors. Worth studying their branch rendering code.

## Open Questions

- What video game HUDs are good reference points? (EVE Online overview, Factorio, Rimworld colony stats, Dwarf Fortress, Diablo resource orbs?)
- How should focus switching work? Dedicated hotkeys per panel, or a tab-cycle through focusable regions?
- What is the tri-axis display? Provider (Anthropic/OpenAI/Local) × Tier (retribution/victory/gloriana) × Thinking (off→high) — how to visualize three dimensions compactly?
- Should the footer be a single dense status line (fighting game style) or keep the multi-card layout but merge/reorganize the cards?

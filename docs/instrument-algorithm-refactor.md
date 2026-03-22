---
id: instrument-algorithm-refactor
title: Instrument algorithm refactor — thinking as glitch, memory as sine strings
status: decided
parent: tui-hud-redesign
tags: [tui, instruments, visual]
open_questions: []
jj_change_id: lozslwvmnptpwntvzyoollnyvpqwpkmm
---

# Instrument algorithm refactor — thinking as glitch, memory as sine strings

## Overview

The four instrument algorithms need better visual differentiation and semantic mapping.

## Research

### Unified instrument layout — context/thinking merged, tools as sort visualization, memory as linked sine strings



## Decisions

### Decision: Thinking glitch overlays the context bar as a static field, not a waterfall

**Status:** decided
**Rationale:** Thinking IS what's happening inside context. The glitch chars appear on the context bar surface — denser when thinking harder. No scrolling needed because the context bar itself is the spatial reference. The glitch disrupts the clean bar surface, making thinking visible as a perturbation of the context state.

### Decision: Memory sine waves are plucked then dampen — direction encodes read vs write

**Status:** decided
**Rationale:** Each memory operation plucks the string. The wave propagates rightward (→) for storage, leftward (←) for retrieval. Amplitude and speed indicate intensity. The wave dampens naturally — a quiet string means no recent activity. Multiple rapid operations create interference patterns as waves overlap. This maps memory ops to a physical metaphor the operator develops intuition for.

### Decision: Thinking overlays context directly — glitch chars on the bar surface

**Status:** decided
**Rationale:** Option (b) wins — thinking bleeds into context. The context bar is a clean gradient when idle. When thinking, glitch chars appear across its surface, denser at higher thinking levels. Context and thinking share the same pixels. A tree connector links context down to memory strings, showing the data flow path.

### Decision: Tools as bubble-sort list with recency bars, not Lissajous curves

**Status:** decided
**Rationale:** Lissajous curves don't communicate WHICH tools are active. A sorted list with tool names, recency bars, and time-since-last-call is self-documenting. The list physically reorders when tools fire (bubble-to-top animation). The operator sees 'bash was called 3s ago' not 'there are teal curves.' Tool error = red bar that stays at top.

### Decision: Two-panel layout: inference state (left) + tool activity (right)

**Status:** decided
**Rationale:** Replace the 2×2 grid with LEFT (unified inference: context bar + thinking glitch overlay + tree connector + memory sine strings) and RIGHT (tool bubble-sort list). Left tells the inference story top-to-bottom. Right shows execution activity. The 2×2 grid's four independent panels become two interconnected panels.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/instruments.rs` (modified) — Complete rewrite: replace 2x2 grid with two-panel layout. Port inference panel (context bar + thinking glitch + tree connector + sine strings) and tool panel (bubble-sort list) from instrument_lab.rs example.
- `core/crates/omegon/src/tui/mod.rs` (modified) — Update instrument panel wiring: pass tool names on ToolStart/ToolEnd, pass memory op direction (store vs recall), update layout split.

### Constraints

- Port directly from instrument_lab.rs — do NOT rewrite from scratch
- Wave physics must use the same 1D wave equation from the lab
- Tool list must show actual tool names from the session, not hardcoded
- Memory direction: store=rightward, recall=leftward wave propagation
- Thinking glitch density scales with thinking level AND agent_active
- Context bar caps at 70% (auto-compaction threshold)
- Tree connector uses │├└ characters linking context to memory minds

## The new model: two panels, not four

The 2×2 grid is replaced by a LEFT (inference state) and RIGHT (tool activity) split. The left panel is a single unified visualization showing context, thinking, and memory as interconnected layers — not separate boxes.

### LEFT PANEL: Inference State (context + thinking + memory linked)

A vertical visualization with three connected layers:

```
┌─ inference ──────────────────────────────────┐
│ ████████████████████░░░░░░░░░░░░░░ 34% / 200k│  ← context bar (gradient fill)
│ ░▒▓╬█▏▎▍▌ ░▒▓ ▎▍▌▐▊ ▓╱╲┼╪╫  ░▒▓█▏           │  ← thinking glitch ON TOP of bar
│┬───────────────────────────────────────────── │
│├─ ∿∿∿∿∿∿∿∿∿ project ∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿ │  ← mind sine wave
│├─ ∿∿∿∿∿∿∿∿∿ working ∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿ │  ← mind sine wave  
│└─ ∿∿∿∿∿∿∿∿∿ episodes ∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿∿ │  ← mind sine wave
└──────────────────────────────────────────────┘
```

**Context bar** — a horizontal gradient fill, like a progress bar but rendered as a smooth color ramp (navy → teal → amber as it fills). NOT Perlin noise — a clean bar that the operator reads at a glance. The bar IS the context percentage.

**Thinking glitch** — CRT noise glyphs overlaid ON TOP of the context bar. When the agent is idle, the bar is clean. When thinking, glitch chars appear across the bar surface, denser and faster at higher thinking levels. The thinking is literally "disrupting" the clean context surface — you can see it happening inside the context.

**Tree connector** — a vertical line drops from the left edge of the context bar down alongside the memory sine waves, with branches (├─, └─) connecting to each mind. Like `tree` command output. This visually links context to memory — the facts flow up from the minds into the context.

**Memory sine waves** — each mind is a horizontal line that oscillates like a plucked guitar string. Amplitude = activity level. Speed = operation frequency. Direction: rightward wave propagation (→) for storage, leftward (←) for retrieval. When idle, the line is flat (a quiet string). When a memory_store fires, the string gets plucked and the wave travels right. When memory_recall fires, the wave travels left. Multiple minds = multiple strings stacked vertically.

### RIGHT PANEL: Tool Activity (sort visualization)

A column of tool names, sorted by recency of use. When a tool fires, it moves to the top — like a bubble sort animation. The list is always showing the full active tool set, but dimmed. Recently used tools are bright, older ones fade.

```
┌─ tools ──────────────────────┐
│ ▸ bash            ████  3s   │  ← just called, bright, at top
│ ▸ write           ███   8s   │  ← called recently
│ ▸ read            ██   12s   │  ← called earlier
│   edit            ░    45s   │  ← older, dimming
│   memory_store    ░    60s   │  ← older
│   design_tree     ·   120s   │  ← dim
│   ...                        │
│   (31 active / 49 total)     │
└──────────────────────────────┘
```

Each tool has:
- Name (left-aligned)
- Activity bar (proportional to recency — full bar = just called, shrinks with time)
- Time since last call (right-aligned, dim)
- Color follows the intensity ramp (recent = teal/amber, old = navy)

When a tool is called, it "bubbles" to the top with an animation — the list physically reorders. This creates the "perpetually shuffling heap" effect. The operator sees which tools are hot and which are cold.

The sort animation doesn't need to be a real sort algorithm — it's just "move the most recently called tool to position 1, shift everything else down." The visual effect is the same as watching a bubble sort.

**Tool error**: the tool's bar turns red and it stays at the top for the error TTL.

### Why this works

1. **Context + Thinking are unified** — thinking IS what's happening inside the context. The glitch overlay makes this literal.

2. **Memory is connected to context** — the tree connector shows facts flowing from minds into context. The sine wave direction shows which way data is moving.

3. **Tools are self-documenting** — you don't need to decode Lissajous curves. You see "bash was called 3 seconds ago, write was called 8 seconds ago." The tool names ARE the visualization.

4. **The layout tells a story** — left column reads top-to-bottom as "context (how full) → thinking (what's happening) → memory (what we know)." Right column is "what actions are being taken." Inference state vs execution state.

## Current (problems)

- Context (Perlin) and Thinking (Plasma) are both 'field fill' algorithms — visually too similar
- Memory (CA waterfall) is visually isolated — no connection to the other instruments
- Thinking doesn't interleave with context and memory despite being the bridge between them

## Proposed

1. **Context** — Perlin flow (KEEP). Ambient field fill works perfectly for 'always present' context state.

2. **Tools** — Lissajous curves (KEEP). Sparse-to-dense curves on activity. Distinct visual character.

3. **Thinking** — CRT glitch characters (REPLACE plasma). Same glyph set as the splash screen. Speed and density increase with thinking level. Digital/crisp aesthetic vs the smooth Perlin — visually distinct. The glitch chars create a 'processing' feel that plasma's smooth waves don't.

4. **Memory** — Oscillating sine wave strings (REPLACE CA waterfall). Each line is a mind (1 line default for project, expanding as more minds link). Amplitude and speed indicate activity level. Direction of oscillation encodes operation: rightward (→) for storage, leftward (←) for retrieval. Horizontal default for single mind. The wave 'pluck' on each memory operation creates a guitar-string visual.

## Thinking as bridge

Thinking interleaves between context and memory — it's the process that connects 'what we know' (memory) with 'what we're computing about' (context). Visually, the thinking glitch could share screen space or visual elements with both neighbors, creating a sense of flow between them.

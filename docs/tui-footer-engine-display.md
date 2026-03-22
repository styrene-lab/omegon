---
id: tui-footer-engine-display
title: Footer redesign — engine display + linked minds
status: exploring
parent: tui-hud-redesign
open_questions:
  - With the fractal moving to the system panel, what replaces it in the dashboard header? Or does the dashboard header collapse and the sidebar gains that space for the design tree?
  - Should each instrument have its own hue (sonar=teal, radar=green, thermal=amber, signal=blue) or should they all share the Alpharius teal band and differentiate only through shape?
jj_change_id: zroxvpvwoqkmpnxsqluvxxpktmnplror
---

# Footer redesign — engine display + linked minds

## Overview

Merge the current 4-card footer into a denser, more meaningful layout:

**Engine panel** (replaces context + model cards): Unified display of the tri-axis — provider/tier/thinking. Shows the "engine configuration" as a single coherent unit. Context gauge stays but is part of this panel. Model name, tier badge, thinking level indicator all in one visual group.

**Minds panel** (replaces memory card): "Linked minds" concept — which memory systems are active (project memory, working memory, episodes, archive). Each mind shows: name, fact count, injection status, estimated token weight. The headline is the active minds, not a raw fact count.

**System panel** (remains but leaner): cwd, git branch (just current, not the full tree — that goes to sidebar), session uptime, MCP status. Tool call and compaction counters move here.

## Research

### Current footer anatomy and waste analysis

**Current layout**: 4 bordered cards in a horizontal row, each Ratio(1,4). Total height: 5 rows (1 border top + 2 content + 1 padding + 1 border bottom).

**Space budget at 160 cols wide**: 4 × 40 cols = 160. Each card loses 4 cols to borders + 2 to padding = 34 usable per card. Total usable: ~136 chars across 2 content lines = ~272 characters of content capacity.

**Actual content at startup**:
- context: `█░░░░░░░ 0% / 1.0M` (~20 chars) — wastes 48 chars
- model: `☁ claude-opus-4-6 · Legion` + `● subscription` (~45 chars) — wastes 23 chars  
- memory: `⌗ 2565` (~7 chars) — wastes 61 chars
- system: `⌂ ~/workspace/ai/omegon` (~25 chars) — wastes 43 chars

**Total waste: 175 of 272 usable chars = 64% empty space.**

**During active session** the cards fill more (injected facts, tool calls, MCP, compactions appear), but the layout was designed for peak, not steady state.

**Data available but not shown anywhere persistent**:
- Tier (retribution/victory/gloriana) — only in /model output
- Thinking level — only in hint line
- Context mode — only in hint line
- Session duration
- Git branch + dirty state
- Active/total tool count
- Estimated tokens consumed this session
- Provider name (anthropic/openai/local)

### Proposed layout — fighting game status bar

Drop the bordered cards entirely. Use a 3-row dense status area with no borders — just color-coded segments separated by dim `│` dividers. Every row is a continuous line of text.

```
 ▸ opus-4-6 · victory · ◎ low │ ████░░░░ 12% / 1.0M · T·3 · ⚙ 7  │ main ±3 · ~/workspace/ai/omegon
 ● anthropic · Legion · native │ ⌗ 2565 · inj 25 · wm 4 · ~3.7k   │ MCP 2(14t) · 🔓 3 · ↻ 0
                                │ ⬡ project · ⬡ working · ⬡ episodes│
```

**Row 1 — Engine + Context + System**:
- Left: model + tier + thinking (the "engine config")
- Center: context gauge + percent + turns + tool calls
- Right: git branch + dirty + cwd

**Row 2 — Auth + Memory + Infrastructure**:
- Left: auth method + context class + context mode
- Center: fact count + injected + working memory + token estimate
- Right: MCP + secrets + compactions

**Row 3 — Minds (optional, only when interesting)**:
- Center: which memory systems are active and their state

This uses ~3 rows instead of 5, eliminates all border/padding waste, and shows ALL data at ALL times (zeros included). The fighting game aesthetic: dense, numeric, color-coded, always the same shape.

Color coding replaces borders for visual grouping:
- Engine specs: accent color
- Context gauge: green→yellow→red gradient
- Memory stats: muted
- Git: branch name in green/amber
- Separators: dim `│`

### Submarine CIC / ops room design principles

**Ecological Interface Design (EID)** — the key framework from submarine/nuclear control room research. Core principle: "make visible the invisible." Three levels:

1. **Skill-based** — direct perception, no thinking required. Gauges, colors, spatial position. A submariner glances at the depth gauge — they don't compute depth from pressure readings. The gauge IS the understanding.

2. **Rule-based** — familiar patterns trigger learned responses. "When this gauge reaches this zone, do this." Color zones on gauges (green/yellow/red). Alarm states. The operator recognizes the pattern and applies a known rule.

3. **Knowledge-based** — novel situations requiring reasoning. This is where text labels, numbers, and raw data matter. The operator needs to THINK.

**Submarine CIC layout principles:**

- **Spatial consistency** — every station has a fixed position on the bridge. The sonar operator doesn't move. The OOW always knows where to look for sonar. Maps directly to: footer regions should be spatially fixed. The operator learns "context is always left, memory is always center."

- **Hierarchical detail** — the tactical display shows bearing lines and contact icons at a GLANCE (skill-based). Hover/select shows classification and course (rule-based). Drill-down shows raw sonar data (knowledge-based). Maps to: show the headline first, details on demand (Tab expand).

- **Redundant encoding** — bearing is shown as BOTH a line on the plot AND a number in the track table. Color AND position AND label all encode the same thing. Not abbreviation — REDUNDANCY.

- **Stable layout, changing values** — the display doesn't rearrange when a contact appears. Empty stations show "NO CONTACT" — the absence of data is information too. Maps to: always show all fields, zeros included.

- **Alarm states change the display, not the data** — when something goes critical, the BORDER changes, the COLOR changes, a TONE sounds. The information is the same — the presentation shifts to draw attention.

**Anti-pattern from our current approach:**
The fighting-game dense-text idea violates EID at every level. It's ALL knowledge-based — the operator must decode symbols and abbreviations. There's no skill-based perception (no gauges, no spatial meaning). No redundant encoding (one symbol = one meaning, miss it and you're lost).

**What the footer SHOULD be:**
- A fixed-position control surface with labeled sections
- Each section readable at a glance (skill-based: gauges, colors)
- Deeper info revealed on interaction (rule-based: Tab to expand)
- Full text labels, not abbreviations (knowledge-based as fallback)
- Empty state shows structure, not nothing

### Split-panel CIC layout — inference vs system state

**Operator's framing**: The footer is not 3-4 equal cards. It's two conceptual halves:

**Left half — "What is inferencing, what is being inferenced about"**
- Engine: model, tier, thinking, context mode, auth
- Memory/Minds: what knowledge is loaded, how much is injected, token budget
- Context gauge: how much runway remains
- This is the SONAR OPERATOR's station — "what are we tracking, what do we know"

**Right half — "What is the current state of the system driving the inference"**  
- Git tree: branches, dirty state, current branch highlighted
- System specs: cwd, OS, maybe CPU/memory utilization
- Session counters: turns, tool calls, compactions
- MCP connections, secrets, extensions
- The FRACTAL moves here as the ambient "sonar screen" — the living visualization of system state
- This is the ENGINEERING station — "what is the state of the boat"

**Why this split works (EID analysis)**:

1. **Spatial consistency**: Left = inference concerns (changes every conversation). Right = system concerns (changes rarely, mostly stable). The operator builds spatial intuition: "left tells me about the AI, right tells me about the machine."

2. **Attention hierarchy**: During a conversation, the operator glances LEFT — "am I running out of context? what model am I on?" They glance RIGHT much less often — "am I on the right branch?" The high-frequency information is on the reading side (left for LTR readers).

3. **The fractal as sonar**: In a submarine CIC, the sonar display is the ambient awareness instrument — always running, always showing the acoustic environment. The fractal serves the same role: ambient, always moving, encoding system state through shape/color/speed. Moving it to the system panel makes it the "sonar screen" of the engineering station — a living indicator of machine health.

4. **Size asymmetry**: Engine+Memory needs ~40% width (dense text, gauges). System needs ~60% width (git tree, fractal, counters). The asymmetry reflects information density — the left is compact readings, the right is spatial/visual.

**Proposed layout sketch**:
```
┌─ engine ──────────┬─ memory ──────────╫─ system state ─────────────────────────────┐
│ claude-opus-4-6   │ project  # 2565   ║ ⌂ ~/workspace/ai/omegon                    │
│ victory · ◎ low   │   inj 25 · ~3.7k  ║ ⎇ main · clean                             │
│ ████████░░ 12%    │ working  #    4   ║ ┌──────────────┐  T 3 · ⚙ 7 · ↻ 0          │
│ 120k / 1.0M       │ episodes # 147    ║ │ ≋≋ fractal ≋≋│  MCP 2(14t) · 🔓 3        │
│ ● anthropic sub   │                   ║ │ ≋≋ sonar   ≋≋│  31/49 tools active        │
│ native context    │                   ║ └──────────────┘  uptime 2h 14m             │
└───────────────────┴───────────────────╫─────────────────────────────────────────────┘
```

The `╫` double-line divider marks the conceptual split. Left half is two bordered sub-cards (engine + memory). Right half is one large panel with the fractal embedded as an inline element alongside system stats.

### Vertical space reallocation — conversation is compressible

**The conversation area is scroll history.** Once you've read a response, it scrolls up. Every row dedicated to conversation is a row of already-processed text. The INSTRUMENTS (footer, sidebar) are what the operator is actively monitoring while the AI works.

**Current allocation (50-row terminal)**:
- Conversation: 41 rows (82%)
- Editor: 3 rows
- Hint line: 1 row
- Footer: 5 rows (10%)

**Proposed reallocation**:
- Conversation: 33-35 rows (66-70%) — still shows 15+ lines of current response
- Editor: 3 rows (unchanged)
- Hint line: absorbed into footer or removed
- Footer/Instrument panel: 10-12 rows (20-24%)

**What 10-12 rows buys**:
- Left half (engine + memory): 5 rows engine, 5 rows memory = full EID display
- Right half (system state): 4-row fractal sonar + 6 rows of git/counters/MCP
- The hint line content (context mode, thinking level) moves into the engine card where it belongs
- Room for full text labels, no abbreviations needed

**The submarine analogy holds**: on a submarine bridge, the instruments take 60%+ of the wall space. The viewport (periscope/sonar display) is ONE instrument, not the whole room. Our "viewport" is the conversation — it's important but it's not the whole interface.

**Scaling behavior**: On tall terminals (80+ rows), conversation gets even more space naturally since it uses `Min(3)`. The footer stays fixed at 10-12. On short terminals (30 rows), 10-12 rows of footer would be too much — need a collapse threshold where we fall back to a compact 5-row layout.

### Focus mode, conversation tabs, and fractal state mapping

**Focus mode — toggle between instruments and content:**

The operator can toggle between two modes:
- **Normal**: 10-12 row instrument panel visible, conversation gets remaining space
- **Focus**: instrument panel disappears entirely, conversation gets full height. Toggle via hotkey or `/focus`. Useful for reading long responses, viewing rendered images/diagrams, or working in alternate tabs.

This eliminates the height budget concern entirely. The default is instrument-heavy. When you need the text, you toggle. The toggle is instant and stateless — your instruments are still updating in the background, they're just not rendered.

**Conversation area becomes multi-tab:**

The conversation is just one TAB of the main content area. Other tabs:
- **Conversation** (default) — the current chat
- **Design tree** — full interactive tree widget with expand/collapse, not the cramped sidebar version
- **Scratchpad / Notes** — quick capture for ideas, bugs, feature thoughts WITHOUT interrupting the agent. The operator thinks "I just noticed X" and switches to the notes tab, jots it down, switches back. The note is persisted (to a local file, git-tracked).
- **Issues** — lightweight issue tracker. Single-branch gists, bug reports, feature requests. Git-native (not GitHub-specific): could be a `notes/` directory, git-notes, or a simple issue format that can be pushed anywhere.

This is the "I don't want to interrupt the agent" workflow. The agent keeps working. The operator captures thoughts in a parallel surface.

**Git-native, not GitHub-specific:**
Issues/notes stored as files in the repo (`.omegon/notes/`, `.omegon/issues/`). Can be pushed to any git remote. If GitHub is available, a future integration could sync to GitHub Issues, but the ground truth is the local git repo. Gists are just single-file commits on a branch.

**Fractal → system state mapping (revised):**

The fractal is the sonar screen. It shows the SYSTEM's state, not just the agent's mode. With the tuned parameters from the demo session:

| System State | Algorithm | Visual Character | Parameters |
|---|---|---|---|
| **Idle** | Perlin flow | Smooth breathing, barely moving. The system is at rest. | scale=18, speed=0.3, octaves=2, amp=0.5 |
| **Agent thinking** | Plasma sine | Rippling fabric — structured interference. Something is being computed. | comp=1.65, speed=1.46, waves=4, dist=0.8 |
| **Tool execution** | Lissajous | Smooth looping trails — work is being done, paths are being traced. | curves=8, speed=0.68, freq=1.9 |
| **Cleave (parallel)** | Lissajous intense | More curves, faster, wider hue sweep — multiple workers. | curves=12, speed=0.85, wider hue |
| **Compaction** | Brief Perlin burst | Speed spike then settle — the system is reorganizing. | speed jumps to 3.0 for 2 seconds |
| **Error/degraded** | Perlin very slow, desaturated | Almost frozen, low saturation — something is wrong. | speed=0.05, amp=0.2 |

The Clifford attractor was dropped earlier because it's too unstable (collapses to scattered points in some parameter regions). The four states above use only Perlin, Plasma, and Lissajous — all of which are stable and tuned.

**Fractal size in system panel:**
No reason to shrink it. If the right half is 60% of 160 cols = 96 cols, and the system stats only need ~40 cols of text, the fractal can be 50×10 — much larger than the current 36×8. It becomes a proper sonar display, not a thumbnail. The stats sit beside it or below it.

### Multi-instrument display — four simultaneous fractals

**CIC analogy**: A submarine CIC has sonar waterfall, bearing plot, frequency analysis, AND tactical overlay — all running simultaneously, each showing a different dimension of the same acoustic environment. We should do the same.

**Four instruments in a 2×2 grid in the system panel:**

| Position | Name | Algorithm | Telemetry source | Visual signature |
|---|---|---|---|---|
| Top-left | **Sonar** | Perlin flow | Context utilization % | Speed/turbulence increases with context fill. Calm=low, churning=near capacity |
| Top-right | **Radar** | Lissajous | Tool execution rate | Still when idle, looping trails during tool calls, intense curves during cleave |
| Bottom-left | **Thermal** | Plasma sine | Thinking/model activity | Flat when waiting for input, rippling during inference, fast waves during extended thinking |
| Bottom-right | **Signal** | Clifford attractor | Memory system activity | Sparse when idle, dense during injection, evolving pattern during store/recall operations |

**Each instrument**: ~22×4 cells (half-block = 8 visual rows). Labeled with a dim title. Parameters shift independently based on their telemetry source. Always running — the stillness of an idle instrument IS information.

**What the operator perceives at a glance:**
- All four calm = system idle, nothing happening
- Sonar calm + Radar active + Thermal rippling = tool execution in progress, model thinking, context fine
- All four active = cleave or heavy multi-tool session
- Sonar turbulent + everything else calm = context is filling up, approaching compaction

**Clifford attractor stabilization:**
The attractor collapsed because parameters drifted into sparse orbits. Fix: constrain the parameter space to a known-good region. Use a lookup table of pre-validated (a, b, c, d) tuples and interpolate between them rather than free-drifting. The evolve_speed controls interpolation rate between known-good states, never venturing into sparse territory.

**Layout in system panel (~96×12 area):**
```
┌─ sonar ─────────┐ ┌─ radar ─────────┐
│  22×4 Perlin     │ │  22×4 Lissajous │  stats column
│                  │ │                 │  (30 chars wide)
└──────────────────┘ └─────────────────┘
┌─ thermal ───────┐ ┌─ signal ────────┐
│  22×4 Plasma    │ │  22×4 Clifford  │
│                  │ │                 │
└──────────────────┘ └─────────────────┘
```

Each instrument is small enough to be ambient but large enough to be readable. The 2×2 grid uses ~48×10 of the system panel, leaving ~48×10 for text stats on the right.

### Cross-instrument visual features — linked minds and injection band

**Linked minds as waterfall columns:**

The waterfall's 22-column width divides into segments per active mind. The structural change (column count) IS the mind count — no number reading needed.

- 1 mind (project only): full 22-column waterfall
- 2 minds (project + working): two 10-column waterfalls with 2-col gap
- 3 minds (+ episodes): three 6-column waterfalls with 2-col gaps
- 4 minds (+ archive): four 4-column waterfalls with 2-col gaps

When a mind activates, a new column segment appears. When it deactivates, the segment fades and merges back. The gap between segments uses a thin border character (│) in dim color.

Each segment can run slightly different CA parameters to show which mind is active — project could use Rule 30, working uses Rule 110, etc. Or they all use the same rule but the active mind's segment is brighter.

**Context injection band on sonar:**

The sonar (Perlin) shows context fill as intensity across the whole instrument. The memory injection portion is a distinct visual layer at the bottom:

- Bottom N rows of the sonar show dim CRT glitch characters (from the waterfall glyph set) overlaid on the Perlin field
- N is proportional to memory_tokens / context_window
- When memory injects 10% of context, the bottom 10% of the sonar has glitch texture
- The glitch band uses the same navy→teal ramp but at ~30% brightness — dim but visible as a different texture
- As injection grows, the glitch band rises, visually showing "this much of your context is occupied by memory"

This creates a visual bridge between the sonar and waterfall instruments — they share the glitch glyph language. The sonar's glitch band echoes the waterfall's aesthetic, reinforcing that both instruments are related to memory/knowledge.

**Why this works (EID):**
- Mind count is SPATIAL (column segments) — skill-based perception, no reading
- Injection band is TEXTURE (glitch vs smooth) — skill-based, a different visual feel in the same space
- Both use existing visual language (glitch chars, color ramp) — no new symbols to learn

## Decisions

### Decision: Footer grows to 10-12 rows, conversation absorbs the loss

**Status:** exploring
**Rationale:** Conversation is scroll history — compressible. The instrument panel is the operator's persistent situational awareness surface. Allocating 20-24% of vertical space to instruments (vs current 10%) follows the CIC pattern where instruments dominate and the viewport is one element among many. Compact fallback at terminal heights under 35 rows.

### Decision: Four simultaneous fractal instruments, not one switching display

**Status:** exploring
**Rationale:** A submarine CIC runs sonar, radar, thermal, and signal analysis simultaneously — different instruments showing different dimensions of the same environment. Each of our four algorithms maps to a distinct telemetry source: Perlin=context health, Lissajous=tool activity, Plasma=thinking state, Clifford=memory activity. Running all four simultaneously gives the operator peripheral awareness of all system dimensions at once. The pattern of which instruments are active/calm IS the situational awareness.

### Decision: Unified color language: idle navy → stormy blue → amber at maximum

**Status:** decided
**Rationale:** All four instruments share the same color ramp: idle navy (near-black teal), increasing activity shifts toward brighter blue, maximum intensity shifts hue toward amber. This keeps every instrument visually consistent with one another and with the theme's existing color meanings (teal=normal, amber=warning). The operator reads intensity across all four instruments as a unified signal — no need to learn per-instrument color vocabularies. Shape (algorithm) differentiates the instruments, color (intensity) differentiates the state.

### Decision: Color ramp: dark navy (idle) → teal (normal) → amber (maximum)

**Status:** decided
**Rationale:** Teal is the Alpharius brand color — it belongs at the center of the ramp as the steady-state "everything is nominal" reading. Dark navy below it for idle/resting. Amber above it for high load/attention needed. The operator's eye calibrates to teal as normal; darker means quieter, warmer means hotter. This matches the existing theme semantics where teal=accent (normal) and amber=warning.

### Decision: Split-panel layout: inference (left 40%) / system state (right 60%)

**Status:** decided
**Rationale:** Left half = what is inferencing and what is being inferenced about (engine config + linked minds). Right half = what is the state of the system driving the inference (four fractal instruments + operational stats). Maps to CIC station separation. High-frequency glance target (inference) on the reading side for LTR.

### Decision: Footer grows to 10-12 rows with focus mode toggle

**Status:** decided
**Rationale:** Conversation is compressible scroll history. Instruments are the persistent situational awareness surface. Default is instrument-heavy (10-12 rows). Focus mode (hotkey or /focus) hides the instrument panel entirely for full-height conversation — useful for reading long outputs, viewing rendered images, or working in alternate tabs. Compact fallback at terminal heights under 35 rows.

### Decision: Four simultaneous fractal instruments in 2×2 grid

**Status:** decided
**Rationale:** CIC pattern — multiple instruments running simultaneously, each showing a different dimension of system state. Perlin=context health, Plasma=thinking/inference, Lissajous=tool activity, Clifford=memory activity. The pattern across all four IS the situational awareness. Shape differentiates instruments, unified color ramp differentiates intensity.

## Open Questions

- With the fractal moving to the system panel, what replaces it in the dashboard header? Or does the dashboard header collapse and the sidebar gains that space for the design tree?
- Should each instrument have its own hue (sonar=teal, radar=green, thermal=amber, signal=blue) or should they all share the Alpharius teal band and differentiate only through shape?

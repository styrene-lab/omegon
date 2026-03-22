# Instrument algorithm refactor — thinking as glitch, memory as sine strings — Design Spec (extracted)

> Auto-extracted from docs/instrument-algorithm-refactor.md at decide-time.

## Decisions

### Thinking glitch overlays the context bar as a static field, not a waterfall (decided)

Thinking IS what's happening inside context. The glitch chars appear on the context bar surface — denser when thinking harder. No scrolling needed because the context bar itself is the spatial reference. The glitch disrupts the clean bar surface, making thinking visible as a perturbation of the context state.

### Memory sine waves are plucked then dampen — direction encodes read vs write (decided)

Each memory operation plucks the string. The wave propagates rightward (→) for storage, leftward (←) for retrieval. Amplitude and speed indicate intensity. The wave dampens naturally — a quiet string means no recent activity. Multiple rapid operations create interference patterns as waves overlap. This maps memory ops to a physical metaphor the operator develops intuition for.

### Thinking overlays context directly — glitch chars on the bar surface (decided)

Option (b) wins — thinking bleeds into context. The context bar is a clean gradient when idle. When thinking, glitch chars appear across its surface, denser at higher thinking levels. Context and thinking share the same pixels. A tree connector links context down to memory strings, showing the data flow path.

### Tools as bubble-sort list with recency bars, not Lissajous curves (decided)

Lissajous curves don't communicate WHICH tools are active. A sorted list with tool names, recency bars, and time-since-last-call is self-documenting. The list physically reorders when tools fire (bubble-to-top animation). The operator sees 'bash was called 3s ago' not 'there are teal curves.' Tool error = red bar that stays at top.

### Two-panel layout: inference state (left) + tool activity (right) (decided)

Replace the 2×2 grid with LEFT (unified inference: context bar + thinking glitch overlay + tree connector + memory sine strings) and RIGHT (tool bubble-sort list). Left tells the inference story top-to-bottom. Right shows execution activity. The 2×2 grid's four independent panels become two interconnected panels.

## Research Summary

### Unified instrument layout — context/thinking merged, tools as sort visualization, memory as linked sine strings



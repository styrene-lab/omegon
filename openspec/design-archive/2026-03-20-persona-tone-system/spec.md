+++
id = "15b8f42b-94b8-49ff-9721-b839cf4d351e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Tone system — conversational voice layer independent of persona expertise — Design Spec (extracted)

> Auto-extracted from docs/persona-tone-system.md at decide-time.

## Decisions

### Tone is a separate axis from persona: persona = what you know, tone = how you speak (decided)

Tone and persona are orthogonal. A PCB designer persona can speak in an Alan Watts tone or a terse military tone. A tutor persona might default to socratic tone but operator can override. Tone is lower-impact during coding (preference), transformative during design/creative work. Tone comprises: TONE.md directive + exemplars/ directory (curated voice passages) + lightweight tone_observations that accumulate in project memory. Not a full separate fact store — lighter than a persona mind.

### Tone intensity is context-aware: full voice in design/creative, muted in coding/execution (decided)

During design tree exploration, creative brainstorming, and architectural discussion, tone shapes the quality of thinking and output. During mechanical code execution, test runs, and file operations, tone is noise. The harness can infer context from the current cognitive mode — design_tree operations and open-ended conversation get full tone, tool execution and cleave children get muted tone. Operator can override to always-full or always-muted.

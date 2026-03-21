---
id: persona-tone-system
title: Tone system — conversational voice layer independent of persona expertise
status: implemented
parent: persona-system
tags: [persona, tone, voice, creative, ux, design-mode]
open_questions: []
issue_type: feature
---

# Tone system — conversational voice layer independent of persona expertise

## Overview

A tone is the conversational voice of the entire Omegon interface — distinct from persona expertise. An 'Alan Watts' tone shifts the philosophical framing and linguistic register of all output. Tones are lower-impact during coding (preference) but transformative during design and creative work. Key question: does a tone need its own fact store (quotes, philosophy, voice patterns) or is it a lighter-weight markdown overlay that layers into the prompt?

## Decisions

### Decision: Tone is a separate axis from persona: persona = what you know, tone = how you speak

**Status:** decided
**Rationale:** Tone and persona are orthogonal. A PCB designer persona can speak in an Alan Watts tone or a terse military tone. A tutor persona might default to socratic tone but operator can override. Tone is lower-impact during coding (preference), transformative during design/creative work. Tone comprises: TONE.md directive + exemplars/ directory (curated voice passages) + lightweight tone_observations that accumulate in project memory. Not a full separate fact store — lighter than a persona mind.

### Decision: Tone intensity is context-aware: full voice in design/creative, muted in coding/execution

**Status:** decided
**Rationale:** During design tree exploration, creative brainstorming, and architectural discussion, tone shapes the quality of thinking and output. During mechanical code execution, test runs, and file operations, tone is noise. The harness can infer context from the current cognitive mode — design_tree operations and open-ended conversation get full tone, tool execution and cleave children get muted tone. Operator can override to always-full or always-muted.

## Open Questions

*No open questions.*

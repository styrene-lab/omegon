+++
id = "ba125215-f3db-4dc4-81b9-c87a8867e2d4"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Conversation visual identity — differentiate operator, agent, thinking, tools

## Overview

The current conversation rendering uses left-edge vertical bars for both user (▎ accent) and agent (│ green). The bars are too similar — the differentiation reads as a bug. Need to rethink how each message type communicates its source within Omegon's design language.

## Research

### First principles — who talks and what matters



## Decisions

### Decision: Agent text is plain (no bar, no decoration). Operator messages get a thin accent bar + bold. Thinking is dimmed and collapsed.

**Status:** decided
**Rationale:** The agent produces 90% of the conversation content — decorating it adds noise to the majority of the screen. Agent text should be the cleanest rendering: normal weight, full width, no gutter. Operator messages are the punctuation marks — a thin accent ▎ bar and bold text signals 'you spoke here' without being heavy. Thinking gets ◎ icon, dim color, collapsed. Tool cards keep their bordered card treatment (already good). System messages get a distinct dim color with no chrome. This follows EID: operator inputs are highlighted to confirm action, system output is the natural background state.

## Open Questions

*No open questions.*

## The conversation has 5 message types

1. **Operator message** — "I said this." Confirmation of input.
2. **Agent text** — The primary output. Most of the screen, most of the time.
3. **Thinking** — Internal reasoning. Usually collapsed. Secondary.
4. **Tool cards** — Execution results. Already have their own bordered card treatment (this works well).
5. **System messages** — Status, errors, command feedback. Transient.

## The core insight

The AGENT talks most of the time. Agent text is the default state of the display — it should be the cleanest, least decorated. The OPERATOR's messages are the interruptions — they should pop out to confirm "you said this, and here's where it happened in the flow."

Current design: both have vertical bars → both have visual chrome → neither stands out.

## What works in other systems

- **Submarine CIC displays**: operator inputs are highlighted, system outputs are the background state
- **Chat apps**: user messages are colored bubbles, agent/other messages are plain
- **IDE terminals**: user input is bold, output is normal weight
- **IRC**: sender nick is colored, message is plain

## Design direction

Agent text = plain. No decoration. Just text flowing naturally.
Operator message = marked. A clear "you spoke here" signal.
Thinking = collapsed, dimmed, secondary.
Tool cards = bordered cards (already good).
System = distinct color, no chrome.

The question is what "marked" means for operator messages without being heavy-handed.

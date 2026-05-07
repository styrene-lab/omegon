+++
id = "fb760877-ffb5-4a36-a3de-ab70bdc3e8f8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Conversation visual identity — differentiate operator, agent, thinking, tools — Design Spec (extracted)

> Auto-extracted from docs/conversation-visual-identity.md at decide-time.

## Decisions

### Agent text is plain (no bar, no decoration). Operator messages get a thin accent bar + bold. Thinking is dimmed and collapsed. (decided)

The agent produces 90% of the conversation content — decorating it adds noise to the majority of the screen. Agent text should be the cleanest rendering: normal weight, full width, no gutter. Operator messages are the punctuation marks — a thin accent ▎ bar and bold text signals 'you spoke here' without being heavy. Thinking gets ◎ icon, dim color, collapsed. Tool cards keep their bordered card treatment (already good). System messages get a distinct dim color with no chrome. This follows EID: operator inputs are highlighted to confirm action, system output is the natural background state.

## Research Summary

### First principles — who talks and what matters



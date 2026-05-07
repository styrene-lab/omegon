+++
id = "8d3b5ed1-a4c7-4be4-bd80-33c25adeb89b"
kind = "document"
title = "Conversation collapse and filter — tree-style fold with view modes"
status = "exploring"
tags = ["tui", "ux", "conversation", "filter", "collapse", "0.15.1"]
aliases = ["conversation-collapse-filter"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = ["Should filter modes cycle (Full→Dialogue→Tools→Compact→Full) or use explicit selection via a popup selector like /model?", "Should the filter persist across turns (agent response adds new segments in the current filter mode) or reset to Full when the agent starts a new turn?"]
parent = "conversation-widget"
priority = "2"
+++

# Conversation collapse and filter — tree-style fold with view modes

## Overview

The conversation panel accumulates tool calls, thinking blocks, and system messages that bury the actual human↔agent dialogue. Need a collapse/filter system with three entry points (button, hotkey, /collapse command) that all do the same thing: toggle a view mode on the conversation.

View modes:
1. **Full** (default) — everything visible, current behavior
2. **Dialogue** — collapse all tool calls and thinking, show only operator prompts and agent responses. Tool calls become single-line summaries (✓ bash · 3s). Conversation reads like a chat transcript.
3. **Tools** — show ONLY tool calls, all expanded to maximum detail. Agent text and operator prompts hidden. For debugging what the agent actually did.
4. **Compact** — tool calls collapsed to one-line, thinking hidden, agent text visible but truncated to first paragraph. Dense overview.

Each segment in the conversation is already a typed enum (UserPrompt, AssistantText, ToolCard, etc.) — the filter is a predicate over segment types plus a display-width override per type.

Entry points: /collapse or /filter command, Ctrl+F hotkey, small [F] button in conversation header.

## Open Questions

- Should filter modes cycle (Full→Dialogue→Tools→Compact→Full) or use explicit selection via a popup selector like /model?
- Should the filter persist across turns (agent response adds new segments in the current filter mode) or reset to Full when the agent starts a new turn?

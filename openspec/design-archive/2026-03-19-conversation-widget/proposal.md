+++
id = "68138c15-649b-4d64-805c-d5e094bb49cc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Conversation widget — structured rendering for all message types

## Intent

Replace the current Vec<Line>-in-a-Paragraph approach with a proper structured conversation widget. Each message type (user, assistant, tool call, thinking, system notification) gets its own rendering strategy with correct backgrounds, borders, wrapping, and visual hierarchy. Must support inline images (Kitty protocol), syntax highlighting, line numbers, diff rendering, and expandable/collapsible sections. This is the foundation for all future TUI work — design-tree, openspec, and cleave views all depend on it.

See [design doc](../../../docs/conversation-widget.md).

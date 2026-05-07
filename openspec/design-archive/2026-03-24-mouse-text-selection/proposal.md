+++
id = "b7ee9a39-92be-47d8-940e-75bf73ce0937"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Mouse text selection — EnableMouseCapture blocks native terminal selection

## Intent

EnableMouseCapture grabs all mouse events for scroll-wheel handling, which prevents the terminal emulator from doing native text selection (click-drag-copy). This is a fundamental tradeoff in crossterm/ratatui apps. Need to find an approach that preserves scroll support while restoring text selection.

See [design doc](../../../docs/mouse-text-selection.md).

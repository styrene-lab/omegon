+++
id = "7271ea90-82bd-4696-adb8-ce815881bdac"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Tool Card Visual System — Omegon-owned card rendering with borders, state colors, and glyphs

## Intent

The pi-mono Box component is a plain padding+background container with no visual identity. Every Omegon tool that renders to the TUI needs cards with state-aware borders, glyph identity, and the Alpharius color language. Rather than patching Box upstream, Omegon should own a ToolCard component that wraps Box (or replaces it) and provides the visual system for all tool rendering — built-in and extension alike.\n\nThis is the rendering equivalent of what sci-ui.ts does for call/result lines, but for the full card body.

See [design doc](../../../docs/tool-card-system.md).

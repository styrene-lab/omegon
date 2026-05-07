+++
id = "ae06a374-530e-4111-9dbf-ad7bc689a6ef"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Replace custom Editor with ratatui-textarea — multi-line input with clipboard paste

## Overview

Replace the custom 514-line Editor (editor.rs) with ratatui-textarea. Gains: multi-line editing with proper cursor navigation, clipboard paste (bracketed paste mode from crossterm), undo/redo, word movement, and optional vim keybindings. The dependency is already added (0.8, crossterm feature). Migration touches: event handling in mod.rs (crossterm events → textarea Input), history management (currently custom), reverse-search (may need to reimplement on top), and render integration (textarea is a Widget). The existing Editor::render_text() / cursor_position() API surface maps cleanly to textarea.lines() / cursor().

## Decisions

### Decision: Clipboard image paste via platform-native tools, not crossterm

**Status:** decided
**Rationale:** crossterm's Event::Paste only delivers text (bracketed paste mode). Image data requires platform-specific extraction: osascript/AppleScript on macOS, xclip on Linux. Support all common formats (PNG, JPEG, TIFF, GIF, BMP, WebP) since clipboard content type is unpredictable — screenshots may be TIFF on macOS, PNG on Linux. Images are base64-encoded and attached as content blocks in the LLM API call (Anthropic source.type=base64, OpenAI image_url data: URI).

## Open Questions

*No open questions.*

+++
id = "4352e052-0ea7-48a1-bd2e-3f9a3ba2ff31"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Inline image rendering — ratatui-image integration and clipboard paste — Design Spec (extracted)

> Auto-extracted from docs/inline-image-rendering.md at decide-time.

## Decisions

### Tool-result images first, clipboard paste deferred (decided)

Images arrive from tool results (view, render, generate_image_local) as file paths — these are the common case. Clipboard image paste requires platform-specific external commands (pbpaste/xclip) and crossterm doesn't expose Kitty's image paste protocol. Deferring paste keeps scope tight.

### Use ratatui-image Picker for protocol detection at startup (decided)

ratatui-image's Picker::from_termios() queries the terminal for Kitty/Sixel/iTerm2 support and falls back to halfblock Unicode rendering. One call at startup, stored globally. No Kitty-specific code needed — the library handles protocol negotiation.

### Segment::Image stores PathBuf with lazy DynamicImage decode (decided)

PathBuf is the source of truth. DynamicImage is decoded on first render and cached. ratatui-image handles resizing/scaling internally — no cache invalidation needed on terminal resize. The StatefulImage protocol image is created per-render from the DynamicImage (cheap operation). Fallback for decode failures: render as [image: path] text.

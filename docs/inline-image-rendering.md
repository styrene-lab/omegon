+++
id = "358d87f3-5fbc-4c5f-bb2c-d7c731dca117"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Inline image rendering — ratatui-image integration and clipboard paste

## Overview

> Parent: [Conversation widget — structured rendering for all message types](conversation-widget.md)
> Spawned from: "Clipboard/paste handling for images — detect image data in paste (Kitty's paste protocol includes MIME type), save to temp dir, render inline? Or require explicit /image command?"

*To be explored.*

## Decisions

### Decision: Tool-result images first, clipboard paste deferred

**Status:** decided
**Rationale:** Images arrive from tool results (view, render, generate_image_local) as file paths — these are the common case. Clipboard image paste requires platform-specific external commands (pbpaste/xclip) and crossterm doesn't expose Kitty's image paste protocol. Deferring paste keeps scope tight.

### Decision: Use ratatui-image Picker for protocol detection at startup

**Status:** decided
**Rationale:** ratatui-image's Picker::from_termios() queries the terminal for Kitty/Sixel/iTerm2 support and falls back to halfblock Unicode rendering. One call at startup, stored globally. No Kitty-specific code needed — the library handles protocol negotiation.

### Decision: Segment::Image stores PathBuf with lazy DynamicImage decode

**Status:** decided
**Rationale:** PathBuf is the source of truth. DynamicImage is decoded on first render and cached. ratatui-image handles resizing/scaling internally — no cache invalidation needed on terminal resize. The StatefulImage protocol image is created per-render from the DynamicImage (cheap operation). Fallback for decode failures: render as [image: path] text.

## Open Questions

*No open questions.*

+++
id = "1c799460-bd13-4586-a607-45e4eab2f77e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Conversation widget — structured rendering for all message types — Design Spec (extracted)

> Auto-extracted from docs/conversation-widget.md at decide-time.

## Decisions

### Segment-based architecture — each message type is a typed enum variant rendered as its own widget (decided)

The current Vec<Line>-in-a-Paragraph approach fundamentally cannot produce per-card backgrounds, borders, images, or interactive elements. The replacement: a typed Segment enum where each variant (UserPrompt, AssistantText, ToolCard, ThinkingBlock, Image, SystemNotification, TurnSeparator) renders as an independent widget with its own Block, background, and internal layout. A custom ConversationWidget implements StatefulWidget, maintains scroll state, and renders only visible segments. This is the architectural foundation for design-tree, openspec, and cleave views.

### Kitty-opinionated image rendering via ratatui-image (decided)

ratatui-image v5.0 handles protocol detection (Kitty/Sixel/iTerm2/Halfblocks fallback) and renders as a StatefulWidget. We'll be opinionated about Kitty as the primary target since the operator uses Kitty-compatible terminals. Images from tool results (view, render) and pasted screenshots render inline as Segment::Image variants. The Picker detects capabilities once at startup. Sixel/halfblock fallback means it works everywhere, just at lower quality.

### Streaming handled via mutable last segment (decided)

During streaming, the last segment is always an AssistantText with complete=false. New text chunks append to it in place. The scroll system knows to re-measure the last segment's height each frame. When the stream completes, complete=true is set. This is the simplest approach and matches the current behavior. No double-buffering needed — the immediate-mode render loop already redraws every 16ms.

### Height computed by rendering into a throw-away buffer — no duplicated wrapping logic (decided)

Rather than pre-calculating heights (which requires duplicating Paragraph's wrapping logic), render each segment into a temporary Buffer of the correct width and unlimited height, then measure how many rows were actually used. This is what tui-scrollview does internally. The cost is negligible — rendering 20-30 visible segments at 60fps is < 1ms. Heights are cached and invalidated only when the segment changes or the terminal width changes.

### Height computed by render-into-temp-buffer with OnceLock-cached syntax resources (decided)

Supersedes the earlier line-counting approach which diverged from Paragraph's word-aware wrapping. Now renders into a temp buffer (capped at 300 rows) and scans for the last used row. Syntax highlighting resources (SyntaxSet, ThemeSet) are cached in a static OnceLock — loaded once, reused for every render. The temp buffer allocation per height() call is acceptable since it's only called when heights are invalidated (resize, new segment, expand toggle), not every frame.

## Research Summary

### Current architecture — why it fails

**The root problem: the conversation is a single `Paragraph`.**

```rust
// Current: everything is Vec<Line> → one Paragraph
let conv_text: Vec<Line> = self.conversation.render_themed(t);
let widget = Paragraph::new(conv_text)
    .block(conv_block)
    .wrap(Wrap { trim: false })
    .scroll((offset, 0));
frame.render_widget(widget, area);
```

Why this fails:
1. **No per-segment backgrounds** — Paragraph wraps text within one style context. A tool card can't have `card_bg` while the surroundin…

### Available ratatui ecosystem components

**tui-scrollview (by Joshka, ratatui maintainer):**
- Creates a virtual `Buffer` larger than the visible area
- Renders child widgets into the virtual buffer at absolute positions
- Scrolls a viewport window over the virtual buffer
- Supports both vertical and horizontal scrolling
- Each child widget is independent — its own Block, background, layout
- This is the core primitive we need for the conversation

**ratatui-image v5.0:**
- `StatefulImage` widget for rendering images in the terminal
- …

### Proposed architecture — segment-based conversation widget

**Core concept: the conversation is a list of Segments, each rendered as its own widget.**

```rust
enum Segment {
    UserPrompt { text: String },
    AssistantText { text: String, complete: bool },
    ThinkingBlock { text: String },
    ToolCard { 
        name: String, args: Value, result: Option<ToolResult>,
        is_error: bool, complete: bool, collapsed: bool,
    },
    SystemNotification { text: String, level: NotifyLevel },
    Image { path: PathBuf, protocol: StatefulProtocol },
   …

+++
id = "4f2e9cd8-15a9-48a6-9a51-e0da4062f332"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Conversation widget — structured rendering for all message types

## Overview

Replace the current Vec<Line>-in-a-Paragraph approach with a proper structured conversation widget. Each message type (user, assistant, tool call, thinking, system notification) gets its own rendering strategy with correct backgrounds, borders, wrapping, and visual hierarchy. Must support inline images (Kitty protocol), syntax highlighting, line numbers, diff rendering, and expandable/collapsible sections. This is the foundation for all future TUI work — design-tree, openspec, and cleave views all depend on it.

## Research

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
1. **No per-segment backgrounds** — Paragraph wraps text within one style context. A tool card can't have `card_bg` while the surrounding text has `bg`. Background fragments bleed when lines wrap.
2. **No per-segment borders** — Box-drawing characters (╭╮╰╯) are just text chars, not real borders. They don't extend to fill the terminal width. When a line wraps, the border breaks.
3. **No per-segment layout** — Each tool card needs its own height calculation, padding, and internal layout. Paragraph treats everything as a continuous text flow.
4. **No images** — Paragraph renders text cells only. Images need Kitty graphics protocol escape sequences placed at specific cell positions.
5. **No interaction** — Can't click to expand/collapse a tool card. Can't hover for details. Each segment needs its own hit testing.
6. **Scroll is line-based, not segment-based** — Scrolling by line count means you can land mid-card. Segment-based scrolling would snap to message boundaries.

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
- Auto-detects graphics protocol: Kitty, Sixel, iTerm2, Halfblocks (fallback)
- `Picker` does protocol detection once at startup
- Image resizing handled internally
- Renders as a `StatefulWidget` — fits into the scrollview

**ratatui built-in:**
- `Block` with `Borders::ALL` + `BorderType::Rounded` — per-card borders
- `Paragraph` with `Wrap` — text content within a card
- `Layout` — split a card's inner area for header/content/footer
- `Table` — could be used for line-numbered code display
- `StatefulWidget` — mutable rendering state (collapse/expand)

**What's NOT available off-the-shelf:**
- Syntax highlighting — need `syntect` or manual regex
- Diff rendering — need custom widget
- Expandable/collapsible sections — need custom state + click handling
- Line numbers with gutter — need custom widget or Table abuse

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
    TurnSeparator { turn: u32 },
}
```

**Rendering pipeline:**

```
ConversationState.segments: Vec<Segment>
    │
    ▼
ConversationWidget::render(area, buf, state)
    │
    ├─ Calculate total virtual height (sum of segment heights)
    ├─ Create ScrollView with virtual height
    ├─ For each visible segment:
    │   ├─ UserPromptWidget::render(segment_area, buf)
    │   ├─ AssistantTextWidget::render(segment_area, buf)
    │   ├─ ToolCardWidget::render(segment_area, buf)
    │   ├─ ImageWidget::render_stateful(segment_area, buf, state)
    │   └─ etc.
    └─ ScrollView handles viewport offset
```

**Per-segment rendering:**

1. **UserPrompt**: `▸` prefix, bold text, full-width bg
2. **AssistantText**: Markdown structural highlighting (headers, bold, code fences), wrapping, streaming cursor
3. **ThinkingBlock**: Dimmed, collapsible, `◌ thinking…` header
4. **ToolCard**: Full Block with borders, internal layout:
   ```
   ╭─ ✓ bash ─────────────────────────────────╮
   │ $ cargo build --release                   │
   │ ~/workspace · 54.7s · exit 0             │
   ├───────────────────────────────────────────┤
   │ Compiling omegon v0.12.0                  │
   │ Finished release [optimized] 54.68s       │
   │ … 42 lines total                          │
   ╰───────────────────────────────────────────╯
   ```
5. **Image**: ratatui-image StatefulImage, sized to fit
6. **SystemNotification**: Dimmed, single-line, icon prefix

**Key architectural decisions:**

- Each segment knows its own height (calculated before layout)
- Segments render into absolute positions in the scroll buffer
- Scroll state is segment-aware: can snap to segment boundaries
- Tool cards have internal collapse state: header-only when collapsed
- Images are loaded lazily (path → StatefulProtocol on first render)
- The conversation widget implements StatefulWidget (scroll offset + per-card states)

**Dependencies to add:**
- `tui-scrollview` — virtual scrolling container
- `ratatui-image` — Kitty/Sixel image rendering
- `syntect` (optional, deferred) — syntax highlighting for code blocks

**What this enables downstream:**
- Design-tree views can be a segment type (tree widget in scrollview)
- OpenSpec scenario views can be a segment type
- Cleave progress can be a segment type
- Each is independently scrollable, collapsible, interactive

## Decisions

### Decision: Segment-based architecture — each message type is a typed enum variant rendered as its own widget

**Status:** decided
**Rationale:** The current Vec<Line>-in-a-Paragraph approach fundamentally cannot produce per-card backgrounds, borders, images, or interactive elements. The replacement: a typed Segment enum where each variant (UserPrompt, AssistantText, ToolCard, ThinkingBlock, Image, SystemNotification, TurnSeparator) renders as an independent widget with its own Block, background, and internal layout. A custom ConversationWidget implements StatefulWidget, maintains scroll state, and renders only visible segments. This is the architectural foundation for design-tree, openspec, and cleave views.

### Decision: Kitty-opinionated image rendering via ratatui-image

**Status:** decided
**Rationale:** ratatui-image v5.0 handles protocol detection (Kitty/Sixel/iTerm2/Halfblocks fallback) and renders as a StatefulWidget. We'll be opinionated about Kitty as the primary target since the operator uses Kitty-compatible terminals. Images from tool results (view, render) and pasted screenshots render inline as Segment::Image variants. The Picker detects capabilities once at startup. Sixel/halfblock fallback means it works everywhere, just at lower quality.

### Decision: Streaming handled via mutable last segment

**Status:** decided
**Rationale:** During streaming, the last segment is always an AssistantText with complete=false. New text chunks append to it in place. The scroll system knows to re-measure the last segment's height each frame. When the stream completes, complete=true is set. This is the simplest approach and matches the current behavior. No double-buffering needed — the immediate-mode render loop already redraws every 16ms.

### Decision: Height computed by rendering into a throw-away buffer — no duplicated wrapping logic

**Status:** decided
**Rationale:** Rather than pre-calculating heights (which requires duplicating Paragraph's wrapping logic), render each segment into a temporary Buffer of the correct width and unlimited height, then measure how many rows were actually used. This is what tui-scrollview does internally. The cost is negligible — rendering 20-30 visible segments at 60fps is < 1ms. Heights are cached and invalidated only when the segment changes or the terminal width changes.

### Decision: Height computed by render-into-temp-buffer with OnceLock-cached syntax resources

**Status:** decided
**Rationale:** Supersedes the earlier line-counting approach which diverged from Paragraph's word-aware wrapping. Now renders into a temp buffer (capped at 300 rows) and scans for the last used row. Syntax highlighting resources (SyntaxSet, ThemeSet) are cached in a static OnceLock — loaded once, reused for every render. The temp buffer allocation per height() call is acceptable since it's only called when heights are invalidated (resize, new segment, expand toggle), not every frame.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `crates/omegon/src/tui/segments.rs` (new) — Segment enum + per-type rendering functions. Each variant renders into a Block with proper borders, backgrounds, and internal layout. Replaces widgets::tool_card_detailed and the render logic in conversation.rs.
- `crates/omegon/src/tui/conv_widget.rs` (new) — ConversationWidget implementing StatefulWidget. Owns scroll state, segment height cache, viewport calculation. Renders only visible segments at computed y-offsets. Replaces the current Paragraph-based rendering in mod.rs.
- `crates/omegon/src/tui/conversation.rs` (modified) — Refactor to store Vec<Segment> instead of Vec<Message>. Push methods create typed segments. Remove render_themed() — rendering moves to ConversationWidget.
- `crates/omegon/src/tui/mod.rs` (modified) — Replace Paragraph::new(conv_text) with frame.render_stateful_widget(ConversationWidget, area, state). Pass theme + terminal width for height computation.
- `crates/omegon/src/tui/image.rs` (new) — Image segment support — ratatui-image Picker initialization, StatefulProtocol management, temp file handling for pasted images.
- `crates/omegon/src/tui/widgets.rs` (modified) — Remove tool_card_detailed, tool_card (moved to segments.rs). Keep shared primitives (gauge_bar, highlight_line, etc).
- `crates/omegon/Cargo.toml` (modified) — Add ratatui-image dependency for inline image rendering.

### Constraints

- Each segment must be independently renderable with its own Block — no shared Paragraph
- Visible-only rendering — never render segments outside the viewport
- Height cache invalidated on terminal resize and segment mutation only
- Image rendering is optional — graceful degradation to [image: path] text on unsupported terminals
- Must handle 1000+ segments without performance degradation (cursor movement, scrolling)

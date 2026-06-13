+++
title = "TUI UI Landscape and Widget Map"
tags = ["tui","ratatui","ui","widgets","architecture"]
+++

# TUI UI Landscape and Widget Map

---
title: TUI UI Landscape and Widget Map
status: implemented
tags: [tui, ratatui, ui, widgets, architecture]
---

# TUI UI Landscape and Widget Map

## Purpose

Map Omegon's current native Ratatui UI landscape to the data it represents, the operator use cases it serves, and the Ratatui ecosystem widgets/components that best match each surface.

This document is a first pass after the `ui-surface-action-protocol` phase was decided. The action/surface seam means UI work can now be evaluated by semantic intent instead of only by local Ratatui implementation details.

## Current platform baseline

Omegon already uses a modern Ratatui stack:

| Crate | Current role | Current version evidence |
|---|---|---|
| `ratatui` | Core terminal UI framework | `0.30.0` |
| `ratatui-image` | Terminal image rendering | `11.0.2` |
| `ratatui-textarea` | Composer/editor widget | `0.9.1` |
| `tachyonfx` | Shader-like terminal effects | `0.25.0` |
| `ratatui-toaster` | Toast notifications | `0.1` |

Current useful third-party candidates from the Ratatui showcase / crates ecosystem:

| Crate | Fit summary | Notes |
|---|---|---|
| `tui-scrollview` | Scrollable detail panes and rich long-form content | Version `0.6.4`; Rust `1.88` requirement observed. Check Omegon MSRV before adopting. |
| `tui-widget-list` | Stateful/virtualized custom row lists | Good for tool streams, worker lists, queues; not for replacing conversation stream initially. |
| `tui-tree-widget` | Hierarchical sidebar/tree navigation | Already relevant to dashboard/design-tree surfaces. |
| `ratatui-image` | Image/artifact preview | Already used; supports Sixel/Kitty/iTerm2/halfblock backends. |
| `ratatui-textarea` | Text composition/editor | Already used; keep stable for now. |
| `tui-term` | Embedded PTY/process pane | Potential substrate experiment, not normal UI polish. |
| `tui-markdown` | Markdown-to-Ratatui text with optional highlighting | Candidate for assistant/system/detail rendering. |
| `ratatui-markdown` | Rich markdown, scroll, tree, preview, Mermaid/image features | Powerful but broader/heavier; evaluate carefully before adoption. |
| `rat-widget` / `rat-salsa` family | Menus, popups, scroll, tables, focus/event-loop patterns | Rich ecosystem; possible source of components, but avoid replacing Omegon event loop wholesale. |

## Conversation segment content model

Current concrete conversation segment variants:

```rust
pub enum SegmentContent {
    UserPrompt { text: String },
    AssistantText { text: String, thinking: String, complete: bool },
    ToolCard {
        id: String,
        name: String,
        args_summary: Option<String>,
        detail_args: Option<String>,
        result_summary: Option<String>,
        detail_result: Option<String>,
        is_error: bool,
        complete: bool,
        expanded: bool,
        live_partial: Option<Box<omegon_traits::PartialToolResult>>,
        started_at: Option<std::time::Instant>,
    },
    SystemNotification { text: String },
    LifecycleEvent { icon: String, text: String },
    Image { path: std::path::PathBuf, alt: String },
    TurnSeparator,
}
```

The segment stream is not one data type. It is a mixed event log, transcript, tool activity stream, artifact index, and lifecycle/status feed. That means a single generic list widget is unlikely to be the right abstraction for the whole conversation. Instead, the stream should remain custom/composite, while detail panes and sub-surfaces can use specialized widgets.

## Segment use-case map

### 1. `UserPrompt`

**Data represented**

- Operator-authored prompt text.
- May include rendered attachment summaries when image/non-image attachments are present.
- Metadata may include provider/model/timestamp/turn via `SegmentMeta`.

**Purpose**

- Anchor the conversational turn.
- Preserve operator intent for auditability and replay.
- Provide copy/export target.

**Best-fit components**

1. **Current custom segment component** — keep for inline transcript card; it already encodes Omegon-specific prompt chrome.
2. **`tui-markdown`** — candidate for rendering prompt bodies with markdown/code snippets in detail view.
3. **`tui-scrollview`** — candidate only for selected prompt detail pane when text is long.
4. **`ratatui-textarea`** — not for historical prompt rendering, but remains best fit for live composer input.

**First-pass recommendation**

Keep inline custom rendering. Add selected-segment detail rendering using existing primitives first; consider `tui-markdown` or `tui-scrollview` once detail pane proves useful.

### 2. `AssistantText`

**Data represented**

- Assistant output text.
- Hidden/secondary `thinking` text.
- Completion flag for streaming state.

**Purpose**

- Primary answer transcript.
- Streaming progress indicator.
- Copy/export and context recall.
- Potential split between visible answer and diagnostic reasoning/thinking surfaces.

**Best-fit components**

1. **`tui-markdown`** — strong candidate for assistant markdown/code rendering if current formatter is insufficient.
2. **`tui-scrollview`** — strong candidate for selected assistant detail pane, especially for long answers.
3. **`ratatui-markdown`** — candidate if we need richer markdown preview/tree/scroll features, but likely heavier than needed for first adoption.
4. **`tachyonfx`** — useful for streaming-complete pulse or subtle new-answer transition, not persistent decoration.

**First-pass recommendation**

Do not replace inline assistant rendering yet. Build a detail pane that can scroll long assistant output; later evaluate `tui-markdown` for better code/table rendering.

### 3. `ToolCard`

**Data represented**

- Stable tool-call id.
- Tool name.
- Args summary and detailed args.
- Result summary and detailed result.
- Error/completion state.
- Expanded visual state.
- Live partial/progress payload.
- Start timestamp for elapsed timer.

**Purpose**

- Operational observability: what the agent is doing.
- Auditability: exact tool args/results.
- Error diagnosis.
- Progress display for long-running actions.
- Detail/copy/open affordances.

**Best-fit components**

1. **`tui-scrollview`** — best candidate for full args/result detail pane. Tool results can be long and should scroll independently from the transcript.
2. **`tui-widget-list`** — candidate for active/recent tool streams, worker lists, and detail sections; less suitable for the mixed transcript itself.
3. **`tui-markdown`** — candidate for rendering structured markdown/code in detailed results.
4. **`tachyonfx`** — candidate for state transitions: started, completed, failed, selected/opened.
5. **`tui-term`** — only if the tool result is a live terminal/process stream; separate substrate decision.

**First-pass recommendation**

This is the highest-value immediate target. Keep inline tool cards compact. Move full args/result/live progress into a selected tool detail pane. Use no new dependency first, then consider `tui-scrollview` if local scrolling gets awkward or MSRV allows.

### 4. `SystemNotification`

**Data represented**

- Slash-command responses.
- Inline operator-facing info messages.
- Queue/status messages.
- Potentially merged consecutive notices.

**Purpose**

- Explain local state transitions and command results.
- Keep the operator oriented without polluting assistant transcript semantics.

**Best-fit components**

1. **Current custom notification component** — good for inline short notices.
2. **`ratatui-toaster`** — appropriate for transient notices that do not need transcript persistence.
3. **`tui-scrollview`** — possible for long command output detail, but tool/system distinction should be explicit.
4. **`tui-markdown`** — useful when command responses contain tables/code.

**First-pass recommendation**

Keep persistent notifications inline. Move truly transient UI-only messages toward toasts. Long system output should probably become a detail pane or tool-style result, not a giant inline notice.

### 5. `LifecycleEvent`

**Data represented**

- Icon + text for phase changes, decomposition, lifecycle milestones.

**Purpose**

- Show agent/runtime lifecycle progress.
- Provide timeline markers between operator prompts, assistant output, and tool activity.

**Best-fit components**

1. **Current lifecycle segment component** — likely sufficient inline.
2. **`tui-widget-list`** — candidate for a lifecycle/event history side panel.
3. **`tui-tree-widget`** — candidate when lifecycle events map to design-tree/OpenSpec hierarchy.
4. **`tachyonfx`** — subtle transition cue for phase changes.

**First-pass recommendation**

Keep inline lifecycle events compact. If lifecycle history grows, build a separate lifecycle/event panel rather than expanding inline segments.

### 6. `Image`

**Data represented**

- File path.
- Alt text.
- Renderable image artifact if terminal supports a graphics protocol.

**Purpose**

- Display screenshots, generated artifacts, diagrams, pasted images, and visual tool results.
- Provide artifact navigation/opening.

**Best-fit components**

1. **`ratatui-image`** — best fit and already present. Supports Sixel/Kitty/iTerm2/halfblock protocols.
2. **Current custom image placeholder component** — required fallback and metadata display.
3. **`tui-scrollview`** — useful for artifact detail pane with image + metadata + related text.
4. **External viewer/Bookokrat/Flynt surface** — better for rich documents/PDFs or complex image workflows.

**First-pass recommendation**

Do image work after the selected detail pane exists. Use `ratatui-image` inside detail pane with graceful fallback to path/metadata/open instructions.

### 7. `TurnSeparator`

**Data represented**

- Visual turn boundary only.

**Purpose**

- Chunk transcript by operator turns.
- Improve scanning and timeline comprehension.

**Best-fit components**

1. **Current separator component** — sufficient.
2. **`tachyonfx`** — not necessary except maybe subtle new-turn sweep.
3. **No third-party widget** — this is chrome, not data.

**First-pass recommendation**

Keep as custom minimal chrome.

## Cross-surface widget map

| Surface/use case | Primary data | Best 3-5 widget/component matches | Recommendation |
|---|---|---|---|
| Main conversation transcript | Mixed segments | Custom segment components, `tachyonfx`, built-in Ratatui `Block`/`Paragraph`/`Scrollbar` | Keep custom; do not replace with generic list. |
| Selected segment detail | One selected segment, long body | `tui-scrollview`, `tui-markdown`, custom detail component, `ratatui-image`, `tachyonfx` | Build v1 custom/no-dependency, then evaluate `tui-scrollview`. |
| Tool details/results | args/result/progress/errors | `tui-scrollview`, `tui-widget-list`, `tui-markdown`, `tachyonfx`, `tui-term` for live PTY only | Highest-value next UI target. |
| Active tool stream | current running tool(s) | `tui-widget-list`, current custom active-tool module, `throbber-widgets-tui`, `tachyonfx` | Consider list widget later; keep current for now. |
| Dashboard/design tree | hierarchical nodes/status | `tui-tree-widget`, `tui-tree-widget-table`, `tui-scrollview`, current dashboard widget | Already aligned; revisit after conversation detail. |
| Composer/editor | operator input | `ratatui-textarea`, current editor wrapper, `tui-menu` for completions | Keep stable; command palette later. |
| Command palette / selectors | options/actions | current selector, `tui-menu`, `rat-menu`, `tui-widget-list` | Later cleanup target. |
| Artifacts/images | paths, image data, metadata | `ratatui-image`, custom placeholder, `tui-scrollview`, external viewer | After detail pane. |
| Embedded process pane | PTY/process screen | `tui-term`, Cockpit/par-term alternatives | Separate substrate experiment, not immediate polish. |

## First-pass priorities

### Priority 1: selected segment detail pane

Implement a selected segment detail pane using existing Ratatui primitives first.

Why:

- Uses the new `SelectConversationSegment` / `OpenConversationSegmentDetail` semantic actions.
- Directly improves native UI.
- Creates a natural home for `ToolCard.detail_args` and `ToolCard.detail_result`.
- Lets us evaluate `tui-scrollview` on a bounded surface.

### Priority 2: tool-card detail renderer

Extract a dedicated detail component for tool cards.

The detail renderer should show:

- tool id/name
- status: running/complete/error
- args summary and full args
- result summary and full result
- live progress if present
- elapsed time

### Priority 3: markdown/detail rendering evaluation

Evaluate `tui-markdown` vs local rendering for assistant/tool detail bodies.

Known tradeoff:

- `tui-markdown` has Rust `1.86`, lighter and purpose-fit for markdown-to-text.
- `ratatui-markdown` is richer but broader/heavier and includes many optional features.

### Priority 4: artifact/image detail

Use `ratatui-image` in detail panes after the detail surface exists.

### Priority 5: embedded terminal/process pane experiment

Evaluate `tui-term` only under the existing pane-substrate design work. It is not part of normal conversation UI cleanup.

## Open questions

1. What is Omegon's current effective MSRV for releases? This gates `tui-scrollview` and `tui-big-text` adoption because both currently report Rust `1.88`.
2. Should selected segment detail be a bottom pane, right pane, overlay, or mode-specific focus view?
3. Should detail pane state be local Ratatui state, or should open detail target become shared semantic surface state?
4. Do we need stable segment IDs before external clients consume segment selection/detail, or are indices acceptable for native-only UI work?
5. Should `ToolCard.expanded` remain an inline visual flag, or should it be replaced by semantic detail-open state plus local compact/expanded rendering?

## Recommended next implementation

Build **Selected Segment Detail Pane v1** with no new dependency.

- Use current Ratatui primitives.
- Render only when a detail target is open.
- Populate from the selected/opened segment.
- Prefer tool-card detail first; fallback to text body for user/assistant/system/lifecycle/image.
- Keep focus traversal, scroll offset, mouse coordinates, and visual-only expansion frontend-local.
- After v1 lands, reassess `tui-scrollview` for the pane body.

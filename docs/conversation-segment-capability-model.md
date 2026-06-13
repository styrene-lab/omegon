+++
title = "Conversation Segment Capability Model"
tags = ["tui","conversation","segments","capabilities","architecture"]
+++

# Conversation Segment Capability Model

---
title: Conversation Segment Capability Model
status: implemented
tags: [tui, conversation, segments, capabilities, architecture]
---

# Conversation Segment Capability Model

## Overview

Define the primitive capabilities of Omegon conversation segments so future UI work can reason about segment behavior consistently instead of scattering special cases across `ConversationView`, `conv_widget`, `segments.rs`, and segment components.

This model builds on:

- [[tui-ui-landscape-widget-map]]
- [[tui-selected-segment-detail-pane]]
- the decided `ui-surface-action-protocol` phase

The conversation area is not a generic chat transcript. It is an operator event timeline composed of transcript, operations/audit, runtime/status, and artifact segments.

## Current segment variants

```rust
pub enum SegmentContent {
    UserPrompt { text: String },
    AssistantText { text: String, thinking: String, complete: bool },
    ToolCard { ... },
    SystemNotification { text: String },
    LifecycleEvent { icon: String, text: String },
    Image { path: PathBuf, alt: String },
    TurnSeparator,
}
```

## Capability vocabulary

| Capability | Meaning |
|---|---|
| `Selectable` | Operator can target this segment explicitly. |
| `FocusTraversable` | Segment participates in keyboard traversal. |
| `ToolFocusTraversable` | Segment participates in tool-card-only traversal. |
| `Copyable` | Segment can be exported/copied as text. |
| `DetailOpenable` | Segment can open a selected detail pane. |
| `StreamUpdatable` | Segment may receive incremental updates after creation. |
| `ProgressBearing` | Segment represents running/progress state. |
| `ArtifactBearing` | Segment points at a file/image/artifact. |
| `ErrorBearing` | Segment can represent an error state. |
| `ReplayRelevant` | Segment should appear in semantic replay/surface fixtures. |
| `ExternalDtoCandidate` | Segment should eventually have a stable external DTO shape. |
| `FrontendLocalOnly` | Segment is visual chrome and should not be externalized. |

## Capability matrix

| Segment | Selectable | FocusTraversable | ToolFocusTraversable | Copyable | DetailOpenable | StreamUpdatable | ProgressBearing | ArtifactBearing | ErrorBearing | ReplayRelevant | ExternalDtoCandidate | FrontendLocalOnly |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `UserPrompt` | yes | yes | no | yes | yes | no | no | indirect | no | yes | yes | no |
| `AssistantText` | yes | yes | no | yes | yes | yes | no | no | no | yes | yes | no |
| `ToolCard` | yes | yes | yes | yes | yes | yes | yes | maybe | yes | yes | yes | no |
| `SystemNotification` | yes | maybe | no | yes | maybe | sometimes | maybe | no | maybe | maybe | maybe | no |
| `LifecycleEvent` | yes | maybe | no | yes | maybe | no | maybe | no | maybe | yes | yes | no |
| `Image` | yes | yes | no | yes | yes | no | no | yes | maybe | yes | yes | no |
| `TurnSeparator` | no | no | no | minimal | no | no | no | no | no | no | no | yes |

## Segment-specific design notes

### `UserPrompt`

Operator-authored input. It is a transcript anchor and should remain selectable, copyable, detail-openable, replay-relevant, and externalizable.

Current caveat: attachments are not fully represented in this variant. Non-image attachments can be folded into text; image attachments become separate `Image` segments.

First-pass capability decision:

```text
UserPrompt = transcript segment; selectable/copyable/detail-openable; external DTO candidate.
```

### `AssistantText`

Assistant output with visible text, hidden/secondary thinking, and completion state.

It is stream-updatable while incomplete and should support detail rendering for long markdown/code/table bodies.

First-pass capability decision:

```text
AssistantText = transcript segment; stream-updatable; selectable/copyable/detail-openable; external DTO candidate.
```

### `ToolCard`

Operational/audit segment. It carries tool identity, arguments, result, status, live partial progress, errors, and elapsed-time support.

This is the richest segment type and should be treated as a first-class operation record, not merely a styled chat bubble.

First-pass capability decision:

```text
ToolCard = operation segment; selectable, tool-focus-traversable, detail-openable, progress/error-bearing, replay-relevant, external DTO candidate.
```

Open design pressure:

- `ToolCard.expanded` is visual/inline state.
- `pinned_segment` / detail pane target is semantic-ish detail state.
- These should remain distinct until we intentionally collapse them.

### `SystemNotification`

Local runtime/control notice. Currently broad untyped text.

Some system messages are replay-relevant; others are frontend-local toasts or mode hints. The raw segment variant cannot answer this alone.

First-pass capability decision:

```text
SystemNotification = local notice segment; selectable/copyable by default; externalization depends on classified subtype/projection.
```

Follow-up need:

Introduce a semantic projection or subtype classification for:

- slash command response
- queue state
- warning/error
- plan progress
- local mode/status hint

### `LifecycleEvent`

Timeline marker for lifecycle/decomposition/phase changes.

First-pass capability decision:

```text
LifecycleEvent = lifecycle marker; replay-relevant and external DTO candidate, but not necessarily focus-priority in the native UI.
```

Follow-up need:

Eventually add typed lifecycle kind/severity/phase references at the projection layer.

### `Image`

Artifact segment with path and alt text. Rendering is stateful because image display uses `ImageCache` and second-pass `ratatui-image` overlays.

First-pass capability decision:

```text
Image = artifact segment; selectable/copyable/detail-openable/replay-relevant/external DTO candidate.
```

Follow-up need:

Detail pane should become the preferred rich image/artifact inspection surface.

### `TurnSeparator`

Visual boundary only.

First-pass capability decision:

```text
TurnSeparator = frontend-local chrome; not selectable, not detail-openable, not externalized.
```

## Proposed Rust shape

Do not add this as a hard public API immediately. First implement it as an internal helper to consolidate current behavior.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentCapabilities {
    pub selectable: bool,
    pub focus_traversable: bool,
    pub tool_focus_traversable: bool,
    pub copyable: bool,
    pub detail_openable: bool,
    pub stream_updatable: bool,
    pub progress_bearing: bool,
    pub artifact_bearing: bool,
    pub error_bearing: bool,
    pub replay_relevant: bool,
    pub external_dto_candidate: bool,
    pub frontend_local_only: bool,
}

impl Segment {
    pub fn capabilities(&self) -> SegmentCapabilities {
        // derive from SegmentContent
    }
}
```

Suggested default helpers:

```rust
impl SegmentCapabilities {
    pub const fn selectable_timeline_item() -> Self { ... }
    pub const fn frontend_chrome() -> Self { ... }
}
```

## First implementation target

Add `SegmentCapabilities` and route the following current special cases through it:

1. `ConversationView::last_selectable_segment`
2. `ConversationView::first_selectable_segment`
3. `ConversationView::move_selected_segment_prev`
4. `ConversationView::move_selected_segment_next`
5. `App::handle_select_conversation_segment_action`
6. `App::handle_open_conversation_segment_detail_action`

Do **not** change behavior in the first pass. The first pass is a consolidation/refactor and should prove existing behavior is preserved.

## Acceptance criteria

- `TurnSeparator` is the only current segment that is not selectable/detail-openable.
- Existing segment selection tests still pass.
- Invalid segment selection still rejects.
- Detail-open action rejects non-detail-openable segments if such a segment is targeted.
- Tool-card focus traversal remains tool-card specific; do not conflate it with generic focus traversal.
- No external DTO/protocol change in this pass.

## Follow-up candidates

1. Classify `SystemNotification` subtypes.
2. Add stable segment identity independent of index.
3. Use capabilities in ACP/Flynt conversation DTO projection.
4. Use capabilities to drive UI hints: `copy`, `detail`, `open artifact`, `retry`, etc.
5. Use capabilities to decide which segments appear in replay fixtures.

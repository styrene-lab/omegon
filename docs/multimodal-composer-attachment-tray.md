+++
title = "Multimodal composer and attachment tray"
tags = ["ux","composer","attachments","multimodal","tui"]
+++

+++
id = "42e80870-03fb-40cd-ac1a-9694cbca47f3"
kind = "design_node"

[data]
title = "Multimodal composer and attachment tray"
status = "exploring"
issue_type = "feature"
priority = 2
parent = "multimodal-conversation-media"
dependencies = []
open_questions = []
+++

## Overview

# Multimodal composer and attachment tray

# Multimodal composer and attachment tray

## Overview

Replace image-only opaque editor tokens with a modality-neutral composer experience. The tray is the source of truth for pending attachments; inline references remain optional anchors inside text.

## Decisions

### Tray owns attachment state

Each attachment appears as a keyboard-focusable row/chip with kind, name, size, state, and compatibility. Removing an inline reference does not implicitly destroy an attachment unless the operator chooses removal; deleting an attachment removes its references.

### Paste feedback is typed and immediate

Clipboard intake returns a structured outcome: text-only, attachment-ready, unsupported media, helper unavailable, conversion failure, or I/O failure. Normal text paste remains quiet. Recognized non-text data creates a visible tray entry; failures produce a compact actionable status.

### Submission has a preflight gate

The send action checks source existence, detection, size, transform completion, and route compatibility. Failures focus the affected attachment and offer actions instead of silently dropping it.

### Keyboard-first actions

The tray supports focus-next/previous, preview/details, remove, reorder, transform, and route resolution without requiring a mouse. Screen-reader-friendly text projections describe icon-only state.

## Proposed projection

```rust
struct ComposerAttachmentProjection {
    id: AttachmentId,
    kind: AttachmentKind,
    label: String,
    metadata: Vec<String>,
    state: AttachmentStateProjection,
    route_compatibility: RouteCompatibility,
    actions: Vec<AttachmentActionProjection>,
}
```

Example:

```text
Attachments
  ▦ screenshot.png · PNG · 1.8 MB · ready
  ▤ report.pdf · 24 pages · extracting…
  ♫ meeting.m4a · 03:42 · transcription required
```

## File Scope

- `core/crates/omegon/src/surfaces/editor.rs` (modified) — tray projection and semantic actions.
- `core/crates/omegon/src/tui/editor.rs` (modified) — stable attachment references and focus behavior.
- `core/crates/omegon/src/tui/mod.rs` (modified) — paste outcomes, tray interaction, preflight flow.
- `core/crates/omegon/src/clipboard.rs` (modified) — typed intake outcomes and managed asset creation.
- `core/crates/omegon/src/settings.rs` (modified if needed) — size/retention defaults.
- `core/crates/omegon/src/tui/tests.rs` and editor tests (modified) — keyboard, failure, compatibility, and submission tests.

## Constraints

- Existing text editing, multiline paste, and Ctrl+V behavior must not regress.
- The operator must see whether every attachment is ready, transforming, unsupported, or failed.
- Empty text plus ready attachments is a valid prompt.
- Removing/reordering attachments must preserve text cursor correctness.
- Provider capability warnings must derive from canonical route capability data, not TUI allowlists.
- Surface projection, not Ratatui widgets, owns action semantics.

## Open Questions

- [assumption] A compact one-row tray is sufficient in Slim mode, with details in a focused overlay.
- Should attachments be automatically referenced in prompt text, or only represented structurally unless the operator inserts a reference?
- What key chord enters tray focus without conflicting with editor navigation and terminal selection?
- Should unsupported inputs default to a recommended transform or require an explicit operator choice?

## Open Questions

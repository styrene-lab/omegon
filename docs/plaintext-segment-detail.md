+++
title = "Plaintext Segment Detail Representation"
tags = ["architecture","tui","acp","session"]
+++

# Plaintext Segment Detail Representation

---
title: Plaintext Segment Detail Representation
status: exploring
tags: [architecture, tui, acp, session]
---

# Plaintext Segment Detail Representation

## Decision

Plaintext segment detail is a first-class session representation, not a copy side effect.

The system should be able to ask a session/conversation segment for a human-readable plaintext representation that is suitable for:

- TUI detail modals
- ACP/external clients
- transcript/export surfaces
- operator inspection, selection, highlighting, and manual copy

## Intent

Separate these concerns:

1. **Rendered transcript display** — rich-ish TUI/ACP surface rendering.
2. **Plaintext detail representation** — stable human-readable text for inspection and selection.
3. **Copy/export policy** — constrained text intended for direct clipboard/export side effects.
4. **Raw/debug representation** — structured or JSON-like payloads for diagnostics.

`/copy` opening a modal is semantically closer to "view plaintext detail" than "copy to clipboard". The converged model should make detail/view canonical and treat copy as a consumer of the same representation where appropriate.

## Implementation direction

Add a shared segment text representation function/API that can produce at least plaintext detail. The function should live outside TUI-only overlay rendering so ACP and future session surfaces can consume it.

Likely shape:

```rust
pub enum ConversationTextFormat {
    Plaintext,
    Markdown,
    Raw,
}

pub enum ConversationTextScope {
    Summary,
    Body,
    Detail,
    Full,
    Copy,
}

pub struct ConversationTextRequest {
    pub format: ConversationTextFormat,
    pub scope: ConversationTextScope,
}
```

The first implementation slice can be smaller: add a clearly named plaintext-detail function on the existing segment type, then wire consumers later.

## Detail behavior

- Entirely plaintext operations can show a modal containing plaintext copyable detail for the operator to peruse, select, and highlight.
- Specialized segment detail, especially tool output/detail with semi-bespoke structure, may continue to use bespoke detail renderers.
- Tool cards need care: plaintext detail can include tool name, status, args, and result, but a specialized tool result detail may still be the better primary view for some outputs.

## Current code anchors

Existing segment text helpers:

- `core/crates/omegon/src/tui/segments.rs`
  - `Segment::export_text(SegmentExportMode::Plaintext)` — full plaintext-ish segment representation.
  - `Segment::export_copy_text(SegmentExportMode::Plaintext)` — copy-policy-aware export; not the same as detail.

Recent modal bridge:

- `core/crates/omegon/src/tui/extension_overlays.rs` renders `{"kind":"text_copy","text":"..."}` as text-only modal body.
- `core/crates/omegon/src/tui/mod.rs` currently routes selected segment copy to that modal. This should be renamed/reframed as plaintext detail before wider wiring.

## Open questions

- Which segment kinds should prefer bespoke detail over plaintext detail?
- Should thinking text be included in plaintext detail, and under what visibility policy?
- Should ACP receive precomputed representations in segment DTOs, or request representations on demand?

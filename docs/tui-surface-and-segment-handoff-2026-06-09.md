+++
title = "TUI Surface and Segment Handoff — 2026-06-09"
tags = ["handoff","tui","ratatui","conversation","segments"]
+++

# TUI Surface and Segment Handoff — 2026-06-09

---
title: TUI Surface and Segment Handoff — 2026-06-09
tags: [handoff, tui, ratatui, conversation, segments]
---

# TUI Surface and Segment Handoff — 2026-06-09

## Session summary

This session advanced Omegon's native Ratatui UI architecture from projection/action scaffolding into concrete conversation-surface work.

Major outcomes:

1. UI surface/action protocol phase decided.
2. Ratatui `/ui` controls routed through semantic UI actions.
3. Conversation segment selection/detail actions added and wired to production focus/mouse paths.
4. Ratatui widget landscape/design docs written.
5. Selected Segment Detail Pane v1 implemented.
6. Conversation Segment Capability Model designed and first-pass implementation committed.

## Recent commits

```text
26b8ff13 refactor(tui): add segment capabilities
ae2fa0e5 feat(tui): add selected segment detail pane
18a1605a test(tui): cover selected segment detail pane
ebc4bd07 docs(tui): map ratatui widget landscape
90b1c027 docs(tui): decide ui surface action protocol phase
```

Other relevant earlier commits in the chain:

```text
fe4d91fe refactor(tui): route segment controls through actions
161855f3 feat(tui): add conversation segment actions
2f28c8ff feat(tui): route ui surface controls through actions
5cbfd241 docs(tui): mark surface replay harness implemented
56a8fd72 test(tui): add ui replay fixture builder
7229a668 feat(tui): add ui revision counter
c4a86c27 feat(tui): add ui runtime envelopes
4478a918 feat(tui): add semantic ui action seam
```

## Key design docs

- `docs/tui-surface-architecture.md`
- `docs/ui-surface-action-protocol.md`
- `docs/tui-ui-landscape-widget-map.md`
- `docs/tui-selected-segment-detail-pane.md`
- `docs/conversation-segment-capability-model.md`

## Implemented code surfaces

### UI runtime/action foundation

- `core/crates/omegon/src/ui_runtime/actions.rs`
- `core/crates/omegon/src/ui_runtime/envelope.rs`
- `core/crates/omegon/src/ui_runtime/revision.rs`
- `core/crates/omegon/src/ui_runtime/replay.rs`

Implemented semantic actions include:

- prompt submission
- continuation
- cancel active turn
- permission response
- operator wait response
- raw slash command
- UI preset/surface visibility
- conversation segment select/open-detail

### Selected segment detail pane

- `core/crates/omegon/src/tui/segment_detail.rs`
- `TuiLayoutInputs.segment_detail_height`
- `TuiLayoutPlan.segment_detail_area`
- render hook in `tui/mod.rs` when `conversation.timeline_expanded_segment()` is valid

V1 intentionally uses existing Ratatui primitives and no new dependency. `tui-scrollview` remains a later candidate after MSRV/release constraints are checked.

### Segment capabilities

- `SegmentCapabilities` added in `core/crates/omegon/src/tui/segments.rs`
- `Segment::capabilities()` added
- `ConversationView` selection traversal now uses `seg.capabilities().selectable`
- semantic select/detail actions reject non-selectable / non-detail-openable segments

Current capability stance:

- `TurnSeparator` is frontend-local chrome and not selectable/detail-openable.
- `ToolCard` is tool-focus/progress/error-capable.
- `AssistantText` is stream-updatable while incomplete.
- `Image` is artifact-bearing.
- `SystemNotification` remains selectable/copyable/detail-openable but is not an external DTO candidate in first pass.

## Validation run before latest commit

```text
cargo fmt
cargo test -p omegon ui_action_ -- --nocapture
cargo test -p omegon conversation::tests -- --nocapture
cargo check -p omegon
git diff --check
```

All passed before commit `26b8ff13`.

Earlier build for operator testing:

```text
just build
```

completed successfully after the selected detail pane v1 work.

## Operator observation

After build, operator tested slim mode and noted tool rows looked unchanged. That is expected for inline slim tool rows: the new work is mostly semantic routing plus a detail pane that appears only when a segment is opened/expanded. The UX affordance is still too hidden and should be improved later.

## Current state and next recommended work

Working tree should be clean after commit `26b8ff13`.

Next recommended design/implementation slice:

### Segment capability-driven UI hints and detail affordance discoverability

Use `SegmentCapabilities` to drive native UI hints and polish:

1. Show a visible hint when focused segment is detail-openable.
2. Make selected/opened segment state more visually distinct in slim/focus mode.
3. Avoid changing inline tool-card layout too much until the detail pane UX is proven.
4. Add tests around capability-derived behavior.

Alternative follow-up:

### SystemNotification subtype classification

`SystemNotification { text }` is currently too broad. Add projection-level subtype classification for:

- slash command response
- queue state
- warning/error
- plan progress
- local mode/status hint

This would improve replay/external DTO readiness without changing raw `SegmentContent` immediately.

## Important constraints

- Do not add `tui-scrollview` yet without checking Omegon's effective MSRV. Current crate metadata reported Rust `1.88` for `tui-scrollview` / `tui-big-text`.
- Keep main conversation stream custom; do not replace with a generic list widget.
- Keep scroll, viewport, traversal, hover, and animation frontend-local.
- Use semantic actions for operator intent: select, open detail, UI preset/surface visibility, cancel, submit, permission response.

---
id: tui-acp-conversation-projection-seam
title: "Unified Conversation Projection Seam for TUI and ACP"
status: implemented
tags: [tui, acp, projection, conversation]
open_questions:
  - "[assumption] TUI, ACP, and future client protocols should consume a shared semantic conversation projection rather than deriving segment meaning from raw SegmentContent independently."
dependencies: []
related: []
---

# Unified Conversation Projection Seam for TUI and ACP

## Overview

Define the shared semantic projection layer beneath TUI, ACP, and future client surfaces so conversation segment semantics are classified once and adapted separately for Ratatui rendering or protocol DTOs.

## Research

### Current extraction state

Implemented groundwork:
- `core/crates/omegon/src/tui/conversation_projection.rs` owns semantic segment roles, presentation classification, tool visual kind classification, and parameterized projection structs for user, assistant, tool, system, lifecycle, image, and separator segments.
- `core/crates/omegon/src/tui/conversation_render_projection.rs` owns Ratatui-facing render context and render/measure/metadata traits.
- `core/crates/omegon/src/tui/conv_widget.rs` now routes height/render/live/image checks through render projection traits rather than direct render calls and SegmentContent matches for those responsibilities.

Remaining coupling:
- `Segment::role()` and `Segment::presentation()` still live as methods on the concrete TUI/domain segment and should delegate through a semantic projection trait.
- `ToolVisualKind::color(&dyn Theme) -> Color` still ties semantic tool category to theme/color resolution; this is acceptable short-term but should move to the Ratatui render adapter once callers are ready.
- ACP has no semantic projection consumer yet; it should not consume Ratatui render traits.

## Decisions

### Introduce a shared semantic projection below all surfaces

**Status:** accepted

**Rationale:** TUI and ACP should be sibling adapters over a common conversation projection so segment roles, tool categories, completion/error state, and media/lifecycle semantics are classified once instead of reimplemented per surface.

### Keep Ratatui render projection separate from semantic projection

**Status:** accepted

**Rationale:** Ratatui concepts such as Buffer, Rect, render density, pinned state, terminal width, and theme resolution are surface-specific. They should live in a TUI render adapter so ACP can consume the same semantic projection without terminal rendering dependencies.

### Expose ACP through concrete protocol DTOs derived from semantic projections

**Status:** accepted

**Rationale:** The generic Rust projection structs are internal scaffolding, not a stable wire contract. ACP should receive concrete serializable DTOs/events derived from semantic projections after applying redaction, identity, revision, and capability policy.

## Open Questions

- [assumption] TUI, ACP, and future client protocols should consume a shared semantic conversation projection rather than deriving segment meaning from raw SegmentContent independently.

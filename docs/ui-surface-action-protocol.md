---
id: ui-surface-action-protocol
title: "UI Surface Action Protocol — make Ratatui one frontend projection"
status: decided
tags: [tui, architecture, surfaces, frontend, ratatui, acp, typescript, protocol]
open_questions:
  - "[assumption] The next decoupling stage should define a bidirectional surface/action contract rather than only adding more one-way semantic projections."
  - "[assumption] Ratatui should remain the reference frontend and should not be replaced or destabilized during protocol extraction."
  - "Which UI actions are core-semantic and portable across frontends, and which remain frontend-local state such as scroll, hover, viewport, and animation?"
  - "Should the first action contract target only conversation/editor/permission flow, or include dashboard/instruments/status from the beginning?"
  - "What envelope/versioning/revision model should external clients use for surface snapshots and action outcomes?"
  - "Where should the in-process Ratatui adapter boundary live: existing `tui` modules only, a new `ui_runtime`/`frontend` module, or a crate-level interface under `surfaces`?"
dependencies:
  - tui-acp-conversation-projection-seam
related:
  - tui-acp-conversation-projection-seam
  - tui-surface-substrate-reevaluation
---

# UI Surface Action Protocol — make Ratatui one frontend projection

## Overview

Define the next separation stage after TUI semantic surface extraction: treat the native Ratatui TUI as the reference in-process frontend over a shared semantic surface/action protocol, so ACP/Flynt/future TypeScript or web frontends can consume the same surface snapshots/events and submit the same bounded UI actions without depending on Ratatui internals.

## Research

### Current extraction baseline

Implemented groundwork already separates semantic surfaces from Ratatui rendering for conversation, footer, dashboard, editor, instruments, and layout under `core/crates/omegon/src/surfaces/`. ACP conversation DTO/update emission is now derived from semantic conversation projections in `core/crates/omegon/src/acp/surfaces.rs`. Ratatui rendering remains under `core/crates/omegon/src/tui`, with `layout_projection.rs` allocating `Rect`s, `conversation_render_projection.rs` mapping semantic concepts to terminal chrome, and `tui/segment_components/*` owning segment render bodies. See `docs/tui-surface-architecture.md`.

## Decisions

### Treat surfaces as outbound semantic state and actions as inbound semantic commands

**Status:** proposed

**Rationale:** The existing surface extraction is one-way: runtime/TUI state projects into semantic structures and ACP DTOs. A real alternate frontend requires the inverse path: bounded commands such as prompt submission, cancellation, permission response, mode selection, overlay open/close, and segment/tool selection. Keeping this command vocabulary semantic prevents external frontends from calling arbitrary TUI internals.

### Keep layout, theme, viewport, scroll, hover, and animation frontend-local

**Status:** proposed

**Rationale:** These concepts are renderer/substrate concerns. Exporting `Rect`, Ratatui style, terminal height, scroll offsets, or hover state would make future clients inherit Ratatui decisions and would recreate the coupling this change is meant to remove.

### Use Ratatui as the reference in-process frontend during extraction

**Status:** proposed

**Rationale:** The goal is frontend plurality, not a rewrite. The native TUI remains the production cockpit and validation oracle; new protocol seams must be introduced in ways that preserve current behavior and can be tested against existing Ratatui rendering paths.

### Start with conversation/editor/permission actions before broad dashboard controls

**Status:** proposed

**Rationale:** Conversation, input, cancellation, and permission responses are the minimum viable operator loop. Dashboard/instrument/status surfaces are mostly outbound observability and can consume the same envelope later without blocking the first actionable frontend contract.

### Serialize external frontend updates through versioned envelopes, not raw Rust structs

**Status:** proposed

**Rationale:** Internal semantic structs can evolve with Rust refactors. External clients need a stable DTO/envelope containing protocol version, session identity, surface name, revision, and payload. ACP already demonstrates this adapter pattern for conversation updates.

### Use a ui_runtime module for inbound frontend actions

**Status:** accepted

**Rationale:** Outbound semantic state remains under surfaces. Inbound operator commands need a distinct home so the contract can grow toward action handling, outcomes, replay, and later external DTOs without confusing surface projections with frontend commands.

### Validated bidirectional surface/action direction with internal action seam

**Status:** accepted

**Rationale:** Commit 4478a918 introduced the inbound UiAction/UiActionOutcome seam and routed initial Ratatui prompt, slash command, continuation, permission, and operator-wait paths through App::handle_ui_action while preserving behavior and passing cargo test -p omegon.

### Keep Ratatui as the reference frontend for subsequent extraction

**Status:** accepted

**Rationale:** The first implementation proved an in-process adapter seam can be added without replacing or destabilizing Ratatui. Ratatui remains the production cockpit and the behavioral reference for future external frontend adapters.

### Defer external wire envelopes until internal action semantics stabilize

**Status:** accepted

**Rationale:** The committed slice intentionally added Rust-native internal action types only. External ACP/Flynt/TS clients should consume versioned DTO/envelope adapters after the internal action vocabulary and replay expectations are clearer.

### Current-phase UI surface action protocol is sufficient for native UI work

**Status:** accepted

**Rationale:** The implemented slices cover outbound semantic surfaces, inbound `UiAction`, internal envelopes, replay fixtures, Ratatui `/ui` controls, and conversation segment selection/detail affordances. External TS/Flynt transport remains a future adapter problem rather than a blocker for native Ratatui UI work.

## Open Questions

- [assumption] The next decoupling stage should define a bidirectional surface/action contract rather than only adding more one-way semantic projections.
- [assumption] Ratatui should remain the reference frontend and should not be replaced or destabilized during protocol extraction.
- Which UI actions are core-semantic and portable across frontends, and which remain frontend-local state such as scroll, hover, viewport, and animation?
- Should the first action contract target only conversation/editor/permission flow, or include dashboard/instruments/status from the beginning?
- What envelope/versioning/revision model should external clients use for surface snapshots and action outcomes?
- Where should the in-process Ratatui adapter boundary live: existing `tui` modules only, a new `ui_runtime`/`frontend` module, or a crate-level interface under `surfaces`?

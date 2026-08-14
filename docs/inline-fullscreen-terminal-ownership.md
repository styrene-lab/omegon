---
id: inline-fullscreen-terminal-ownership
title: "Inline conversation and fullscreen interaction ownership"
status: decided
tags: []
open_questions: []
dependencies: []
related:
  - tui-full-display-navigation-shell
---

# Inline conversation and fullscreen interaction ownership

> Historical native-inline branch decision record. Current terminal ownership and
> bounded publication follow `tui-presentations.md` and
> `../openspec/changes/tui-dual-presentation/`. The implementation notes and
> acceptance labels below describe the original branch, not this rebased tree.

## Overview

Introduce a shell-level terminal presentation state machine that preserves the live agent/runtime event loop and canonical transcript publication state while rich menus temporarily own a 100% alternate-screen viewport. Fullscreen interaction is presentation-only: it must never pause, cancel, replace, or backpressure agent execution.

## Research

### Existing concurrency boundary

Current run_tui loop drains operator input, then drains AgentEvent traffic under an event-count and wall-time budget, then drains widget traffic and draws on a frame scheduler. App menus are only render state; opening active_menu does not stop coordinator work or event ingestion. Therefore fullscreen presentation can preserve background work if the same loop and channels remain alive and only the terminal backend/presentation target changes.

### Publication while fullscreen

Native transcript publication currently happens only in the draw path and advances TranscriptPublicationCursor when take_conversation_publications is called. Calling terminal insertion while on the alternate screen would publish into the wrong buffer. Leaving the cursor untouched is semantically safe and preserves exactly-once ordering, but deferred completed segments remain in canonical conversation memory until inline return. Existing conversation state is already authoritative; no second publication payload queue is needed for the first implementation.

## Decisions

### Fullscreen interaction is presentation-only

**Status:** accepted

**Rationale:** The agent coordinator, command channel, cancellation channel, AgentEvent receiver, widget receivers, scheduler, and App remain alive in the same run_tui loop. Entering alternate screen changes only terminal ownership and drawing; it must not await agent completion, replace receivers, suspend drains, or alter queue mode.

### Separate primary-screen inline and alternate-screen fullscreen terminals

**Status:** accepted

**Rationale:** Stock Ratatui retains an inline viewport's configured height and does not expose a safe viewport-kind transition. Conversation therefore retains a bounded primary-screen inline Terminal while rich interactions construct a distinct fullscreen Terminal after entering the alternate screen. The inline Terminal remains alive and dormant until primary-screen ownership returns.

### Defer native publication in canonical conversation

**Status:** accepted

**Rationale:** Native insert_before while alternate-screen ownership is active would target the disposable buffer. The TranscriptPublicationCursor remains unchanged during fullscreen interaction; canonical conversation is the sole deferred store. After successful return to inline ownership, eligible immutable records publish once in canonical order. No duplicate publication queue is introduced.

### Root interaction owns fullscreen across nested surfaces

**Status:** accepted

**Rationale:** Opening root navigation acquires fullscreen ownership. Nested selectors, prompts, inspectors, editors, and detail pages inherit that ownership through explicit return targets. Closing or cancelling a child restores its parent; only closing the root releases fullscreen ownership and restores inline conversation.

### Terminal modes use symmetric success-ordered tracking

**Status:** accepted

**Rationale:** Alternate-screen and fullscreen-only mouse capture are acquired and released at runtime. TerminalSessionGuard must expose symmetric tracked transitions. Tracking changes only after the corresponding crossterm command succeeds, so ordinary errors, panic restoration, and Drop always reflect modes Omegon actually owns.

### Menu paging derives from rendered list capacity

**Status:** accepted

**Rationale:** Fullscreen geometry changes the number of visible rows. Menu layout computes and records actual list capacity; Page Up/Down moves capacity minus one row, selection is restored by stable row identity, and offset is recomputed to reveal that identity after resize, filter, tab, or parent restoration.

### Passive interactions queue behind the active fullscreen owner

**Status:** accepted

**Rationale:** Extension modals, action-required payloads, and passive prompts arriving during fullscreen ownership cannot replace the active interaction. A bounded FIFO stores pending interaction descriptors, deduplicates replaceable updates by producer identity, and drops the oldest passive entry at capacity with an operator-visible diagnostic. Blocking permission/responder prompts are admitted through explicit priority and return-target policy rather than silent replacement.

### Publication cursor commits only after successful primary-screen insertion

**Status:** accepted

**Rationale:** Publication selection and cursor advancement must be transactional. Returning inline first restores primary-screen ownership, then projects eligible canonical records without committing cursor state, inserts them, and commits the cursor only if insertion succeeds. On failure, records remain eligible for retry and the loop returns the I/O error rather than silently losing or duplicating transcript history.

## Implementation Notes

### File Scope

- `core/crates/omegon/src/surfaces/navigation.rs` — Renderer-neutral interaction ownership, destination identity, return-target, and pending-interaction contracts. (create)
- `core/crates/omegon/src/surfaces/mod.rs` — Expose navigation contracts. (modify)
- `core/crates/omegon/src/tui/terminal_presentation.rs` — Inline/fullscreen presentation state machine and transition planning independent of App rendering. (create)
- `core/crates/omegon/src/tui/terminal_session.rs` — Symmetric alternate-screen and mouse-capture ownership tracking with rollback-safe tests. (modify)
- `core/crates/omegon/src/tui/navigation.rs` — Fullscreen root/nested interaction stack, return targets, pending passive queue, and identity restoration. (create)
- `core/crates/omegon/src/tui/mod.rs` — Integrate presentation transitions into run_tui without changing receiver drains; defer/commit native publication transactionally. (modify)
- `core/crates/omegon/src/tui/transcript_publication.rs` — Split publication inspection from cursor commit for retry-safe insertion. (modify)
- `core/crates/omegon/src/tui/render.rs` — Exclusive full-frame rendering dispatch. (modify)
- `core/crates/omegon/src/tui/menu_surface.rs` — Full-frame menu layout metrics and capacity-aware paging state. (modify)
- `core/crates/omegon/src/tui/menu_effects.rs` — Explicit navigation, child, Back, and return-target effects. (modify)
- `core/crates/omegon/src/tui/process_viewer.rs` — Route inspector through fullscreen ownership. (modify)
- `core/crates/omegon/src/tui/tests.rs` — TDD integration coverage for ownership, concurrency, publication, scrolling, and restoration. (modify)
- `core/crates/omegon/src/tui/snapshot_tests.rs` — Normal and constrained fullscreen navigation snapshots. (modify)

### Constraints

- Do not pause, await, cancel, replace, or backpressure the agent coordinator or existing input/AgentEvent/smoke/widget receiver drains during fullscreen interaction.
- Keep renderer-neutral destination and return-target identities under surfaces::*; terminal modes, Ratatui geometry, and key codes remain TUI-local.
- Keep the bounded primary-screen inline Terminal alive while fullscreen interaction owns a separate alternate-screen Terminal; never insert native transcript publications into the alternate screen.
- Make transcript publication transactional: inspect eligible canonical records without mutating the committed cursor, insert on the primary screen, then commit; insertion failure leaves records eligible.
- Track alternate-screen and fullscreen-only mouse capture symmetrically and only after successful terminal commands; panic and Drop restoration must reflect actual ownership.
- Root interaction ownership spans nested selectors, prompts, inspectors, and editors; only root close returns inline.
- Compute page movement from actual rendered list capacity with one-row overlap; restore selection by stable row identity and reveal it after resize/filter/tab/return.
- Bound pending passive interactions, deduplicate replaceable producer updates, preserve explicit priority for blocking responder prompts, and surface overflow rather than silently replacing active state.
- Every fullscreen renderer must paint the complete frame and exclusive input must not fall through to the composer.
- Tests must be written before production wiring and cover transition rollback, background event progress, nested ownership, deferred exactly-once publication, resize paging, repeated cycles, quit, and panic restoration.

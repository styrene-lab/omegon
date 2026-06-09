---
id: ui-surface-action-protocol-inventory
title: "Inventory portable UI actions vs frontend-local state"
status: exploring
parent: ui-surface-action-protocol
tags: [tui, actions, inventory]
open_questions: []
dependencies: []
related: []
---

# Inventory portable UI actions vs frontend-local state

## Overview

Inventory existing TUI input paths and distinguish portable semantic UI actions from frontend-local render/navigation state.

## Research

### First implementation evidence

Commit 4478a918 established the initial action seam. Portable actions already routed through `App::handle_ui_action`: prompt submission, empty-enter continuation, raw slash command dispatch, permission response, and operator-wait response. Remaining direct/local paths include queue-mode interrupt inside `submit_prefixed_prompt`, global conversation scroll/pin controls, selector popup navigation, secret-input editing, reverse search, tutorial overlay navigation, dashboard key handling, terminal copy mode, mouse hit testing, and layout/render state.

## Decisions

### Inventory remaining TUI interactions before expanding UiAction

**Status:** accepted

**Rationale:** The first slice showed the seam works for a small operator loop. Expanding actions without classifying the rest of the event loop risks protocolizing frontend-local behavior such as scroll, selector cursor movement, tutorial navigation, or terminal input mechanics.

## Interaction inventory

### Already portable through `UiAction`

| Interaction | Current path | Classification | Notes |
|---|---|---|---|
| Prompt submission | `submit_editor_buffer` → `UiAction::SubmitPrompt` | Portable semantic action | The frontend supplies text/attachments/source; core applies queue/prefix behavior. |
| Empty continuation | empty Enter when `awaiting_continuation` | Portable semantic action | Current action is `SubmitContinuation`; later DTO can expose this as a typed command. |
| Raw slash command | editor text starting with `/` | Portable semantic action | Currently raw-string `RunSlashCommand`; stable commands can become typed later. |
| Permission response | pending permission lane keys | Portable semantic action | `PermissionResponse` is already domain-level. |
| Operator wait response | pending manual wait keys | Portable semantic action | `OperatorWaitResponse` is already domain-level. |

### Portable candidates for next slice

| Interaction | Current path | Candidate action | Reason |
|---|---|---|---|
| Explicit cancel/interrupt | Esc/Ctrl+C branches and queue-mode interrupt | `CancelActiveTurn` cleanup | The action exists, but some in-process branches still call `interrupt()` directly to avoid async recursion. Split prompt submission helpers before routing all cancel paths. |
| UI preset/surface visibility | `apply_ui_preset`, `toggle_ui_surface` | `SetUiPreset`, `SetSurfaceVisible` | Portable if it expresses durable operator intent; frontend-local if only a local layout toggle. Needs preference/persistence decision. |
| Tool card expansion/collapse all | focus-mode and tool-card keyboard branches | `SetToolCardExpansion` or local only | Portable only if expansion affects shared semantic affordances or replay; otherwise local viewport state. |
| Conversation segment selection/open detail | mouse hit-test and focus-mode Enter | `SelectSegment`, `OpenSegmentDetail` | Candidate because future frontends need stable segment affordances, but scroll/focus mechanics remain local. |
| Attachment insertion | paste/image and Tab path expansion | `AttachInputArtifact` | Portable if external clients can reference artifacts by URI/path/capability; local editor mechanics remain frontend-owned. |

### Keep frontend-local for now

| Interaction | Current path | Classification | Reason |
|---|---|---|---|
| Conversation scroll/pin/viewport | PageUp/PageDown/Home/End, mouse wheel, Ctrl+O | Frontend-local | Scroll offset, pinning-in-viewport, and local viewport height are rendering concerns. |
| Selector popup navigation | `selector.move_up/down`, confirm/cancel | Frontend-local shell over semantic command | The selected outcome may produce a command, but cursor movement and popup lifecycle are local. |
| Secret input editing | `EditorMode::SecretInput` | Frontend-local input mode | Hidden buffer editing is terminal/frontend mechanics; final `SecretsSet` control request is the semantic operation. |
| Reverse search | `EditorMode::ReverseSearch` | Frontend-local editor feature | History search UI is a local composer affordance. |
| Tutorial overlay navigation | `tutorial_overlay` Tab/BackTab/choice handling | Frontend-local onboarding UX | The tutorial may emit prompts, but overlay navigation is not a core action contract. |
| Dashboard key handling | `dashboard.handle_key` | Frontend-local until dashboard actions are decomposed | Dashboard currently owns its own selector/focus model. Inventory separately before protocolizing. |
| Terminal copy mode / mouse capture | `terminal_copy_mode`, `set_mouse_capture` | Frontend-local terminal substrate | Terminal capabilities should not leak into core UI protocol. |
| Text editing keystrokes | Ctrl-W/U/K/Y, Alt-B/F/D, arrows, character insert | Frontend-local editor mechanics | External frontends own their own composer widget. |

## Next implementation target

The next code slice should be the non-recursive cancel/action cleanup:

1. Split prompt submission into action handling and lower-level non-recursive helpers.
2. Route top-level Esc/Ctrl+C active-turn cancellation through `UiAction::CancelActiveTurn`.
3. Keep editor-clear and double-Ctrl+C quit semantics Ratatui-local.
4. Add tests for `UiAction::CancelActiveTurn` accepted/noop outcomes.

This tightens the already-introduced action seam without expanding the protocol vocabulary prematurely.

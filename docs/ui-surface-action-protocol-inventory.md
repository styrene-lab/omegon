---
id: ui-surface-action-protocol-inventory
title: "Inventory portable UI actions vs frontend-local state"
status: decided
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

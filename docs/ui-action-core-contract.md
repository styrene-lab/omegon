---
id: ui-action-core-contract
title: "Core UI action contract"
status: implementing
parent: ui-surface-action-protocol
tags: [surfaces, actions, api]
open_questions: []
dependencies: []
related: []
---

# Core UI action contract

## Overview

Introduce a bounded UiAction/UiActionOutcome contract for portable operator commands such as prompt submission, cancellation, permission decisions, surface mode selection, overlays, and selection/open-detail affordances.

## Decisions

### Implement initial internal action contract before external protocol DTOs

**Status:** accepted

**Rationale:** The first slice proves Ratatui can emit semantic actions in-process without changing visible behavior. External clients will be added later through versioned DTO/envelope adapters instead of exposing raw Rust enums.

### Route the minimum operator loop through App::handle_ui_action

**Status:** accepted

**Rationale:** Prompt submission, continuation, slash commands, permission responses, and operator-wait responses are the smallest useful bidirectional frontend seam. Dashboard, selector, tutorial, mouse, and layout behavior remain Ratatui-local for now.

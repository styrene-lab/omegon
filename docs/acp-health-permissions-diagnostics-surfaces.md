---
id: acp-health-permissions-diagnostics-surfaces
title: "ACP health, permissions, and diagnostics surfaces"
status: deferred
tags: [acp, health, permissions, diagnostics, issue-132-followup]
open_questions:
  - "[assumption] Diagnostic history can be bounded and redacted before exposure to ACP clients."
  - "[assumption] Permission state can be represented without exposing host-specific implementation details that clients cannot act on."
dependencies:
  - acp-132-0-26-9-completion
related:
  - docs/acp-132-runtime-observability-extension-control.md
---

# ACP health, permissions, and diagnostics surfaces

## Overview

This follow-up owns the P2 issue #132 diagnostic surfaces for reconnecting clients and operator dashboards. It turns transient runtime failures into structured, bounded, redacted status.

## Scope

- `_runtime/health`
  - checks with `ok | warning | degraded | blocked | unknown`
  - provider/auth health
  - extension health
  - memory/store health
  - package/substrate health

- `_permissions/status`
  - current permission mode and host-action mediation state
  - whether approvals are available
  - policy-denied vs user-denied vs unavailable distinctions

- `_diagnostics/recent`
  - bounded recent structured events/errors
  - reconnect-safe buffer
  - redacted payloads

- `_errors/last`
  - last actionable error per subsystem
  - code, message, subsystem, recommended action

## Non-goals

- No raw log streaming.
- No prompt/tool argument transcript exposure.
- No unredacted exception stacks to UI clients.

## Design direction

Diagnostics should be actionable and bounded. The UI should be able to say "blocked by permission policy" or "provider auth expired" without scraping logs. All surfaces should share a stable subsystem/code vocabulary.

## Acceptance criteria

- ACP client can render a health dashboard without invoking mutating operations.
- ACP client can distinguish policy-denied, user-denied, unavailable, degraded, and unknown states.
- Reconnecting clients can retrieve recent structured failures without log scraping.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

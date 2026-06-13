---
id: acp-128-session-cancel
title: "Use standard ACP session/cancel for scoped turn cancellation"
status: deferred
parent: acp-128-turn-control-telemetry
tags: [acp, cancellation, issue-128]
open_questions: []
dependencies: []
related: []
---

# Use standard ACP session/cancel for scoped turn cancellation

## Overview

Strengthen and test the existing ACP CancelNotification path so Flynt can cancel the active turn without killing transport. Validate session id, stop provider retry loops via worker cancellation token, interrupt active tools where safe, return StopReason::Cancelled for the prompt, and emit a structured cancellation terminal event.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

---
id: acp-128-session-cancel
title: "Use standard ACP session/cancel for scoped turn cancellation"
status: exploring
parent: acp-128-turn-control-telemetry
tags: [acp, cancellation, issue-128]
open_questions: []
dependencies: []
related: []
---

# Use standard ACP session/cancel for scoped turn cancellation

## Overview

Strengthen and test the existing ACP CancelNotification path so Flynt can cancel the active turn without killing transport. Validate session id, stop provider retry loops via worker cancellation token, interrupt active tools where safe, return StopReason::Cancelled for the prompt, and emit a structured cancellation terminal event.

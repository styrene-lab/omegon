---
id: acp-128-turn-control-telemetry
title: "Issue 128: ACP turn cancellation and provider telemetry"
status: deferred
tags: [acp, issue-128, flynt, provider-telemetry, turn-control]
open_questions: []
dependencies: []
related: []
---

# Issue 128: ACP turn cancellation and provider telemetry

## Overview

Complete upstream issue #128 by separating provider/system telemetry from assistant-authored content and exposing scoped turn cancellation over ACP. Use standard ACP session/cancel for cancellation and ACP ExtNotification events for provider retry/failure/turn-cancelled telemetry.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

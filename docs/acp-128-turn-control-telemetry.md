---
id: acp-128-turn-control-telemetry
title: "Issue 128: ACP turn cancellation and provider telemetry"
status: exploring
tags: [acp, issue-128, flynt, provider-telemetry, turn-control]
open_questions: []
dependencies: []
related: []
---

# Issue 128: ACP turn cancellation and provider telemetry

## Overview

Complete upstream issue #128 by separating provider/system telemetry from assistant-authored content and exposing scoped turn cancellation over ACP. Use standard ACP session/cancel for cancellation and ACP ExtNotification events for provider retry/failure/turn-cancelled telemetry.

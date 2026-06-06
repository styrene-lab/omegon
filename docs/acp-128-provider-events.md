---
id: acp-128-provider-events
title: "Emit provider retry/failure as ACP extension notifications"
status: exploring
parent: acp-128-turn-control-telemetry
tags: [acp, provider-telemetry, issue-128]
open_questions: []
dependencies: []
related: []
---

# Emit provider retry/failure as ACP extension notifications

## Overview

Replace provider retry/failure assistant-text spam with structured ACP ExtNotification events such as _provider/retry and _provider/failure carrying provider, model, attempt, delay, reason, message, and recovery hints.

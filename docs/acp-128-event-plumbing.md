---
id: acp-128-event-plumbing
title: "Broaden ACP event plumbing for system events"
status: exploring
parent: acp-128-turn-control-telemetry
tags: [acp, worker-events, issue-128]
open_questions: []
dependencies: []
related: []
---

# Broaden ACP event plumbing for system events

## Overview

Add typed WorkerEvent variants for provider retry, provider failure, and turn cancelled; forward them through ACP as extension notifications; keep ordinary StatusUpdate for human-facing non-model status only.

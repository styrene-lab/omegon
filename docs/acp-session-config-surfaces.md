---
id: acp-session-config-surfaces
title: "ACP session and config observability surfaces"
status: deferred
tags: [acp, session, config, runtime-observability, issue-132-followup]
open_questions:
  - "[assumption] Session state can be exposed without leaking prompt content or tool arguments."
  - "[assumption] Structured config set/get should share validation with existing ACP config option handling."
dependencies:
  - acp-132-0-26-9-completion
related:
  - docs/acp-132-runtime-observability-extension-control.md
---

# ACP session and config observability surfaces

## Overview

This follow-up owns the P1 issue #132 surfaces that let clients inspect and modify active session state without scraping chat events or waiting for transient `ConfigChanged` notifications.

## Scope

- `_session/status`
  - active session id/cwd
  - turn active/queued/last_error
  - current model/thinking/posture
  - cancellation state where relevant

- `_session/config`
  - list current config options and values
  - set values through a structured request/response surface
  - reuse existing validation and `SetSessionConfigOptionRequest` behavior

## Non-goals

- No provider auth probing; owned by provider/runtime surfaces.
- No full diagnostic event log; owned by diagnostics surfaces.
- No prompt/tool transcript exposure.

## Design direction

Expose session control-plane state, not conversation content. Responses should be safe for UI dashboards and reconnecting clients. Values that can alter runtime behavior should use the same validation path as current ACP session config option setting.

## Acceptance criteria

- Client can reconnect and ask `_session/status` to determine whether a turn is active, cancelled, queued, or idle.
- Client can ask `_session/config` for the current model/thinking/posture and available options.
- Client can set supported config values and receive structured success/error results.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

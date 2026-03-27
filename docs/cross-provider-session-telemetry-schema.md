---
id: cross-provider-session-telemetry-schema
title: Cross-provider session telemetry schema for replay and inspection
status: seed
parent: sovereign-multi-repo-project-management
tags: [telemetry, observability, sessions, providers, dashboard]
open_questions: []
jj_change_id: zvvzqkmprokmkwnvwzmmknvptqrnwlqp
issue_type: feature
priority: 1
---

# Cross-provider session telemetry schema for replay and inspection

## Overview

Define a provider-agnostic session/event log schema rich enough to support a claude-devtools-class inspector for Omegon across Anthropic, OpenAI-compatible providers, Codex, and local models. The schema should preserve replayability, token/cost/quota attribution, tool execution detail, context composition, model/provider switching, and subagent/cleave trees without binding the format to any single upstream provider's transcript structure.

## Open Questions

*No open questions.*

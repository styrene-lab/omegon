---
id: assistant-run-substrate
title: "Assistant Run Substrate"
status: decided
tags: [console, backend, assistant, runs, security]
open_questions: []
dependencies: []
related: []
---

# Assistant Run Substrate

## Overview

Define a backend-owned assistant run/readiness substrate for the console. The substrate should expose durable run summaries and safe progress metadata for assistant executions without leaking prompts, secrets, or environment payloads.

## Decisions

### Use secret-safe run summaries

**Status:** proposed

**Rationale:** Run summaries should carry status, assistant identity, trigger/source, timestamps, readiness status, and redacted progress only. Raw prompts, environment variables, secret recipe payloads, and unredacted logs must stay out of default projections.

### Introduce assistant runs as a sibling backend domain

**Status:** accepted

**Rationale:** Assistant runs have different security and lifecycle semantics than plan/task projections. Keeping them in a separate backend domain avoids overloading plan task identity and lets console clients consume assistant execution state directly.

### Start with runtime-only projections

**Status:** accepted

**Rationale:** A runtime-only read model is the smallest useful slice for console integration. It avoids committing to storage format before launch/control semantics are stable while still giving HTTP and ACP consumers a stable DTO contract.

### Use explicit console launch events as first run source

**Status:** accepted

**Rationale:** The first assistant-run substrate should model runs launched through future console/backend control surfaces. Existing ACP sessions, TUI commands, and plan/task bindings can be related later without conflating their current lifecycles.

### Assistant runs must support terminal blocked status

**Status:** accepted

**Rationale:** Long non-interactive assignments must have an explicit terminal outcome for work that cannot proceed without external input. The run contract must distinguish completed from blocked so agents do not end with conversational readiness text like 'ready to begin' while the orchestrator waits indefinitely.

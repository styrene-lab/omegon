---
id: acp-plan-task-revision-events
title: "ACP plan/task revisions and event cursors"
status: deferred
tags: [acp, events, revisions, tasks, ecosystem]
parent: plan-refinement
related: [acp-task-durability-contract, external-work-surface-integration, acp-ecosystem-capability-negotiation]
open_questions:
  - "[assumption] Polling clients need cheap revision cursors before a durable push/event stream is available."
  - "What is the minimal revision token for file-backed OpenSpec/design projections: mtime+hash, content hash, git blob id, or lifecycle FSM revision?"
  - "How should session-local events compose with repo-durable revision streams?"
  - "Should revisions be per task, per plan, per source artifact, or all three?"
---

# ACP plan/task revisions and event cursors

## Overview

Define revision and event cursor semantics for ACP plan/task consumers. Durable binding is unsafe without a way for clients to detect stale projections, changed backing artifacts, and conflicting mutations. Flynt can poll today, but polling is only safe if task and plan responses expose revision tokens that change when the backing source changes.

This node is the event/revision companion to [[acp-task-durability-contract]].

## Research

### Current state

`_plans/events` and `_tasks/events` currently advertise session-local/read-only event surfaces, but they do not provide durable cursors. Plan/task projections can be rebuilt from OpenSpec/design/session sources, but consumers cannot yet distinguish unchanged data from a newly rebuilt projection with different backing artifacts.

Flynt's `docs/omegon-plan-task-acp-mapping.md` accepts either real event streams with cursors or polling with revision comparison. That means Omegon can ship polling-safe revisions before implementing durable push streams.

### Required event model

A minimum polling response should include:

```json
{
  "revision": "sha256:...",
  "events": [],
  "durability": "session|repo|none"
}
```

Task and plan objects should include their own `revision` fields so clients can cache and compare rows independently.

Suggested task event shape:

```json
{
  "cursor": "...",
  "events": [
    {
      "type": "task.updated",
      "task_id": "...",
      "stable_id": "...",
      "revision": "sha256:...",
      "changed_fields": ["status", "evidence"]
    }
  ]
}
```

### Pagination and filtering pressure

Large repos cannot rely on unfiltered full-list polling forever. Revision support should pair with eventual `_tasks/list` filters:

```json
{
  "plan_id": "...",
  "source": "openspec|design|hybrid",
  "status": "pending|in_progress|done",
  "limit": 100,
  "cursor": "..."
}
```

## Decisions

### Use opaque revision tokens at the ACP boundary

**Status:** proposed

**Rationale:** ACP clients should not rely on whether Omegon uses content hashes, git object ids, mtime tuples, or lifecycle counters internally. The token only needs equality/change semantics at the boundary.

### Support polling before durable streams

**Status:** proposed

**Rationale:** Flynt and similar tools can integrate through cheap polling if revisions are explicit. Durable push streams can be added later without changing the read model.

### Separate repo revisions from session events

**Status:** proposed

**Rationale:** Repo-backed source changes and session-local view events have different lifetimes. Combining them into one cursor would make clients over-trust volatile session state.

## Open Questions

- [assumption] Polling clients need cheap revision cursors before a durable push/event stream is available.
- What is the minimal revision token for file-backed OpenSpec/design projections: mtime+hash, content hash, git blob id, or lifecycle FSM revision?
- How should session-local events compose with repo-durable revision streams?
- Should revisions be per task, per plan, per source artifact, or all three?

## Implementation Notes

Primary files:

- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/tools/mod.rs`
- `core/crates/omegon/src/acp.rs`
- `core/crates/omegon/src/web/ws.rs`

Implementation targets:

- Add opaque `revision` to plan and task projection ACP JSON.
- Add source-artifact revision computation for OpenSpec/design projections.
- Add `since_revision` request handling for `_plans/events` and `_tasks/events` if cheap; otherwise explicitly return current revision and empty events.
- Add pagination/filtering request fields for `_tasks/list` once revision semantics are in place.
- Add tests that revisions change when backing `tasks.md` or design node content changes.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

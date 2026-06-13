---
id: acp-task-status-error-pagination-contract
title: "ACP task status, errors, filtering, and pagination"
status: deferred
tags: [acp, tasks, errors, pagination, status, ecosystem]
parent: acp-ecosystem-capability-negotiation
related: [acp-task-mutation-contract, acp-plan-task-revision-events]
open_questions:
  - "[assumption] Large repositories will need pagination before direct sync is broadly useful."
  - "Should status mapping expose both canonical status and source-native status detail?"
  - "What cursor format is stable enough for filtered task listings?"
---

# ACP task status, errors, filtering, and pagination

## Overview

Define the non-glamorous but necessary ACP details that let external work surfaces scale beyond demos: status mapping, structured errors, filters, pagination, and conservative client behavior for unknown states.

## Research

Flynt's mapping contract calls out status mapping, pagination/filtering, and structured errors as prerequisites for direct bidirectional task mapping.

Conservative status mapping:

| Omegon | External presentation |
|---|---|
| `pending` | todo |
| `in_progress` | in_progress |
| `done` | done |
| `blocked` | todo plus blocked detail/tag |
| `skipped` / `deferred` | archived/deferred presentation, never automatic delete |

Structured error baseline:

```json
{
  "accepted": false,
  "code": "not_found|stale_revision|not_writable|unsupported_source|conflict",
  "error": "human readable detail"
}
```

## Decisions

### Expose raw and canonical status

**Status:** proposed

**Rationale:** External clients can map common states while preserving source-specific detail for display and debugging.

### Make errors machine-readable first

**Status:** proposed

**Rationale:** Clients need stable error codes to decide between refresh, retry, conflict UI, or disabled controls.

## Open Questions

- [assumption] Large repositories will need pagination before direct sync is broadly useful.
- Should status mapping expose both canonical status and source-native status detail?
- What cursor format is stable enough for filtered task listings?

## Implementation Notes

Primary files:

- `core/crates/omegon/src/acp.rs`
- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/tools/mod.rs`

Implementation targets:

- Document and expose canonical `WorkItemStatus` mapping.
- Add structured error helper for ACP plan/task surfaces.
- Add `_tasks/list` filters: `plan_id`, `source`, `status`, `limit`, `cursor`.
- Add tests for unknown status display and cursor/filter behavior.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

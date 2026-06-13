---
id: acp-ecosystem-capability-negotiation
title: "ACP ecosystem capability negotiation"
status: deferred
tags: [acp, ecosystem, capabilities, compatibility]
parent: plan-refinement
related: [acp-task-durability-contract, acp-plan-task-revision-events, external-work-surface-integration, acp-task-status-error-pagination-contract]
open_questions:
  - "[assumption] ACP clients will span multiple protocol/client versions and need machine-readable compatibility modes."
  - "How granular should capability advertisement be: per method, per field, per mutation, or per source type?"
  - "Should compatibility modes be global (`read_only + manual_link`) or negotiated per plan/task source?"
  - "How should deprecations and future stronger contracts be advertised without breaking older clients?"
---

# ACP ecosystem capability negotiation

## Overview

Define how Omegon advertises plan/task capabilities to Flynt and future ACP clients. The plan/task boundary is version-sensitive: Flynt 0.12.x should remain in `read_only + manual_link` mode, while future clients may support durable bindings, revision-aware polling, and explicit mutations.

This node turns implicit behavior into machine-readable compatibility negotiation.

## Research

### Current capability surface

`_runtime/capabilities` already advertises `_plans/*` and `_tasks/*` surfaces with version numbers. It does not yet describe durability levels, mutation safety, revision support, structured error codes, pagination/filtering support, or client compatibility modes.

### Needed compatibility levels

Proposed modes:

- `read_only`: client may poll/render plans and tasks.
- `manual_link`: client may create local links but must not treat them as authoritative.
- `session_bind`: client may request session-local external bindings.
- `repo_bind`: client may request durable repo bindings with revision preconditions.
- `mutate`: client may request explicit lifecycle/task mutations listed in `supported_mutations`.

Flynt 0.12.x should stay at:

```text
read_only + manual_link
```

until task durability and revision nodes are implemented.

### Structured errors

Flynt's mapping doc requires machine-readable errors before direct mapping:

```json
{
  "accepted": false,
  "code": "not_found|stale_revision|not_writable|unsupported_source|conflict",
  "error": "human readable detail"
}
```

This should be advertised as a capability, not inferred from response shape.

## Decisions

### Advertise compatibility modes explicitly

**Status:** proposed

**Rationale:** Clients should not infer safety from method existence. `_tasks/bind` existing does not mean it is repo-durable or authoritative.

### Negotiate capabilities per source type

**Status:** proposed

**Rationale:** Session tasks, OpenSpec tasks, design questions, validation tasks, and external issue tasks may support different mutation and durability levels.

### Keep older clients safe by default

**Status:** proposed

**Rationale:** If a client does not understand durability/revision fields, the server should expose conservative behavior and clear warnings rather than optimistic bidirectional semantics.

### Treat structured errors as part of the contract

**Status:** proposed

**Rationale:** UI clients need stable error codes to choose between retry, refresh, disable mutation UI, or present a conflict-resolution flow.

## Open Questions

- [assumption] ACP clients will span multiple protocol/client versions and need machine-readable compatibility modes.
- How granular should capability advertisement be: per method, per field, per mutation, or per source type?
- Should compatibility modes be global (`read_only + manual_link`) or negotiated per plan/task source?
- How should deprecations and future stronger contracts be advertised without breaking older clients?

## Implementation Notes

Primary files:

- `core/crates/omegon/src/acp.rs`
- `docs/acp-surface.md`
- `docs/flynt-integration.md`

Implementation targets:

- Extend `_runtime/capabilities` with plan/task compatibility modes.
- Include source-specific mutation capability hints.
- Add field-level capability indicators for `stable_id`, `revision`, `durability`, `supported_mutations`, `pagination`, `filtering`, and `structured_errors`.
- Add tests that Flynt-era compatibility returns `read_only + manual_link` until stronger contracts are present.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

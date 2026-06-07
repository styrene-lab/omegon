---
id: acp-task-durability-contract
title: "ACP task durability and binding contract"
status: exploring
tags: [acp, tasks, flynt, durability, ecosystem]
parent: plan-refinement
related: [acp-plan-task-revision-events, external-work-surface-integration, acp-ecosystem-capability-negotiation]
open_questions:
  - "[assumption] Flynt and future ACP clients need task bindings that survive projection rebuilds, process restarts, and repository movement."
  - "Which binding store is authoritative for repo-durable external task refs: OpenSpec sidecar, design-node frontmatter, .omegon repo metadata, or a dedicated binding index?"
  - "What conflict policy should apply when an external task ref points to a task projection whose source task disappeared or changed identity?"
  - "Should session-durable bindings be exposed to external clients, or should ACP only accept repo-durable bindings for non-local ecosystems?"
---

# ACP task durability and binding contract

## Overview

Define the missing durability contract between Omegon task projections and external work surfaces such as Flynt task boards. The current ACP task surface is sufficient for read-only display and manual local linking, but not for authoritative bidirectional mapping. This node defines the stronger contract needed before Flynt or another ACP client can treat task mappings as durable coordination state.

The core boundary is: Omegon remains authoritative for lifecycle task completion when the backing source is OpenSpec, design-tree, session plan, branch/worktree, or validation evidence. External systems may attach coordination references, but they do not become the lifecycle source of truth unless an explicit mutation contract says so.

## Research

### Current implementation baseline

Plan/task ACP surfaces exist via `_plans/*` and `_tasks/*`. Task projections include ids, parent plan ids, labels, status, intent, completion policy, evidence refs, external task refs, and writable flags. `_tasks/bind` currently records/control-routes a binding intent, but it does not yet return a durable binding envelope with revision, source, and durability semantics.

### Flynt 0.12.x safe mode

Flynt's `docs/omegon-plan-task-acp-mapping.md` defines the safe 0.12.x posture as read-only display plus manual link/import. Flynt stores local durable mappings in `Task.external_refs` using an `omegon-plan:<json>` prefix, but that mapping is Flynt-owned and does not prove Omegon persisted a reciprocal binding.

Until this node lands, Flynt should stay in `read_only + manual_link` mode. It may display Omegon task projections and let users manually create local board links, but it must not treat those links as authoritative or bidirectional.

### Flynt-owned local link format

Flynt persists local links as:

```text
omegon-plan:<json>
```

Payload shape from Flynt:

```json
{
  "plan_id": "openspec:sync-hardening",
  "task_id": "openspec:sync-hardening:group:Validation:1.2",
  "label": "Validate iCloud open-idle behavior",
  "revision": "sha256:optional"
}
```

Omegon must treat this as an external/local reference unless `_tasks/bind` explicitly returns repo durability.

### Minimum direct-mapping envelope

Task read responses need a stable envelope:

```json
{
  "task": {
    "id": "...",
    "stable_id": "...",
    "revision": "...",
    "source": {
      "kind": "openspec|design|session|hybrid",
      "path": "openspec/changes/foo/tasks.md",
      "anchor": "..."
    },
    "writable": false,
    "supported_mutations": []
  }
}
```

`id` may remain display/projection identity. `stable_id` is what Flynt and future clients bind to. The stable id should survive harmless source edits where possible: OpenSpec group title edits, task reordering, minor label edits, and task renumbering.

Bind responses need an explicit durability envelope:

```json
{
  "accepted": true,
  "durability": "repo|session|none",
  "revision": "sha256:...",
  "binding": {
    "task_id": "...",
    "system": "flynt",
    "external_task_id": "..."
  }
}
```

Flynt may only treat `durability: "repo"` as authoritative for bidirectional mapping.

### Decomposition into implementation contracts

This contract splits into narrower follow-up nodes:

- [[acp-task-stable-identity]] — stable ids for external bindings.
- [[acp-task-binding-store]] — repo/session/none durability and reciprocal binding persistence.
- [[acp-task-mutation-contract]] — explicit supported mutations and revision preconditions.
- [[acp-task-status-error-pagination-contract]] — status mapping, structured errors, and large-repo listing behavior.

## Decisions

### Keep external task refs as attachments, not lifecycle authority

**Status:** proposed

**Rationale:** External systems such as Flynt task boards are coordination surfaces. OpenSpec/design/session/branch artifacts remain authoritative for lifecycle state unless ACP explicitly advertises a supported mutation with revision preconditions.

### Require durability in every bind response

**Status:** proposed

**Rationale:** A client cannot safely know whether a link survived a restart or is only session-local unless `_tasks/bind` returns `durability`. Silent session-local behavior would produce false confidence in Flynt and future clients.

### Require stable task identity separate from projection id

**Status:** proposed

**Rationale:** Projection ids are useful for rendering, but durable external references need a stable identity that survives projection rebuilds when the backing source has not semantically changed.

### Require explicit supported mutations

**Status:** proposed

**Rationale:** `writable: true` is insufficient. Clients need specific operations such as `bind_external_ref`, `set_status`, `append_evidence`, `complete`, and `reopen`, and should disable UI for unsupported operations.

## Open Questions

- [assumption] Flynt and future ACP clients need task bindings that survive projection rebuilds, process restarts, and repository movement.
- Which binding store is authoritative for repo-durable external task refs: OpenSpec sidecar, design-node frontmatter, .omegon repo metadata, or a dedicated binding index?
- What conflict policy should apply when an external task ref points to a task projection whose source task disappeared or changed identity?
- Should session-durable bindings be exposed to external clients, or should ACP only accept repo-durable bindings for non-local ecosystems?

## Implementation Notes

Primary files:

- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/tools/mod.rs`
- `core/crates/omegon/src/acp.rs`
- `openspec/changes/plan-refinement/specs/lifecycle/work-plan-threading.md`

Implementation targets:

- Add task `stable_id`, `revision`, `source`, and `supported_mutations` to ACP task response shapes.
- Define `TaskDurability::{Repo, Session, None}` or equivalent response enum.
- Change `_tasks/bind` to return `accepted`, `durability`, `revision`, and `binding`.
- Accept `expected_revision` on mutating calls and reject stale writes with structured conflict errors.
- Add status mapping docs for `WorkItemStatus` variants and conservative external mapping.
- Reject ambiguous task ids and stale revision preconditions.
- Add tests proving external refs do not mutate OpenSpec/design completion.

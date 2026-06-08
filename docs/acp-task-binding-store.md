---
id: acp-task-binding-store
title: "ACP task binding store"
status: exploring
tags: [acp, tasks, bindings, durability, flynt]
parent: acp-task-durability-contract
related: [acp-task-stable-identity, external-work-surface-integration]
open_questions:
  - "[assumption] Repo-durable external task bindings should not be written into every lifecycle source file by default."
  - "Should the binding store live under `.omegon/`, OpenSpec sidecars, design-node frontmatter, or a dedicated repo metadata document?"
  - "How should bindings merge across branches and worktrees?"
---

# ACP task binding store

## Overview

Define where Omegon persists reciprocal external task bindings. Flynt can already store Flynt-owned `omegon-plan:<json>` links, but authoritative bidirectional mapping requires Omegon to persist its side of the binding and report its durability.

## Research

Durability levels required by the ACP boundary:

- `none`: rejected or not persisted.
- `session`: stored only in live/session state.
- `repo`: persisted in repo-level metadata or source artifacts and safe for external clients to treat as authoritative.

Potential stores:

1. Source-embedded markers in OpenSpec/design files.
2. Source-specific sidecars beside tasks/design docs.
3. Repo-wide `.omegon/task-bindings.*` metadata.
4. Hybrid: repo index plus optional source annotations.

## Decisions

### Return durability explicitly from `_tasks/bind`

**Status:** proposed

**Rationale:** Clients need to distinguish local hints from repo-durable reciprocal bindings.

### Keep binding persistence separate from lifecycle completion

**Status:** proposed

**Rationale:** Binding an external card must not complete, reopen, or otherwise mutate OpenSpec/design task state.

## Open Questions

- [assumption] Repo-durable external task bindings should not be written into every lifecycle source file by default.
- Should the binding store live under `.omegon/`, OpenSpec sidecars, design-node frontmatter, or a dedicated repo metadata document?
- How should bindings merge across branches and worktrees?

## Implementation Notes

Primary files:

- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/acp.rs`
- `.omegon/` metadata conventions

Implementation targets:

- Define binding record schema with `stable_id`, `system`, `external_task_id`, `source`, `revision`, and timestamps.
- Implement `_tasks/bind` durable response envelope.
- Add structured stale/not-found/conflict errors.
- Add tests proving repo-durable binding survives projection rebuild.

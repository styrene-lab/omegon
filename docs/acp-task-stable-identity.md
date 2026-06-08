---
id: acp-task-stable-identity
title: "ACP task stable identity"
status: exploring
tags: [acp, tasks, identity, flynt, ecosystem]
parent: acp-task-durability-contract
related: [acp-task-binding-store, acp-plan-task-revision-events]
open_questions:
  - "[assumption] OpenSpec checkbox text and numbering are not stable enough to be external identity."
  - "Should stable ids be embedded in source artifacts, derived from anchors, or stored in a sidecar index?"
  - "How much task movement/editing must a stable id survive before Omegon reports a new task?"
---

# ACP task stable identity

## Overview

Define stable task identity for ACP task projections. External systems such as Flynt need an id that survives harmless projection rebuilds, task reordering, title edits, and OpenSpec renumbering where possible. The current projection `id` can remain a render/display id, but direct mapping requires a separate `stable_id`.

## Research

Flynt's mapping contract explicitly distinguishes projection id from stable id. Flynt may store local links as `omegon-plan:<json>`, but should only bind authoritatively to Omegon after each task exposes stable identity plus revision/source metadata.

Candidate identity inputs:

- OpenSpec: change name + task group source path + checkbox anchor/stable marker.
- Design-tree: design node id + question/decision/evidence anchor.
- Session plan: session id + item ordinal/content hash, session-durable only.
- Hybrid: durable parent source id + source-specific task anchor.

## Decisions

### Separate projection id from stable id

**Status:** proposed

**Rationale:** Projection ids can remain readable and UI-oriented. Stable ids must optimize for external binding and source reconciliation.

### Prefer source-anchored identity over label hashing

**Status:** proposed

**Rationale:** Label hashes break on harmless edits. Source anchors or explicit markers survive more meaningful changes.

## Open Questions

- [assumption] OpenSpec checkbox text and numbering are not stable enough to be external identity.
- Should stable ids be embedded in source artifacts, derived from anchors, or stored in a sidecar index?
- How much task movement/editing must a stable id survive before Omegon reports a new task?

## Implementation Notes

Primary files:

- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/tools/mod.rs`
- `core/crates/omegon/src/lifecycle/spec.rs`
- `core/crates/omegon/src/lifecycle/design.rs`

Implementation targets:

- Add `stable_id` to task projection ACP JSON.
- Define stable id constructors per source type.
- Add tests for OpenSpec reorder/title-edit stability.
- Mark session tasks as session-scoped identity unless explicitly promoted.

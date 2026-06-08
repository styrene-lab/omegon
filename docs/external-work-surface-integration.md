---
id: external-work-surface-integration
title: "External work surface integration model"
status: exploring
tags: [acp, ecosystem, flynt, integrations, tasks]
parent: plan-refinement
related: [acp-task-durability-contract, acp-plan-task-revision-events, acp-ecosystem-capability-negotiation, external-task-promotion]
open_questions:
  - "[assumption] Flynt is the first external work surface, but the contract should support future boards, issue trackers, IDEs, and dashboards."
  - "Which integration roles should ACP recognize: renderer, linker, coordinator, mutator, or authority?"
  - "How should external systems express local-only links without implying durable lifecycle binding?"
  - "What metadata is common enough to standardize versus provider-specific extension payloads?"
---

# External work surface integration model

## Overview

Define how Omegon exposes work to external surfaces beyond Flynt: task boards, IDE panels, dashboards, issue trackers, and future ACP clients. The immediate trigger is Flynt task-board linkage, but the design should avoid hard-coding Flynt as a special source of truth.

The integration model should classify external systems by role and capability. A work surface may render Omegon tasks, attach local references, coordinate status, or request mutations, but each role has different safety requirements.

## Research

### Current Flynt boundary

Flynt can render task projections and maintain local board cards. The safe current contract is `read_only + manual_link`. Direct mapping requires [[acp-task-durability-contract]] and [[acp-plan-task-revision-events]].

### Generalized external roles

Potential roles:

- `renderer`: reads plans/tasks and displays them.
- `local_linker`: stores local references without lifecycle authority.
- `coordinator`: creates external cards/issues linked to Omegon projections.
- `mutator`: requests explicit Omegon mutations through supported operations.
- `authority`: owns task state for a source domain.

The default for external work surfaces should be `renderer` or `local_linker` until they negotiate stronger capabilities.

## Decisions

### Model Flynt as one external work surface, not a privileged task source

**Status:** proposed

**Rationale:** The ACP boundary should support multiple clients. Hard-coding Flynt semantics into core task projections would make future integrations brittle and would risk elevating a UI board into lifecycle authority accidentally.

### Require explicit authority and mutation negotiation

**Status:** proposed

**Rationale:** A renderer should not be able to mutate lifecycle state by accident. Mutation and authority roles need explicit capability advertisement, supported mutation lists, and revision preconditions.

### Preserve provider-specific metadata in extension fields

**Status:** proposed

**Rationale:** Common fields should be standardized, but each external work surface will have board/list/card/issue metadata that should not force core model churn.

## Open Questions

- [assumption] Flynt is the first external work surface, but the contract should support future boards, issue trackers, IDEs, and dashboards.
- Which integration roles should ACP recognize: renderer, linker, coordinator, mutator, or authority?
- How should external systems express local-only links without implying durable lifecycle binding?
- What metadata is common enough to standardize versus provider-specific extension payloads?

### Flynt-created task promotion

Flynt-created GUI tasks remain Flynt-owned until explicit promotion. The promotion path is defined in [[external-task-promotion]]: offline promotion creates a pending-review draft that preserves original Flynt content, while online promotion injects context into a Flynt-agent skill that binds/imports through Omegon ACP.

## Implementation Notes

Primary files:

- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/acp.rs`
- `docs/flynt-integration.md`
- `docs/acp-surface.md`

Implementation targets:

- Define external surface role/capability vocabulary.
- Add `external_task_refs[].system`, `board_id`, `task_id`, `external_refs`, and future `metadata` semantics to docs/specs.
- Document Flynt 0.12.x compatibility mode as `read_only + manual_link`.
- Define when external systems may request mutations and which source remains authoritative.

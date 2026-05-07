+++
id = "e9096822-37c4-402a-8048-74d375b0abac"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle hygiene — verification substates and binding normalization

## Overview

Harden the post-implementation lifecycle so task-complete changes cannot linger in ambiguous verifying state. Make status surfaces report concrete blockers like missing assessment, stale assessment, or missing design-tree binding, and normalize how OpenSpec/design-tree determine whether a change is bound.

## Research

### Observed backlog failure mode

Several OpenSpec changes sat in generic `verifying` despite having different real blockers: some lacked persisted `assessment.json`, one lacked a bound design-tree node, and some were actually archive-ready. This allowed lifecycle debt to accumulate because `openspec_manage status` did not distinguish actionable sub-states.

### Binding inconsistency

Archive gating currently accepts either an explicit `openspec_change` binding or a fallback match on design node ID, but design-tree lifecycle metadata may still report `boundToOpenSpec: false` when only the fallback path is used. This makes status surfaces disagree about whether a change is lifecycle-bound.

## Decisions

### Decision: Expose concrete verification substates instead of a single generic verifying stage

**Status:** decided
**Rationale:** Operators and agents need to know whether a task-complete change is blocked on missing assessment, stale assessment, missing lifecycle binding, reopened work, or is archive-ready. A generic verifying label hides the next action and lets backlog accumulate.

### Decision: Normalize lifecycle binding so all status surfaces agree on whether a change is bound

**Status:** decided
**Rationale:** If archive gating accepts ID-based fallback binding, the design-tree/OpenSpec status model must either persist that binding canonically or compute `boundToOpenSpec` from the same rule set. Otherwise operators see contradictory lifecycle truth.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/openspec/index.ts` (modified) — status reporting and archive-readiness substate surfacing for task-complete changes
- `extensions/openspec/spec.ts` (modified) — assessment/binding freshness helpers reused by status and archive gates
- `extensions/openspec/archive-gate.ts` (modified) — binding normalization and shared lifecycle gate predicates
- `extensions/design-tree/index.ts` (modified) — lifecycle metadata should agree with OpenSpec binding truth
- `extensions/openspec/*.test.ts` (modified) — regression coverage for missing-assessment, missing-binding, stale-assessment, and archive-ready status surfaces
- `extensions/design-tree/index.test.ts` (new) — Tool-level regression coverage for fallback OpenSpec binding metadata in design-tree status surfaces
- `extensions/design-tree/tree.test.ts` (modified) — Archive transition regression coverage aligned with normalized OpenSpec binding truth

### Constraints

- Status surfaces must distinguish archive blockers instead of collapsing them into a generic verifying state.
- Binding truth must be computed once and reused by OpenSpec status, archive gate, and design-tree lifecycle metadata.
- Fallback ID-based OpenSpec bindings must remain visible in design-tree lifecycle metadata and archive transitions.

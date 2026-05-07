+++
id = "50b9f9bf-1961-4043-9280-faa92c7f4fb7"
kind = "document"
title = "Lifecycle state normalization"
status = "implemented"
tags = ["lifecycle", "design-tree", "openspec", "dashboard", "shared-state"]
aliases = ["lifecycle-state-normalization"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
open_questions = []
openspec_change = "lifecycle-state-normalization"
parent = "repo-consolidation-hardening"
+++

# Lifecycle state normalization

## Overview

Define the next repo-consolidation-hardening slice that reduces duplicated lifecycle truth across design-tree, OpenSpec, dashboard, and memory by introducing a more canonical resolver/publication seam.

## Research

### Why this should be the next slice

The parent consolidation topic identified duplicated lifecycle truth as a top opportunity: design-tree, OpenSpec, dashboard, and memory each publish overlapping state with partially separate derivations. After subprocess hardening, this is the next highest-leverage slice because it improves correctness and internal coherence without requiring an immediate full decomposition of every large extension entrypoint.

### Likely implementation seam

The most practical seam is not a repo-wide rewrite of all state handling at once. Instead, introduce a canonical lifecycle snapshot/resolver module that design-tree, OpenSpec, and dashboard can consume for shared status concepts such as change stage, verification substate, design binding, task completion, and current assessment freshness. Existing extensions can then publish or render from that shared resolver incrementally.

## Decisions

### Decision: Make lifecycle normalization the next repo-consolidation-hardening slice

**Status:** decided
**Rationale:** It directly addresses a top architectural duplication identified in the repo assessment, improves lifecycle correctness across multiple extensions, and is more bounded than attempting broad extension decomposition or model-control consolidation next.

### Decision: Start with a canonical lifecycle resolver, not a full rewrite

**Status:** decided
**Rationale:** A shared resolver for lifecycle state can be adopted incrementally by dashboard, OpenSpec, and design-tree with lower risk than trying to centralize all mutable state immediately. This keeps the slice specable and testable.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/openspec/spec.ts` (modified) — Added canonical lifecycle summary resolver and shared lifecycle summary fields
- `extensions/openspec/index.ts` (modified) — Moved status/get/archive-facing lifecycle reporting onto the shared resolver
- `extensions/openspec/dashboard-state.ts` (modified) — Published dashboard-facing OpenSpec lifecycle state from canonical resolver output
- `extensions/design-tree/index.ts` (modified) — Aligned bound-to-OpenSpec lifecycle metadata with canonical lifecycle binding truth
- `extensions/design-tree/dashboard-state.ts` (modified) — Aligned design-tree dashboard lifecycle publication with canonical resolver semantics
- `extensions/openspec/spec.test.ts` (modified) — Added canonical lifecycle resolver regression coverage
- `extensions/openspec/lifecycle-surfaces.test.ts` (modified) — Added surface agreement tests across status/detail/archive/dashboard
- `extensions/design-tree/index.test.ts` (modified) — Added binding-truth normalization coverage
- `openspec/changes/lifecycle-state-normalization/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/lifecycle-state-normalization.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Preserve historical coarse stage semantics while adding finer verification substate detail through the canonical resolver.
- Dashboard, OpenSpec, and design-tree must not re-derive overlapping lifecycle readiness/binding truth independently when a canonical resolver result is available.

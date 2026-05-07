+++
id = "9a5ae752-0a1a-4aba-906f-913235c1f8b7"
kind = "document"
title = "Dashboard and lifecycle publisher consolidation"
status = "implemented"
tags = ["dashboard", "lifecycle", "publishers", "consolidation", "shared-state"]
aliases = ["dashboard-lifecycle-publisher-consolidation"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
open_questions = []
openspec_change = "dashboard-lifecycle-publisher-consolidation"
parent = "repo-consolidation-hardening"
+++

# Dashboard and lifecycle publisher consolidation

## Overview

Reduce repeated dashboard-state and lifecycle publication plumbing by extracting a narrower shared publisher/update seam for OpenSpec, design-tree, and cleave status emission.

## Research

### Why this is the next bounded consolidation slice

After canonical lifecycle resolution, the next concentrated duplication is publisher plumbing: OpenSpec and design-tree still call `emitOpenSpecState`/`emitDesignTreeState` across many command/tool paths, and cleave maintains its own adjacent status emission flow. Consolidating publication triggers and shared dashboard-update behavior is smaller and safer than attacking oversized entrypoint decomposition or all model-control responsibilities at once.

### Likely implementation seam

Introduce a small shared publisher module or command-safe refresh helpers that own dashboard-state recomputation and event emission for OpenSpec/design-tree/cleave. Existing extensions would call the shared refresher at mutation boundaries instead of manually invoking emit functions at many sites. The goal is to reduce repeated boilerplate and keep dashboard-facing state refresh semantics consistent without rewriting each extension's domain logic.

## Decisions

### Decision: Consolidate publisher plumbing before attempting large entrypoint decomposition

**Status:** decided
**Rationale:** A shared publisher/refresh seam removes repetitive mutation-boundary boilerplate across OpenSpec and design-tree, improves consistency of dashboard updates, and is much more bounded than splitting several 1.5k-2.8k line extensions all at once.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/openspec/index.ts` (modified) — Centralized OpenSpec dashboard refresh usage around the shared publisher helper and file-watch refresh scheduling.
- `extensions/openspec/dashboard-state.ts` (modified) — Serves as the shared OpenSpec dashboard refresh helper that writes shared state and emits dashboard updates.
- `extensions/openspec/dashboard-state.test.ts` (modified) — Regression coverage for the shared OpenSpec dashboard refresh helper contract.
- `extensions/design-tree/index.ts` (modified) — Replaced repeated inline design-tree publisher calls with the local emitCurrentState refresh helper across mutation paths.
- `extensions/design-tree/dashboard-state.ts` (modified) — Continues to own focused-node-aware design-tree dashboard publication for the consolidated refresh path.
- `extensions/design-tree/index.test.ts` (modified) — Regression coverage for focus-aware design-tree dashboard refresh behavior after consolidation.
- `openspec/changes/dashboard-lifecycle-publisher-consolidation/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/dashboard-lifecycle-publisher-consolidation.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Keep consolidation bounded to publisher and refresh seams; do not rewrite unrelated OpenSpec or design-tree domain logic.
- Preserve focus-aware design-tree publication and existing dashboard update semantics while reducing repeated inline refresh boilerplate.

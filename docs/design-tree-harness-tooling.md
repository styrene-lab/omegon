---
id: design-tree-harness-tooling
title: "Design-tree harness tooling"
status: resolved
tags: []
open_questions: []
dependencies: []
related:
  - design-tree-lifecycle
  - directive-branch-lifecycle
  - knowledge-quadrant-lifecycle
issue_type: feature
priority: 1
---

# Design-tree harness tooling

## Overview

Extend the design-tree harness surface so lifecycle management can represent stale, superseded, and historical nodes without deleting them. The immediate driver is cleanup of obsolete TS/pi-era nodes: the current tooling has no archive operation or archive-aware filtering, so dead branches linger in active planning surfaces and force manual doc surgery. The node should define the status/metadata contract, query behavior, and mutation surface needed for archival and supersession inside the design tree harness.

## Research

### Current harness gap

Observed tool gap: `design_tree_update` supports create/set_status/add_question/add_decision/implement but has no archive/supersede surface. As a result, obsolete TS/pi-era nodes cannot be removed from active planning surfaces without manual doc edits. The lack of an archive state also forces query surfaces like `list`, `ready`, and `frontier` to treat historical migration artifacts as if they remain live design work.

## Decisions

### Decision: archival is a first-class terminal status plus metadata

**Status:** decided

**Rationale:** A pure metadata-only archive flag would force every query surface to remember bespoke filtering rules and would leave lifecycle semantics ambiguous. A terminal `archived` status gives clean default filtering and explicit lifecycle meaning, while archive metadata preserves the historical reason and replacement link.

### Decision: archived nodes carry structured historical metadata

**Status:** decided

**Rationale:** Archive events need durable context. Minimum fields: `archive_reason` (obsolete/superseded/merged/historical/rejected), optional `superseded_by`, `archived_at`, and freeform rationale. Without this metadata, future cleanup and design archaeology cannot distinguish deliberate retirement from accidental abandonment.

### Decision: archive-aware query surfaces hide archived nodes by default

**Status:** decided

**Rationale:** The main operator need is active planning hygiene. `list`, `frontier`, `ready`, and child listings should exclude archived nodes unless explicitly requested, while direct `node` lookup should still return archived nodes with their archive banner and metadata. This preserves history without polluting active views.

### Decision: archival must guard against orphaning active descendants

**Status:** decided

**Rationale:** Archiving a parent while live children remain would silently strand active design work. The archive mutation should either refuse archival when non-archived descendants exist or require an explicit override mode that records how descendants were handled. Safe default: fail closed.

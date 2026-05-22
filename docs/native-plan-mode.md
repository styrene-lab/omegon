+++
id = "f7b1cac7-7dbf-42a4-a7f9-8d497a9654b8"
kind = "document"
title = "Native plan mode — structured task decomposition with TUI widget and Auspex/browser integration"
status = "exploring"
tags = ["rust", "tui", "planning", "auspex", "openspec", "design-tree"]
aliases = ["native-plan-mode"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["embedded-web-dashboard"]
issue_type = "epic"
open_questions = ["Should the plan TUI widget live in the dashboard panel, the conversation stream, or both? Dashboard gives persistent visibility; conversation keeps context inline.", "The local `/dash` compatibility surface currently serves basic HTML via the embedded axum server. What's the frontend stack for the enriched plan/lifecycle view in Auspex and, where still needed, the local browser fallback — raw HTML+JS, HTMX, or full Dioxus WASM (mdserve-dioxus-frontend is already a seed node)?"]
parent = "conversation-rendering-engine"
priority = "2"
related = ["mdserve-dioxus-frontend"]
+++

# Native plan mode — structured task decomposition with TUI widget and Auspex/browser integration

## Overview

The Rust TUI needs native task planning — structured decomposition, dependency ordering, interactive approval, and progress tracking. Two surfaces: (1) TUI widget — compact plan view in the conversation or dashboard showing tasks with status badges, dependency arrows, and approve/reject controls. (2) Browser view — Auspex should be the primary rich plan viewer, with the existing `/dash open` localhost UI kept only as a local compatibility surface until behavior migrates. The browser surface should show the full design tree, implementation specs with Given/When/Then scenarios, task progress, and plan history. This is not a separate planning system — it surfaces the same lifecycle data (design nodes, OpenSpec changes, cleave decomposition) through a visual plan interface. The TUI widget shows the current plan inline; the browser view shows the full graph. OpenCrabs' PlanDocument model is a useful reference for the data structure: typed tasks with dependencies, complexity scores, acceptance criteria, and status transitions. But our version should be backed by the existing design-tree + OpenSpec artifacts rather than a separate plan store.

## Decisions

### Decision: Native plan mode is the lowest-level projection of recursive tasking

**Status:** decided
**Rationale:** The Slim operational plan, IntentDocument work plan, design-tree tasks, OpenSpec task groups, cleave decomposition, and memory-backed research questions are not independent planning systems. They are representations of the same recursive tasking system at different abstraction layers. The Slim plan is the immediate execution slice for the next actions/tools; IntentDocument owns the durable session-level tasking state that drives and validates those slices.

### Decision: Slim plan state is a tasking projection, not a TUI-owned store

**Status:** decided
**Rationale:** Slim may cache a render projection for responsiveness, but it must not own plan truth. The authoritative state belongs in IntentDocument tasking, with revisioned projections emitted to TUI/ACP/daemon surfaces. This supersedes any interpretation of the Slim pinned plan as a separate UI checklist substrate.

### Decision: Tasking unification must support suspend, block, resume, and supersede

**Status:** decided
**Rationale:** Unifying the substrates must not lock the operator's whole conversation inside the current plan. Immediate execution slices need lifecycle states for active execution, environmental/upstream blockage, operator suspension ("put a pin in it"), resumption, completion, failure, and supersession. Conversation remains steerable; tasking constrains only claims of execution progress and validation.

### Decision: Memory records durable tasking conclusions and supersession rationale

**Status:** decided
**Rationale:** Task supersession and memory supersession are the same semantic operation at different persistence horizons. When a tasking slice supersedes an approach or architectural belief, durable memory facts tied to the old approach must be superseded and linked to the new rationale. Memory should store decisions, blockers likely to recur, recovery paths, and resumable suspended work pointers — not every transient checklist item.

## 0.23.1 Stop-gap Plan Fix

The 0.23.1 release should not attempt the full recursive tasking migration. The stop-gap is to preserve the existing `IntentDocument.work_plan`/`PlanMode` substrate and make Slim render it with correct active-vs-complete semantics:

- completed retained plans remain visible as evidence, but do not advertise active `/plan advance` or `/plan skip` controls;
- active plans keep the existing advance/skip hints;
- cleared/off plans clear the Slim projection;
- the TUI cache remains a projection cache only and is not treated as an independent tasking store.

This leaves the long-term recursive tasking work exactly where the decisions above place it: replace the stop-gap work-plan projection with revisioned IntentDocument tasking projections that support execution slices, evidence refs, blockers, suspension, resumption, and supersession.

## Open Questions

- What exact Rust data model should represent recursive tasking in IntentDocument: evolve `WorkItem`/`PlanMode`, or introduce a new `TaskingState` with milestones, execution slices, evidence refs, blockers, and revisioned projections?
- Which plan actions are required for the first unification slice: `suspend`, `resume`, `block`, `supersede`, `fail`, and/or `archive`, and what are their operator-facing slash/tool names?
- How should validation evidence attach to an execution slice so high-level IntentDocument milestones can distinguish agent-reported completion from tool-backed or operator-accepted completion?
- The local `/dash` compatibility surface currently serves basic HTML via the embedded axum server. What's the frontend stack for the enriched plan/lifecycle view in Auspex and, where still needed, the local browser fallback — raw HTML+JS, HTMX, or full Dioxus WASM (mdserve-dioxus-frontend is already a seed node)?

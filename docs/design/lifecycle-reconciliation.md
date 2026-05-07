+++
id = "e02a90c5-c5f1-487b-81fc-443626f5909c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle Reconciliation — Ambient Sync from Implementation Reality back to Design Tree + OpenSpec

## Overview

Make lifecycle tracking bidirectional. Today the flow from design tree → OpenSpec → cleave works well, but agent-driven implementation does not automatically reconcile tasks, node status, and dashboard state as reality changes. Add ambient reconciliation points so implementation progress updates design-tree and OpenSpec continuously and reliably without manual bookkeeping.

## Research

### Gap analysis — current flow is write-forward but not reality-reconciled

Current lifecycle automation is front-loaded: design_tree implement/scaffold creates OpenSpec artifacts and cleave executes against them, but downstream reality changes are only partially reflected back. Design tree status updates on archive are good, but OpenSpec task checkboxes, partial completion markers, and implementation notes depend on the agent remembering to update them. This creates dashboard drift: the system can show planned work when code and tests indicate substantial completion. The missing abstraction is a reconciliation layer that runs at natural checkpoints and treats design-tree + OpenSpec as runtime state to be synchronized, not static planning docs.

### Recommended reconciliation checkpoints

Four checkpoints should own state sync. (1) **Implement/scaffold**: bind node ↔ openspec_change ↔ branch and mark node implementing. (2) **Post-child / post-cleave merge**: update tasks.md by marking completed items and adding honest partial progress where merged code landed but follow-up remains. (3) **Post-assess / post-fix**: if review or spec fixes change scope, append implementation notes and refresh task status to reflect what remains. (4) **Pre-archive / archive**: validate that tasks.md and node status match reality, then transition implementing→implemented and archive the change. These checkpoints should be harness-owned so the agent naturally performs them during execution.

## Decisions

### Decision: Reconciliation is a required lifecycle phase, not an optional cleanup step

**Status:** decided
**Rationale:** Every implementation flow should include ambient reconciliation checkpoints that update design-tree and OpenSpec to match reality. The agent should not treat task docs as write-once planning artifacts. Instead, after scaffold, after cleave child completion, after assessment/fixes, and before archive, the harness should reconcile: (1) design node status, (2) OpenSpec tasks.md completion state, (3) implementation notes/file scope when materially changed, and (4) dashboard-facing summary state. This makes progress metrics trustworthy without requiring operator reminders.

### Decision: Automatic checkpoint reconciliation with lightweight stale-state guard

**Status:** decided
**Rationale:** Checkpoint reconciliation should run automatically at implement/scaffold, post-cleave, post-assess, and pre-archive so the agent does not need to remember bookkeeping. Add a lightweight guard only where stale state would mislead lifecycle transitions — especially before archive and after cleave completion when tasks remain obviously unreconciled. Automatic sync keeps flow smooth; the guard preserves dashboard trust.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/openspec/reconcile.ts` (new) — Shared lifecycle reconciliation checks for stale OpenSpec/design-tree state
- `extensions/openspec/dashboard-state.ts` (new) — Reusable OpenSpec dashboard state emitter shared by openspec and cleave
- `extensions/cleave/index.ts` (modified) — Emit OpenSpec dashboard refresh after write-back and warn on unmapped completed work
- `extensions/cleave/openspec.ts` (modified) — Return unmatched task-group labels during post-merge write-back
- `extensions/openspec/index.ts` (modified) — Refuse archive when lifecycle state is stale and reuse shared dashboard emitter
- `skills/openspec/SKILL.md` (modified) — Document reconciliation checkpoints as required lifecycle behavior
- `skills/cleave/SKILL.md` (modified) — Document post-cleave reconciliation and pre-archive expectations

### Constraints

- Automatic reconciliation should happen at natural checkpoints instead of relying on operator reminders
- Archive must refuse obviously stale lifecycle state, especially incomplete tasks or missing design-tree binding
- Dashboard-facing OpenSpec state must refresh after cleave reconciles tasks.md

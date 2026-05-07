+++
id = "d0eb3588-c5ac-49af-bcf1-1fc9e1dd9f7b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle Reconciliation — Ambient Sync from Implementation Reality back to Design Tree + OpenSpec — Design

## Architecture Decisions

### Decision: Reconciliation is a required lifecycle phase, not an optional cleanup step

**Status:** decided
**Rationale:** Every implementation flow should include ambient reconciliation checkpoints that update design-tree and OpenSpec to match reality. The agent should not treat task docs as write-once planning artifacts. Instead, after scaffold, after cleave child completion, after assessment/fixes, and before archive, the harness should reconcile: (1) design node status, (2) OpenSpec tasks.md completion state, (3) implementation notes/file scope when materially changed, and (4) dashboard-facing summary state. This makes progress metrics trustworthy without requiring operator reminders.

### Decision: Automatic checkpoint reconciliation with lightweight stale-state guard

**Status:** decided
**Rationale:** Checkpoint reconciliation should run automatically at implement/scaffold, post-cleave, post-assess, and pre-archive so the agent does not need to remember bookkeeping. Add a lightweight guard only where stale state would mislead lifecycle transitions — especially before archive and after cleave completion when tasks remain obviously unreconciled. Automatic sync keeps flow smooth; the guard preserves dashboard trust.

## Research Context

### Gap analysis — current flow is write-forward but not reality-reconciled

Current lifecycle automation is front-loaded: design_tree implement/scaffold creates OpenSpec artifacts and cleave executes against them, but downstream reality changes are only partially reflected back. Design tree status updates on archive are good, but OpenSpec task checkboxes, partial completion markers, and implementation notes depend on the agent remembering to update them. This creates dashboard drift: the system can show planned work when code and tests indicate substantial completion. The missing abstraction is a reconciliation layer that runs at natural checkpoints and treats design-tree + OpenSpec as runtime state to be synchronized, not static planning docs.

### Recommended reconciliation checkpoints

Four checkpoints should own state sync. (1) **Implement/scaffold**: bind node ↔ openspec_change ↔ branch and mark node implementing. (2) **Post-child / post-cleave merge**: update tasks.md by marking completed items and adding honest partial progress where merged code landed but follow-up remains. (3) **Post-assess / post-fix**: if review or spec fixes change scope, append implementation notes and refresh task status to reflect what remains. (4) **Pre-archive / archive**: validate that tasks.md and node status match reality, then transition implementing→implemented and archive the change. These checkpoints should be harness-owned so the agent naturally performs them during execution.

---
id: plan-refinement
title: "Plan Refinement: Lifecycle-Threaded Small Tasklists"
status: exploring
tags: [plan, lifecycle, openspec, ux]
open_questions:
  - "[assumption] Operators want the small tasklist to remain lightweight and directly editable during a turn, even when it is backed by OpenSpec or design-tree artifacts."
  - "[assumption] OpenSpec tasks.md can be the durable task source for spec-backed work without forcing every conversation plan to become an OpenSpec change."
  - "What should happen when an operator edits or completes a small plan item that is projected from OpenSpec tasks.md: write through, stage a proposed patch, or mark runtime-only?"
  - "How should the UI disclose plan binding/source so operators understand whether they are editing ephemeral session state, design-tree state, or OpenSpec-backed task state?"
  - "What lifecycle transitions should small-plan UX nudge toward, and which transitions should require explicit operator confirmation?"
  - "How should plan bindings represent non-coding work such as research, design exploration, writing, review, operations, and decision capture without forcing those tasks into implementation-only semantics?"
dependencies: []
related: []
---

# Plan Refinement: Lifecycle-Threaded Small Tasklists

## Overview

Explore how Omegon's small in-session work plan/tasklist should thread through the design-tree and OpenSpec lifecycle without becoming a second competing source of truth. Focus areas: UX, binding semantics, runtime projection, lifecycle reconciliation, and degradation when lifecycle artifacts are absent.

## Research

### Initial code surface

Small work-plan runtime state is stored in core/crates/omegon/src/conversation.rs as IntentDocument.work_plan plus IntentDocument.plan_mode; prior memory records that these can diverge. Slash commands mutate plan state through execute_plan_slash_command in core/crates/omegon/src/main.rs. Lifecycle/OpenSpec read models are projected through core/crates/omegon/src/lifecycle/spec.rs, lifecycle/read_model.rs, ipc/snapshot.rs, web/api.rs, and tui/dashboard.rs. Design goal should avoid creating a third independent task source.

### Locked UX model for background plans and resume

UX model locked: the small tasklist remains a selected visible projection. Plans get stable identities and lifecycle bindings; a registry indexes active/backgrounded/blocked/completed/detached/archived plans; a completion ledger records background completions with evidence refs; resume uses ranked candidates instead of blindly restoring the last visible tasklist. Background plan completion emits a notification and updates recent/completed lanes but does not steal focus.

### Session vs repo plan scope

Locked scope model: session plans are for immediate/disposable execution and live in conversation/session state; repo-bound plans are durable/resumable/reviewable projections over OpenSpec, design-tree, branch/worktree, commit, and lifecycle artifacts. Promotion is explicit and should be nudged for multi-session, backgrounded, branch-attached, multi-file, public API, or design-question-heavy work. Clearing session plans deletes runtime state; clearing repo-bound plans detaches the visible projection and leaves durable artifacts intact.

### Adversarial assessment corrections

Assessment found major implementation risks: loose plan_scope/plan_binding fields beside legacy work_plan/plan_mode would worsen divergence; OpenSpec write-through is unsafe until stable task identity exists; registry status mixes derived artifact state with view/session state; background plan events need explicit sources or stale detection; non-coding task intents require completion evidence policies; session snapshot migration must preserve legacy work_plan/plan_mode; TUI snapshot JSON must remain backward-compatible; branch/worktree registry entries need noise filters; tracked ledger storage is deferred until evidence boundaries are clear.

## Decisions

### Use binding-aware projections instead of a new durable task source

**Status:** proposed

**Rationale:** The small tasklist should remain fast and low-friction, but durable lifecycle-backed work should project from OpenSpec/design-tree artifacts where possible. Creating another authoritative task store would worsen divergence between conversation plan state, OpenSpec tasks.md, and design-tree implementation state.

### Make plan source/binding visible in the UX

**Status:** proposed

**Rationale:** Operators need to know whether a checklist action will affect only the current conversation, update design lifecycle state, or write/propose edits to OpenSpec tasks.md. Hidden binding state creates surprising side effects and undermines trust.

### Use a plan registry, visible projection, and completion ledger

**Status:** accepted

**Rationale:** The visible tasklist should be the current lens over work, not the storage model. A registry organizes active/backgrounded/completed plans by lifecycle binding; projections render the selected plan; a completion ledger preserves background completions and resume evidence without cluttering the foreground UX.

### Background plan completion must not steal focus

**Status:** accepted

**Rationale:** Background plans should emit notifications and enter a completed/recent lane. They must not replace the operator's current visible plan, because focus-stealing completions would make the compact tasklist unreliable during active work.

### Resume work from ranked lifecycle-aware plan candidates

**Status:** accepted

**Rationale:** Session resume should be assisted but explicit. Candidates should be ranked from active foreground plans, blocked/backgrounded lifecycle-bound plans, incomplete recent OpenSpec/design work, and recent completed plans as context. The operator chooses what to resume.

### Separate session-scoped plans from repo-bound plans

**Status:** accepted

**Rationale:** Session plans are lightweight runtime checklists for immediate work. Repo-bound plans are projections over durable lifecycle artifacts such as OpenSpec, design nodes, branches, and commits. Promotion from session to repo is explicit and nudged by complexity/backgrounding; detaching a repo-bound plan hides the projection without mutating durable artifacts.

### Treat OpenSpec as work tracking, not code-only tracking

**Status:** accepted

**Rationale:** OpenSpec should track any durable, reviewable work with acceptance criteria or lifecycle state: research, design, operations, validation, documentation, review, and implementation. Code changes are one task intent, not the only valid OpenSpec-backed plan type.

### Implement compatibility wrapper before registry or write-through

**Status:** accepted

**Rationale:** Adding loose scope/binding metadata beside legacy work_plan and plan_mode would preserve and worsen divergence. First implementation must introduce a central VisiblePlanState/PlanAction compatibility wrapper and route slash/tool mutations through one API before registry, projections, or durable OpenSpec write-through.

### Require stable task identity before OpenSpec write-through

**Status:** accepted

**Rationale:** Current OpenSpec task handling only counts checkboxes. Durable write-through would be unsafe without stable task identity across edits. First registry/projection work must be read-only or runtime-only until task identity is defined and tested.

### Split registry derived state from view state

**Status:** accepted

**Rationale:** The plan registry must not become a competing database. Derived state should be recomputed from OpenSpec/design/git/session artifacts; view state should hold backgrounded/detached/last-seen/resume UI state. Durable task truth remains in lifecycle artifacts.

### Treat non-coding completion as evidence-policy driven

**Status:** accepted

**Rationale:** Task intent labels are insufficient. Research, design, validation, operations, review, and documentation tasks need completion policies and evidence kinds so work can complete without code diffs while still being auditable.

### Defer tracked ledger storage until evidence boundaries are clear

**Status:** accepted

**Rationale:** A tracked JSONL ledger could create repo churn and merge conflicts; an untracked ledger would not support cross-machine resume. First implementation should keep ledger/event state as local/session cache and write durable summaries to existing lifecycle artifacts where appropriate.

## Open Questions

- [assumption] Operators want the small tasklist to remain lightweight and directly editable during a turn, even when it is backed by OpenSpec or design-tree artifacts.
- [assumption] OpenSpec tasks.md can be the durable task source for spec-backed work without forcing every conversation plan to become an OpenSpec change.
- What should happen when an operator edits or completes a small plan item that is projected from OpenSpec tasks.md: write through, stage a proposed patch, or mark runtime-only?
- How should the UI disclose plan binding/source so operators understand whether they are editing ephemeral session state, design-tree state, or OpenSpec-backed task state?
- What lifecycle transitions should small-plan UX nudge toward, and which transitions should require explicit operator confirmation?
- How should plan bindings represent non-coding work such as research, design exploration, writing, review, operations, and decision capture without forcing those tasks into implementation-only semantics?

## Implementation Notes

### Constraints

- Do not make every small plan require OpenSpec.
- Do not introduce a second durable task store competing with OpenSpec tasks.md or design-tree state.
- PlanMode and work_plan state must not diverge after this work.
- UX must disclose source/binding before write-through to lifecycle artifacts.
- Clearing a visible plan must not silently delete durable lifecycle tasks.

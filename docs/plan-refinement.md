---
id: plan-refinement
title: "Plan Refinement: Lifecycle-Threaded Small Tasklists"
status: exploring
tags: [plan, lifecycle, openspec, ux]
open_questions:
  - "Should the first implementation slice use names `VisiblePlanState` and `PlanAction`, or should it adopt existing Workbench/domain terminology before code lands?"
  - "Where should local/session-only plan registry cache live so resume works without creating tracked repo churn?"
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

### Flynt-owned ACP task mapping contract

Flynt now documents `docs/omegon-plan-task-acp-mapping.md` in its repository. The important boundary is that Flynt 0.12.x stores local links in `Task.external_refs` using `omegon-plan:<json>`, but that link is Flynt-owned and does not prove Omegon persisted a reciprocal binding. Direct/bidirectional mapping requires new Omegon contract work: stable task identity, revision/concurrency tokens, durable bind responses, explicit supported mutations, status enum mapping, real events or revision polling, pagination/filtering, and structured errors. Follow-up nodes: [[acp-task-durability-contract]], [[acp-task-stable-identity]], [[acp-task-binding-store]], [[acp-task-mutation-contract]], [[acp-task-status-error-pagination-contract]], [[acp-plan-task-revision-events]], [[external-work-surface-integration]], and [[acp-ecosystem-capability-negotiation]].

## Decisions

### Use binding-aware projections instead of a new durable task source

**Status:** accepted

**Rationale:** The small tasklist should remain fast and low-friction, but durable lifecycle-backed work should project from OpenSpec/design-tree artifacts where possible. Creating another authoritative task store would worsen divergence between conversation plan state, OpenSpec tasks.md, and design-tree implementation state.

### Make plan source/binding visible in the UX

**Status:** accepted

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

### Keep Flynt 0.12.x in read-only + manual-link mode

**Status:** accepted

**Rationale:** Flynt can safely render Omegon plan/task projections and store Flynt-local `omegon-plan:<json>` references, but authoritative bidirectional mapping is unsafe until Omegon exposes stable ids, revision tokens, durable bind responses, explicit mutation lists, and event/revision polling.

### Keep the small tasklist lightweight and directly editable

**Status:** accepted

**Rationale:** The in-turn tasklist is valuable because it is fast, visible, and low-friction. Lifecycle backing should enrich provenance and resume behavior, not make every checklist action feel like editing a database. For lifecycle-backed projections, direct edits are session/runtime actions unless the UI explicitly discloses and confirms a durable write.

### Use OpenSpec tasks.md as the durable source for spec-backed work only

**Status:** accepted

**Rationale:** OpenSpec can be the durable task source when work is already spec-backed, but ordinary conversation plans remain session-scoped. Promotion to OpenSpec is explicit and nudged by complexity, multi-session scope, public API impact, or reviewability needs.

### Treat OpenSpec-projected edits as runtime-only or staged until stable identity exists

**Status:** accepted

**Rationale:** Automatic write-through to tasks.md is unsafe until tasks have stable IDs, revision/concurrency tokens, and conflict behavior. Near-term plan actions on OpenSpec projections may update runtime view state or stage/propose a patch, but must not silently mutate durable task files.

### Disclose plan source and mutation effect in the Workbench

**Status:** accepted

**Rationale:** Every visible plan projection should show compact source metadata such as `session`, `openspec:<change>`, `design:<node>`, or `branch:<name>`. Actions that would mutate durable lifecycle artifacts require explicit language before execution; runtime-only actions should say the durable source is unchanged.

### Require confirmation for durable lifecycle transitions

**Status:** accepted

**Rationale:** The UX may nudge toward promotion, resume, deciding, or implementation, but durable transitions must remain explicit operator decisions. Confirm before writing OpenSpec/design-tree files, marking tasks complete in durable artifacts, marking design nodes decided/implemented, archiving, deleting, detaching durable plans, or promoting a session plan to repo-bound lifecycle state.

### Model non-coding tasks with intent and evidence policy

**Status:** accepted

**Rationale:** Research, design, validation, operations, review, documentation, and decision-capture work should complete through evidence appropriate to the work, not through code diffs. Plan items need an intent and accepted evidence kinds such as command output, document paths, design decisions, validation logs, external references, operator acknowledgement, or commits.

## Open Questions

- Should the first implementation slice use names `VisiblePlanState` and `PlanAction`, or should it adopt existing Workbench/domain terminology before code lands?
- Where should local/session-only plan registry cache live so resume works without creating tracked repo churn?

## Implementation Notes

### First implementation slice

Implement a compatibility wrapper before registry/write-through work. The first slice should centralize current `IntentDocument.work_plan` and `IntentDocument.plan_mode` handling behind one visible-plan API, route `/plan` slash commands and tool-result plan mutations through that API, and preserve current session snapshot/TUI JSON compatibility. It should not add OpenSpec write-through, a durable registry, or Flynt bidirectional task binding.

### Constraints

- Do not make every small plan require OpenSpec.
- Do not introduce a second durable task store competing with OpenSpec tasks.md or design-tree state.
- PlanMode and work_plan state must not diverge after this work.
- UX must disclose source/binding before write-through to lifecycle artifacts.
- Clearing a visible plan must not silently delete durable lifecycle tasks.

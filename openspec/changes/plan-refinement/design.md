# Plan Refinement Design

## UX thesis

The small tasklist should remain the operator's immediate execution cockpit. It should not become a mandatory planning ceremony. The UX must answer three questions at a glance:

1. What am I doing now?
2. Where did this tasklist come from?
3. What will happen if I check, edit, clear, or complete an item?

## Binding model

Plans should carry an explicit binding:

- `ephemeral` — session-local runtime plan.
- `design` — projected from or attached to a design-tree node.
- `openspec` — projected from an OpenSpec change task file.
- `branch` — associated with a git branch or worktree.
- `hybrid` — OpenSpec owns task acceptance while design-tree owns rationale/decisions.

Plans also have scope:

- `session` — fast, local, disposable checklist for immediate execution.
- `repo` — durable, resumable, reviewable projection over lifecycle/repo artifacts.

The visible plan is a projection over the binding source. OpenSpec and design-tree artifacts remain the durable source of truth when bound. Promotion from session to repo is explicit and should be nudged when work becomes multi-session, backgrounded, branch-attached, multi-file, public API affecting, or design-question-heavy. Detaching a repo-bound plan hides the projection without mutating durable artifacts.

## UX states

### Ephemeral

Label: `Plan · session`

Actions:
- edit item: runtime-only
- complete item: runtime-only
- clear: remove visible runtime plan
- bind: optional explicit action

### OpenSpec-bound

Label: `Plan · OpenSpec:<change>`

Actions:
- complete item: explicit write-through or prompt-on-first-write
- clear: detach/clear runtime view only
- sync: refresh from the backing task file
- open: show backing task file

### Design-bound

Label: `Plan · Design:<node>`

Actions:
- complete item: update design task/checkpoint only if supported; otherwise runtime annotation
- clear: detach runtime view only
- promote: create/spec OpenSpec change from design node when work becomes non-trivial

## Interaction principles

- Never hide binding state.
- Never silently mutate durable lifecycle files from an ambiguous action.
- Prefer projection over copying.
- Use ephemeral plans for low-ceremony execution.
- Nudge to OpenSpec when the plan becomes multi-file, public API, cross-cutting, or review-bound.

## Implementation direction

First implementation must introduce a compatibility wrapper and central mutation API before registry or write-through work. Adding loose metadata beside the existing `work_plan` and `plan_mode` fields would preserve the divergence this design is meant to remove.

Use a `VisiblePlanState`/`PlanAction` style boundary:

- legacy session fields can remain for serde compatibility during migration;
- new code should mutate plans only through one `apply_plan_action` API;
- slash commands and tool-driven plan updates must use the same API;
- snapshot JSON must remain backward-compatible for existing TUI behavior;
- old session snapshots must migrate legacy `work_plan` + `plan_mode` into an ephemeral session visible plan.

Registry and durable lifecycle write-through come after this compatibility layer is in place.

## Plan registry and completion ledger

The visible tasklist is only the selected lens over work. It is not the storage model.

Plans need stable identities and registry entries:

```text
plan_id
title
source: ephemeral | design | openspec | hybrid
binding: design node / OpenSpec change / task group / session id
status: active | backgrounded | blocked | completed | detached | archived
progress
last_event_at
last_visible_at
resume_hint
```

The registry is an index over durable sources and runtime state, not a new authoritative task database. It is allowed to cache view state and resume hints, but OpenSpec task completion still belongs in OpenSpec task files and design lifecycle state still belongs in design-tree artifacts.

Registry data must split derived state from view state:

```text
derived state
  recomputed from OpenSpec, design-tree, git, session snapshots, and validation artifacts

view state
  backgrounded/detached/dismissed status, last visible time, last seen event, resume UI hints
```

This keeps background/detach/resume UX from becoming a competing source of task truth.

Completed plans move into a completion ledger rather than staying pinned as the foreground plan. Completion records should include source, binding, completion time, summary, item count, evidence references, commits, and validation/spec references when available.

## Background behavior

Background plans can continue to receive task events while hidden. They emit concise notifications:

```text
Background plan completed: par-term probe evidence · 9/9
```

They must not steal foreground focus. The visible plan remains whatever the operator selected. Completed/backgrounded plans appear in collapsed dashboard/history lanes and can be reopened explicitly.

## Resume behavior

Resume should be explicit but assisted. On session resume, rank candidates from:

1. Previous foreground plan if still active.
2. Blocked or backgrounded lifecycle-bound plans with recent activity.
3. OpenSpec changes with incomplete tasks and recent commits/files.
4. Design nodes in exploring/implementing states with recent activity.
5. Recent completed plans as context only.

The operator chooses via plan resume/switch UX; Omegon should not silently bind the session to stale work.

## Non-coding tasking

Plans are not implementation-only. A plan item needs an intent type so research, design, writing, review, operations, validation, and decision capture are first-class.

Candidate task intents:

- `research` — gather evidence, compare options, cite sources, produce findings.
- `design` — refine architecture/UX, resolve open questions, record decisions.
- `spec` — write or update OpenSpec requirements/scenarios/tasks.
- `implementation` — modify code/config/assets.
- `validation` — run tests, checks, adversarial review, smoke evidence.
- `documentation` — author docs, changelog, release notes, operator guidance.
- `operations` — branch/release/worktree/deployment workflow actions.
- `review` — inspect code/spec/design and produce acceptance/blockers.

Each intent has different completion evidence. Research completes with findings and citations; design completes with decisions and resolved questions; spec work completes with scenarios/tasks; implementation completes with diffs/tests; operations complete with branch/tag/deployment state. The compact plan UI should show the same checklist shape while the backing lifecycle evidence differs by intent.

Non-coding work should usually bind to design-tree nodes first when it is still exploratory and question-heavy. OpenSpec is appropriate once the work has durable lifecycle value, acceptance criteria, review checkpoints, or completion evidence to track. That includes research, design, operations, validation, documentation, review, and implementation; OpenSpec is work tracking, not code-only tracking.

## Data model sketch

The runtime should separate durable source identity from visible projection state:

```rust
enum PlanScope {
    Session,
    Repo,
}

enum PlanSource {
    Ephemeral,
    Design,
    OpenSpec,
    Branch,
    Hybrid,
}

enum PlanStatus {
    Active,
    Backgrounded,
    Blocked,
    Completed,
    Detached,
    Archived,
    Stale,
}

enum TaskIntent {
    Research,
    Design,
    Spec,
    Implementation,
    Validation,
    Documentation,
    Operations,
    Review,
}

struct PlanBinding {
    design_node_id: Option<String>,
    openspec_change: Option<String>,
    openspec_task_group: Option<String>,
    branch: Option<String>,
    worktree: Option<PathBuf>,
    session_id: Option<String>,
}

struct PlanRegistryEntry {
    plan_id: String,
    title: String,
    scope: PlanScope,
    source: PlanSource,
    status: PlanStatus,
    binding: PlanBinding,
    progress: ProgressSummary,
    last_event_at: Timestamp,
    last_visible_at: Option<Timestamp>,
    resume_hint: Option<String>,
}

struct PlanItemProjection {
    id: String,
    label: String,
    status: WorkItemStatus,
    intent: TaskIntent,
    evidence: Vec<EvidenceRef>,
    writable: bool,
}
```

`IntentDocument.work_plan` should either become the current selected session projection or be wrapped by a `VisiblePlanState` that points at a `plan_id`. The plan registry can be cached, but it must be rebuildable from repo artifacts plus session metadata where possible.

## Safety constraints from adversarial assessment

Implementation ordering is part of the design:

1. Compatibility wrapper and central mutation API.
2. Backward-compatible visible plan snapshot extensions.
3. Read-only registry/projections.
4. Repo-bound visible projections with detach/degrade behavior.
5. Stable task identity.
6. Explicit durable write-through.

OpenSpec write-through is forbidden until task identity is stable. Current task counting only sees checkbox totals, so write-through cannot safely target a specific task item yet.

Non-coding task completion must be evidence-policy driven. Intent labels such as `research` or `design` are not enough; items need policies such as manual, evidence required, operator accepted, lifecycle state reached, or all subtasks done.

Branch/worktree registry entries must be filtered to avoid stale branch noise. Prefer current worktrees, recent branches, pinned branches, or branches with OpenSpec/design bindings; ignore backup branches and historical release branches by default.

Do not introduce a tracked JSONL ledger in the first implementation. Keep local/session event state as cache and write durable summaries to existing lifecycle artifacts when appropriate.

## Command surface

The slash-command UX should make scope and mutation explicit:

```text
/plan list                 Show active/backgrounded/completed registry entries.
/plan show [id]            Render a plan projection without switching focus.
/plan switch <id>          Make an existing plan the visible foreground plan.
/plan resume <id>          Resume a ranked candidate and make it active.
/plan background [id]      Hide from foreground while preserving registry state.
/plan detach [id]          Clear visible projection without deleting durable source.
/plan promote              Convert session plan to design/OpenSpec/hybrid tracking.
/plan bind ...             Attach current plan to design/OpenSpec/branch context.
/plan complete-item ...    Complete runtime-only or explicit write-through item.
/plan ledger [id]          Show completion/evidence history.
```

`/plan clear` remains valid, but it should dispatch by scope:

- session plan: clear runtime state.
- repo-bound plan: detach visible projection and report that durable artifacts are unchanged.

## UX copy examples

Foreground labels:

```text
Plan · session · 2/5
Plan · OpenSpec:plan-refinement · UX and Binding · 0/4
Plan · Design:plan-refinement · research · 2 questions open
Plan · Hybrid:plan-refinement · spec+design · 5/17
```

Background notifications:

```text
Background plan completed: par-term probe evidence · 9/9
Plan blocked: host-actions voice · branch diverged from main
Detached OpenSpec plan view; task file unchanged.
```

Resume prompt:

```text
Resume candidates
1. plan-refinement · OpenSpec+Design · 0/17 · branch plan-refinement
2. host-actions-0.24 · Branch · blocked/diverged · worktree present
3. par-term graphics · completed · 1 commit ahead of main
```

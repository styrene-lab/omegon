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

- `openspec_manage` owns durable OpenSpec mutation. It is the only tool surface that should edit OpenSpec task checkboxes.
- `/plan` and the `plan` tool project, list, navigate, and manage session-local plan state. They must not duplicate OpenSpec mutation actions.
- A future ergonomic `/plan complete-backed` command may resolve a visible OpenSpec-backed item, but it must delegate to the same OpenSpec service function used by `openspec_manage`.

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

## Targeted remainder

The remaining implementation is scoped to `plan-refinement` only. The target is not a new durable task database; it is a registry/read-model plus session-local view state that prevents stale foreground plans and makes lifecycle bindings explicit.

### Decisions now closed

- Foreground labels are canonical: `Plan · session`, `Plan · OpenSpec:CHANGE`, `Plan · Design:NODE`, and `Plan · Hybrid:CHANGE/NODE`.
- Completing an OpenSpec-backed item through `/plan` is runtime-only unless it explicitly delegates to `openspec_manage` task-status mutation with a stable numeric task id. `/plan` must not edit `tasks.md` directly.
- `/plan clear` is scope-sensitive: session plans clear runtime state; repo-bound plans detach the visible projection and report that durable artifacts are unchanged.
- Missing or changed backing artifacts degrade to `Stale`; the UI keeps a last summary and asks for explicit `/plan sync`, `/plan rebind`, or `/plan detach`.

### Implementation target

1. Finish repo-bound detach/reconciliation around the existing `VisiblePlanState` and `apply_plan_action` boundary.
2. Add a read-only `PlanRegistry` builder that derives entries from visible session state, OpenSpec tasks, design-tree state, and git context.
3. Add session-local registry view state for background/detached/dismissed/last-visible/resume hints. This state is cache/UI state, not task truth.
4. Add a session-local completion ledger shape for evidence and summaries. Do not add tracked JSONL storage in this change.
5. Add explicit resume/switch/background/detach/show/ledger UX. Startup may present candidates but must not silently foreground stale or completed work.
6. Thread `PlanScope`, `TaskIntent`, `TaskCompletionPolicy`, and `EvidenceRef` through projections and snapshots so non-coding task completion has first-class evidence semantics.

### Primary files

- `core/crates/omegon/src/conversation.rs` — data model, visible state, registry entries, ledger/event structs, snapshot compatibility.
- `core/crates/omegon/src/main.rs` — `/plan` slash command handlers and remote slash behavior.
- `core/crates/omegon/src/loop.rs` — plan tool/list enrichment, notification behavior, no-focus-steal behavior.
- `core/crates/omegon/src/lifecycle/design.rs` — design-node candidate projection and evidence binding hooks.
- `core/crates/omegon/src/tui/dashboard.rs` / Slim plan lane surfaces — resume candidates, stale/degraded copy, foreground labels.
- `openspec/changes/plan-refinement/tasks.md` — authoritative remaining task breakdown.

### ACP and Flynt task-board exposure

Flynt should see the same plan registry as the TUI, not a second task system. ACP exposes plan surfaces as typed methods, and treats a plan as a composition of task projections. Some tasks may be backed by OpenSpec/design items, some may be session-local, and some may be linked to Flynt board tasks. Omegon owns the plan/task projection contract; Flynt owns its board UI and task records.

Advertise these capabilities from `runtime/capabilities`:

```text
_plans/list       read-only registry rows for visible/backgrounded/completed/stale plans
_plans/show       read-only detail for one plan id, including task projections and evidence refs
_plans/events     recent session-local plan/task events and completion ledger rows
_plans/switch     make an existing registry entry foreground; explicit operator action
_plans/detach     detach foreground or selected repo-bound projection; never edits durable artifacts
_tasks/list       read-only task projections, optionally filtered by plan id or lifecycle binding
_tasks/show       read-only detail for one task projection
_tasks/bind       bind an external task id, such as a Flynt board task, to a plan task projection
_tasks/events     recent task-level coordination/evidence events
```

Method semantics:

- ACP read methods (`_plans/list`, `_plans/show`, `_plans/events`, `_tasks/list`, `_tasks/show`, `_tasks/events`) are safe for Flynt to poll and render in sidebars, boards, and lenses. They must not mutate OpenSpec, design-tree, git, or task-board state.
- ACP mutation methods (`_plans/switch`, `_plans/detach`, `_tasks/bind`) mutate only Omegon session/view binding metadata unless they delegate to an existing lifecycle tool with explicit intent.
- Task projections carry stable ids, labels, status, intent, completion policy, evidence refs, and parent `plan_id`. They are not a new durable task table; they are the item-level view of the plan registry.
- External task linkage is metadata on a task projection/binding, e.g. `external_task_refs: [{ system: "flynt", board_id, task_id }]`. The Flynt task can represent coordination/status, but OpenSpec/design remains authoritative for lifecycle task completion.
- Completing a Flynt board task must not silently check OpenSpec boxes. The safe bridge is: Flynt task completion records a task event/evidence ref; durable OpenSpec mutation still goes through `openspec_manage` stable task-id mutation.
- ACP plan entries should include `plan_id`, title, source, scope, status, binding summary, progress, stale flag, and resume hint. ACP task entries should include parent `plan_id`, lifecycle binding, external task refs, task intent, completion policy, evidence refs, and writable/runtime-only status.

This keeps three surfaces aligned without merging their responsibilities:

1. TUI small plan = foreground cockpit over selected plan/task projections.
2. OpenSpec/design = lifecycle source of truth.
3. Flynt task board = external board that can link to plan tasks and lifecycle artifacts without owning Omegon internals.

### Implementation sequence

Execute the remaining work in four slices. Each slice must leave `/plan list` and existing snapshot JSON backward-compatible.

#### Slice A — Registry core and stale-safe detach

Scope: `conversation.rs`, `main.rs`, focused tests.

- Add stable id constructor helpers for session, OpenSpec, design, hybrid, and branch plan ids.
- Extend `PlanBinding` with optional external task refs but keep default serde compatibility.
- Add `TaskCompletionPolicy`, `EvidenceRef`, `PlanEventSource`, `PlanEvent`, and `CompletionLedgerEntry` data shapes behind session-local state.
- Make `PlanAction::Clear` scope-aware: session plans clear; repo-bound plans become detached and preserve binding summary.
- Add stale/degraded copy helpers and tests for clear/detach not mutating OpenSpec/design files.

Acceptance: existing plan tests pass; new tests prove repo-bound clear detaches and never deletes durable artifacts.

#### Slice B — Read-only registry and task projections

Scope: `conversation.rs`, `tools/mod.rs`, `main.rs`, `lifecycle/design.rs`.

- Implement `PlanRegistry` read model from visible plan, last completed plan, OpenSpec task groups, design candidates, and branch/worktree hints.
- Convert visible items and OpenSpec task groups into `PlanItemProjection` rows with parent `plan_id`.
- Add external task refs to item projections, not Flynt-specific fields.
- Update `/plan list` and `/plan show` to render from registry/projected tasks.
- Mark missing backing files/nodes as `Stale` with last summary when available.

Acceptance: `/plan list` remains read-only, includes OpenSpec/design candidates, and exposes source/scope/status/progress without parsing durable files for mutation.

#### Slice C — Resume, background, ledger UX

Scope: `conversation.rs`, `main.rs`, `loop.rs`, TUI dashboard/Slim plan lane.

- Add session-local registry view state for backgrounded/detached/dismissed/last-visible/resume hints.
- Implement `/plan switch`, `/plan resume`, `/plan background`, `/plan detach`, `/plan ledger`, and `/plan show`.
- Add resume ranking: active foreground, backgrounded/blocked lifecycle-bound, incomplete OpenSpec/design, recent completed context.
- Record background/completion events without replacing the visible plan.
- Surface resume candidates in dashboard/Slim lane with explicit operator choice.

Acceptance: completed/backgrounded/stale plans never become foreground on startup without explicit `/plan switch` or `/plan resume`.

#### Slice D — ACP plan/task projection surfaces

Scope: `acp.rs`, `acp_worker.rs`, `conversation.rs`, tests.

- Advertise `_plans/*` and `_tasks/*` in `runtime/capabilities`.
- Implement `_plans/list`, `_plans/show`, `_plans/events`, `_plans/switch`, `_plans/detach`.
- Implement `_tasks/list`, `_tasks/show`, `_tasks/bind`, `_tasks/events`.
- Ensure read methods are mutation-free and safe for Flynt polling.
- Ensure `_tasks/bind` only stores external task refs on task projections/session metadata; no OpenSpec/design completion mutation.
- Add ACP tests for capability advertisement, read-only list/show, switch/detach semantics, and external task binding.

Acceptance: Flynt can render plans as task-composed rows through ACP without parsing slash text or owning lifecycle completion.

### Acceptance gates

- `/plan list` remains read-only and lists visible, backgrounded, completed, OpenSpec, design, and stale candidates with source/scope/status/progress.
- `/plan clear` on OpenSpec/design/hybrid state does not mutate lifecycle files.
- Backgrounded/completed plans never replace the visible plan without `/plan switch` or `/plan resume`.
- Stale backing artifacts are visible as stale, not silently resurrected as active foreground work.
- Research/design/validation/operations tasks can require evidence without requiring code diffs.

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

### Read-only plan list

`/plan list` is the first shared operator/harness registry surface. It does not mutate lifecycle artifacts and does not perform durable write-through. The output is intentionally plain text so it can serve both humans and remote slash execution:

```text
Plans

Visible
- session:current · session · Active · 1/3
  - ● inspect current plan state
  - ◐ wire list renderer

Completed
- last session plan · 2/2

OpenSpec
- plan-refinement · Implementing · 7/36
  - UX and Binding Semantics · 0/4
  - Runtime Model · 6/6
  - Lifecycle Projection · 1/3
```

Acceptance criteria:

- `/plan status` shows the foreground visible plan.
- `/plan list` lists visible session state and OpenSpec task-group summaries.
- `/plan list` is read-only: it may normalize legacy visible-plan state but must not edit OpenSpec task files.
- Remote slash execution accepts `/plan list` and returns the same text surface.
- OpenSpec task groups come from parsed `tasks.md`; no checkbox write-through is attempted.

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

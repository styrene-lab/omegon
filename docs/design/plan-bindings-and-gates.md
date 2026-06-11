# Plan Bindings and Execution Gates

## Intent

Plans serve two related purposes:

1. **Imperative execution surface** — the ordered steps the agent is executing now.
2. **Task-bound execution plan** — an ordered task list bound to a durable work item such as a kanban task, forge issue, OpenSpec task, or design node.

The durable artifact answers *what work exists*. The plan answers *how the agent is executing that work now*.

## Terminology

Use **bind / binding / bindings** throughout. Do not use “pin” terminology for plan-to-work-item mappings.

- **Primary binding**: the single durable work item whose completion policy governs the plan.
- **Related bindings**: additional lifecycle artifacts that receive evidence/status links.
- **Evidence**: semantic proof attached to plan steps and propagated through bindings.

A plan has at most one primary binding and any number of related bindings.

## Binding model

```rust
struct PlanRecord {
    id: PlanId,
    title: String,
    tab_title: String,
    status: PlanStatus,
    primary_binding: Option<PlanBinding>,
    related_bindings: Vec<PlanBinding>,
    items: Vec<PlanItem>,
    evidence: Vec<EvidenceRef>,
}
```

```rust
enum PlanBinding {
    Session,
    KanbanTask { task_id: String },
    ForgeIssue { engagement_id: String, repo: String, issue_id: String },
    OpenSpecTask { change: String, task_id: String },
    OpenSpecChange { change: String },
    DesignNode { node_id: String },
    GitBranch { branch: String },
    GitCommit { sha: String },
}
```

The primary binding determines completion policy. Related bindings receive evidence but do not decide whether the plan is complete.

## Plan gate

Non-trivial plans should pass through an explicit gate before large mutation sets:

```text
assess → draft plan → review required → approved → execute → evidence → complete
```

The agent owns plan creation and maintenance. The operator owns approval, priority, and course correction.

Approval is required for high-risk or high-cost work, including:

- multi-file architectural changes
- public API/data model changes
- OpenSpec/design-tree implementation
- auth/secrets/security changes
- plans with lifecycle side effects
- high-context execution where checkpoint/compact is recommended

Small reversible edits may auto-execute.

## Context checkpoint / clear

For large approved plans, the system should support:

```text
approve → checkpoint plan → compact/clear context → resume from plan → execute
```

The checkpoint must preserve:

- objective and constraints
- approved steps
- bindings
- acceptance checks
- current step
- evidence already collected
- operator approval evidence

The resume prompt should be generated from the plan record, not from lossy scrollback.

## Evidence loop

Plan steps complete only with evidence unless explicitly marked as planning-only. Evidence can include:

- tool call references
- file changes
- validation/test runs
- operator approval
- checkpoint records
- git commits
- lifecycle decisions

Execution flow:

```text
Plan step starts
  → tool calls bind to active step
  → tool results become evidence
  → step completes with evidence
  → plan completes when required steps complete
  → primary binding completion policy runs
  → related bindings receive evidence links
```

## `/plan execute`

`/plan execute` executes the active selected plan. If the plan has a primary binding, execution is a stateful evidence-producing transaction against that binding. If the plan is unbound and has lifecycle side effects, execution should request a binding or explicit session-only execution.

The command is an override/debug surface. Normal flow should be agent-driven: the agent creates, binds, requests approval, checkpoints if needed, and executes after approval.

## Terminal tab title

The active plan projection should expose a one-to-two word `tab_title` for terminal titles:

```text
? Plan Dock   review required
▶ Plan Dock   active
! Plan Dock   blocked
✓ Plan Dock   completed briefly
```

The tab title remains concise; bindings are shown in the Plan Dock, not the tab title.

## Workstreams, plans, cleave, and delegate

A **workstream** is the durable lifecycle envelope for a coherent unit of work. It may be bound to a forge issue, kanban task, OpenSpec change, design node, branch, or a combination of those artifacts.

A **plan** is an execution episode inside a workstream. Plans are expected to be numerous over the lifetime of a larger workstream: discovery plan, design plan, implementation plan, review/fix plan, release plan, and so on. Completed plans remain evidence. Paused or unapproved plans remain resumable intent.

`cleave` and `delegate` are execution mechanisms inside a plan episode. They do not own lifecycle state.

```text
Workstream
  owns: durable intent, bindings, branch/worktree, evidence ledger, completion policy

  Plan episode
    owns: ordered operator-visible execution intent, approval gate, step evidence

    cleave
      owns: decomposition of a plan step into child lanes and merge/salvage results

    delegate
      owns: one bounded child lane or subtask execution
```

### Cleave contract

`cleave` should run against a plan step or plan episode, not as a separate project lifecycle. Its output must attach evidence back to the active plan and owning workstream:

- child lane id
- child description and scope
- worktree/branch if created
- status: running, succeeded, failed, timed out, cancelled, salvaged
- validation evidence
- commit/merge evidence
- final summary

A cleave child can produce a completed plan item, a new follow-up plan item, or a blocking issue. It should not silently become its own durable owner.

### Delegate contract

`delegate` is the smaller execution primitive. A delegate task is a child lane under either:

- a plan item, or
- a cleave child lane.

Delegate evidence must include the model/profile used, scope, result, and any validation output. The parent plan/workstream decides whether that evidence is sufficient.

### Plan Dock projection

The Plan Dock should show one active plan episode in detail and summarize workstreams around it:

```text
active plan: implement status sidecar 2/4 · workstreams×3
1. done    inspect layout contract
2. active  patch status height
3. todo    validate focused tests

workstreams summary:
paused 3/8 issue #123 cleave retry cleanup
pending 0/6 issue #140 auth docs refresh
blocked 4/9 design node plan-gate substrate
```

The active plan remains the operator's immediate cockpit. Workstream summaries prevent loss of situational awareness without rendering every plan in full.

### Lifecycle transitions

A workstream can contain many plan episodes:

```text
issue/design node discovered
  → workstream created/bound
  → discovery plan approved/executed
  → design plan approved/executed
  → implementation plan approved/executed
      → cleave child lanes and delegate subtasks run inside this plan
  → review/fix plan approved/executed
  → validation/release plan approved/executed
  → workstream complete
```

When a plan completes, the workstream remains open unless its primary binding completion policy is satisfied.

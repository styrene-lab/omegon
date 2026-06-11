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

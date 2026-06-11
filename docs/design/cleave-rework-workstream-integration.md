+++
title = "Cleave Rework and Workstream Integration"
tags = ["design","cleave","workstream","execution"]
+++

# Cleave Rework and Workstream Integration

---
title: Cleave Rework and Workstream Integration
status: seed
tags: [design, cleave, workstream, execution]
---

# Cleave Rework and Workstream Integration

## Context

Cleave is still an early primitive for assessing/decomposing work into parallel children. As the execution model shifts toward durable [[plan-bindings-and-gates|workstreams, plan episodes, bindings, and evidence]], cleave should be revisited rather than treated as a standalone lifecycle mechanism.

Current framing:

```text
Workstream
  owns durable intent, binding, code lane, evidence ledger
  └─ Plan Episode
       owns approved tactical execution
       └─ Cleave Run
            decomposes the episode into parallel child lanes/tasks
```

## Problem

The existing cleave assessment/decomposition algorithm is too primitive for the intended workstream model. It likely lacks enough understanding of:

- primary/related bindings
- plan episode approval gates
- code lane ownership
- child lane/worktree/jj-change lifecycle
- evidence collection and propagation
- scope conflict detection
- dependency modeling between child tasks
- merge/checkpoint policy
- acceptance criteria derived from OpenSpec/design/issue context

Without rework, cleave risks creating orphan parallel work instead of subordinate execution inside a durable workstream.

## Desired direction

Cleave should become a **workstream-aware plan episode execution strategy**, not a separate lifecycle system.

A cleave run should know:

```rust
struct CleaveRunRecord {
    id: CleaveRunId,
    workstream_id: WorkstreamId,
    plan_episode_id: PlanEpisodeId,
    children: Vec<CleaveChildRecord>,
    status: CleaveRunStatus,
}
```

Each child should track:

```rust
struct CleaveChildRecord {
    label: String,
    scope: Vec<PathBuf>,
    code_lane: CodeLaneRef,
    depends_on: Vec<String>,
    status: ChildStatus,
    evidence: Vec<EvidenceRef>,
}
```

## Rework areas

### 1. Assessment algorithm

Revisit `cleave_assess` so it considers more than generic task complexity:

- number of files/components affected
- public API/data model risk
- OpenSpec/design-node bindings
- whether work has independent acceptance criteria
- whether scopes can be isolated safely
- whether child results can merge cleanly
- required validation evidence
- operator approval requirements

Output should explain *why* cleave is or is not appropriate.

### 2. Decomposition algorithm

Decomposition should group by semantic acceptance criteria, not only files.

Preferred grouping priorities:

1. Spec/domain scenario ownership
2. Design decision or implementation area
3. Independent code ownership boundary
4. Test/verification responsibility
5. File scope only as a fallback

Child plans should include:

- scope constraints
- acceptance criteria
- expected evidence
- dependency edges
- merge risk notes
- code lane policy

### 3. Code lane discipline

Cleave should integrate with the future Workstream Code Lane Contract:

- child worktrees/jj changes must be subordinate to the parent workstream
- repeated ensure operations must be idempotent
- child lanes must be cleaned/archived/merged deliberately
- dirty state must be attributed to the owning child/workstream

### 4. Evidence ledger

Cleave child results must write evidence back to the parent plan episode/workstream:

- files changed
- tests run
- validation output
- merge/checkpoint refs
- child failure/blocker summaries

### 5. Plan Dock projection

Plan Dock should project cleave activity under the active plan episode:

```text
active: issue #88 workstream registry 3/7
▶ 3. Implement registry persistence
   cleave: 4 children · 2 running · 1 done · 1 blocked
```

The instruments footer can show raw activity, but the Plan Dock should show why cleave is running.

## Open questions

- [ ] Should cleave remain a tool API with optional workstream metadata, or become a method on a WorkstreamExecutionService?
- [ ] What is the minimum viable workstream-aware cleave record persisted on disk?
- [ ] Should cleave create child code lanes itself or request them from a CodeLaneService?
- [ ] How should cleave recover from partial child completion after interruption/context clear?
- [ ] What merge/checkpoint policy should be default for git vs jj substrates?

## Initial recommendation

Do not extend the old primitive incrementally without a contract pass. First define:

1. workstream-aware cleave input/output schema
2. child lane lifecycle
3. evidence propagation model
4. Plan Dock projection shape
5. compatibility wrapper around the existing `cleave_run`

Then replace or deeply revise the assessment/decomposition algorithm behind that contract.

## Upstream substrate: PR 138

PR 138 (`feat(subagents): expose cleave delegate progress and execution evals`) has been merged into `main` and should be treated as the current substrate for this rework.

Relevant additions from that merge:

- richer cleave progress snapshots with child state, checklist/task progress, last tool/turn activity, PID/supervision state, token accounting, and terminal run state
- richer delegate progress snapshots with running/completed/failed state, last tool/turn activity, checklist progress, and terminal child result summaries
- deterministic injected-child execution evals for delegate success/failure/wall-clock timeout/idle timeout
- deterministic injected-child execution evals for cleave success/failure/wall-clock timeout/cancellation
- `omegon-git` worktree creation now explicitly creates the child branch before adding the worktree
- failed cleave child with no salvaged changes is reported as `Skipped` rather than successful `NoChanges`

These are not incidental implementation details. They should shape the rework contract.

## Vocabulary alignment

Use **execution lane** as the user-facing umbrella for subordinate execution contexts.

A cleave child is one bounded execution lane with:

- label
- scope
- model/profile
- worktree or jj change
- branch/bookmark/ref
- status
- merge outcome
- evidence

Delegate tasks may run in the parent lane for read-only/scout/verify work or in a child lane for isolated patch work.

## Test strategy

Any cleave assessment/decomposition rewrite must use the PR 138 deterministic execution harness before relying on live LLM/provider tests.

Required regression families:

- dependency-wave decomposition and scheduling
- failed-child salvage with useful work preserved
- cancellation without orphan child processes
- delegate cancellation if/when the cancellation seam is added
- child lane cleanup/archive behavior
- merge outcome semantics (`Merged`, `Skipped`, `NoChanges`, `Conflict`, salvage variants)

## Projection strategy

Do not invent a separate Plan Dock runtime model for subagent progress. Adapt the merged cleave/delegate status snapshots into the workstream projection:

```text
cleave/delegate status snapshot
  -> workstream execution adapter
  -> active plan episode progress/evidence
  -> Plan Dock projection
```

The instruments footer can continue showing raw activity telemetry. The Plan Dock should show why the subagent system is active and which workstream/plan step it advances.

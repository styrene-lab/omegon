---
title: Subagents, Cleave, and Delegate Operations
status: exploring
tags: [architecture, subagents, cleave, delegate, workbench, lifecycle]
---

# Subagents, Cleave, and Delegate Operations

## Overview

Assess the operational contract between Omegon's subagent mental model, `delegate` single-worker execution, and `cleave` batch decomposition. The goal is not to collapse them into one mechanism by default; it is to identify which guarantees must be shared so operators get predictable lifecycle state, progress, cancellation, provenance, and failure handling across all child-agent work.

## Definitions

| Term | Operational meaning | Current owner |
|---|---|---|
| Subagent | User-facing mental model: a task-scoped child worker with its own prompt/context/tools/model. | Design/UX layer; partially represented by `AgentSpec` in `features/delegate.rs`. |
| Delegate | On-demand single child task, usually async, with result retrieval and status. | `core/crates/omegon/src/features/delegate.rs`. |
| Cleave | Batch decomposition and parallel child execution over a plan, with dependency waves and merge outcomes. | `core/crates/omegon/src/features/cleave.rs` and `core/crates/omegon/src/cleave/orchestrator.rs`. |
| Cleave child | One scoped unit inside a cleave run, typically worktree-backed and merge-governed. | `core/crates/omegon/src/cleave/*`. |
| Workbench operation | Live visible progress projection for active plan/cleave/delegate work. | `core/crates/omegon/src/tui/workbench.rs` plus dashboard/runtime state. |

## Current Architecture Evidence

### Tool ownership

`core/crates/omegon/src/tool_registry.rs` already names the split:

- Cleave/decomposition tools: `cleave_assess`, `cleave_run`.
- Delegate/subagent tools: `delegate`, `delegate_result`, `delegate_status`.

This is useful because it makes the product-level distinction explicit: `cleave` is decomposition; `delegate` is subagent invocation.

### Cleave execution path

Primary files:

- `core/crates/omegon/src/features/cleave.rs`
- `core/crates/omegon/src/cleave/orchestrator.rs`
- `core/crates/omegon/src/cleave/progress.rs`
- `core/crates/omegon/src/cleave/state.rs`
- `core/crates/omegon/src/cleave/worktree.rs`

Cleave's orchestrator owns dependency waves, child process spawning, worktree state, cancellation tokens, progress events, and merge outcomes. It is a strong execution engine for batch work, but it carries more ceremony than a one-off subagent request should need.

### Delegate execution path

Primary file:

- `core/crates/omegon/src/features/delegate.rs`

Delegate owns `AgentSpec`, `DelegateTask`, `DelegateTaskStatus`, `DelegateResultStore`, `DelegateProgress`, duplicate-task checks, background result retrieval, failure classification, and the child prompt/run path. It shares child-agent primitives with cleave but is not merely a thin wrapper over cleave.

### Workbench/progress path

Primary files:

- `core/crates/omegon/src/tui/workbench.rs`
- `core/crates/omegon/src/tui/mod.rs`
- `core/crates/omegon/src/tui/tests.rs`

Existing tests verify active cleave and active delegate states route into the Workbench without requiring the old instruments panel. This is the right direction: child-agent operations should be visible as first-class work, not hidden behind tool-result prose.

## Lifecycle Comparison

| Phase | Delegate | Cleave | Shared contract needed? |
|---|---|---|---|
| Intent capture | One scoped task, optional agent/profile/scope/model. | Directive plus plan with children/dependencies. | Yes: canonical operation summary and scope disclosure. |
| Start | Stores `DelegateTask`, spawns child task. | Creates/resumes `CleaveState`, dispatches wave children. | Yes: operation ID, child IDs, started timestamp. |
| Running progress | `DelegateProgressChild` with status, last tool/turn, checklist heuristic. | Cleave progress events/child statuses/waves. | Yes: common child progress projection for Workbench. |
| Cancellation | Delegate has task runtime cancellation/failure paths; slash/control surface is status-focused. | Cleave has run cancel and child cancel paths. | Yes: cancellation semantics and operator-facing status should align. |
| Completion | Result stored and retrieved with `delegate_result`; background notifications. | Merge outcomes and run result state. | Yes: completion summary, result/provenance pointer. |
| Failure | Failure classifier, repeated failure hard-disable. | Child failure kinds, timeout/upstream retry/merge conflict outcomes. | Yes: failure taxonomy and remediation rows. |
| Provenance | Task/result store and child output summary. | State file, worktree paths, merge outcomes, progress. | Yes: durable enough to inspect what child did and why. |

## Adversarial Findings

### 1. Do not force delegate through cleave just for conceptual purity

The subagent design doc says cleave infrastructure can power delegate. Code evidence shows delegate and cleave are separate features sharing lower-level child-agent primitives. That separation is not inherently bad. A one-off read-only scout delegate should not inherit all cleave ceremony: waves, merge policy, plan JSON, or worktree lifecycle unless it needs mutation isolation.

**Decision pressure:** unify contracts and projections first, not execution engines.

### 2. The product vocabulary is ahead of the runtime vocabulary

Operators think in subagents. Code mostly thinks in `delegate` tasks and `cleave` children. `AgentSpec` exists, but the named-agent/subagent concept is not yet the single visible abstraction across CLI/TUI/control surfaces.

**Risk:** docs/prompts may say “subagent” while status surfaces say “delegate” or “cleave child,” making progress and failures feel like different systems.

### 3. Progress shapes are similar but not obviously shared

Delegate and cleave both expose child progress fields: label/status/last tool/task checklist. They are represented by separate structs (`DelegateProgressChild`, cleave progress/state child types). This can be acceptable internally, but the Workbench should consume a shared projection to avoid drift.

**Risk:** Workbench polish fixes one operation type and misses the other.

### 4. Cancellation parity needs explicit review

Cleave has explicit child cancellation control surfaces (`cleave_cancel_child_response`, `/cleave cancel ...`). Delegate has status/result tools and runtime failure handling, but the operator-facing cancellation contract is less visible from the initial scan.

**Risk:** background delegates become stuck or invisible compared with cleave children.

### 5. Failure taxonomy is duplicated

Delegate has failure classification and a hard-disable after repeated failures. Cleave has child failure kinds, provider exhaustion handling, timeout paths, and merge outcomes. These likely evolved separately.

**Risk:** same underlying failure produces different remediation text depending on whether it happened in delegate or cleave.

### 6. Scope/sandbox guarantees should be stated per operation

Cleave's strongest value is worktree/scope/merge isolation. Delegate has scope in the tool prompt and child runtime, and may use sandboxing, but the exact mutation guarantees are not as operator-obvious.

**Risk:** “delegate” is perceived as safe subagent isolation while actually being policy/prompt constrained unless runtime isolation is enabled for that mode.

## Assessment Matrix

| Area | Current confidence | Evidence | Needed next check |
|---|---:|---|---|
| Tool registration split | High | `tool_registry.rs` cleave/delegate modules. | None. |
| Workbench visibility | Medium-high | Tests for active cleave/delegate Workbench routing. | Confirm common projection/height behavior under mixed active states. |
| Delegate result lifecycle | Medium | `DelegateResultStore`, `delegate_result`, background notifications. | Test status/result after timeout, duplicate, failed, and successful tasks. |
| Cleave cancellation | Medium-high | Control runtime has cleave status/cancel handlers. | Verify UI/control parity and stale progress cleanup. |
| Delegate cancellation | Low-medium | Initial scan did not surface a first-class slash/control cancel command. | Inspect whether cancellation exists, then decide if `/delegate cancel <id>` is required. |
| Shared failure remediation | Low | Separate failure classifiers. | Map delegate and cleave failures to a common operator-facing taxonomy. |
| Named subagent UX | Low-medium | `AgentSpec`, docs describe `.omegon/agents/*.md`. | Verify loader, listing, validation, and invocation path. |

## Open Questions

- [assumption] Delegate and cleave should remain separate top-level tools while sharing lower-level progress/failure/status projections.
- [assumption] A “subagent” is a UX/product abstraction over delegate-style execution, not a third execution engine.
- Should `/delegate cancel <task_id>` exist to match `/cleave cancel <child>`?
- Should Workbench consume a unified `ChildOperationProgress` projection for both delegate and cleave?
- Should failure remediation be centralized so provider exhaustion, timeout, sandbox spawn failure, and scope errors render consistently?
- Which delegate modes require worktree isolation by default: read-only scout, patch, verify, or only write-capable agents?
- What durable provenance is required for a completed delegate: prompt file, model, cwd, scope, tool transcript, diff, final result?

## Proposed Workstream Plan

1. **Surface map**
   - Inventory slash/control/tool paths for `delegate`, `delegate_result`, `delegate_status`, `cleave status`, and `cleave cancel`.
   - Record whether each is TUI-only, control-runtime safe, ACP-visible, or tool-only.

2. **Projection contract**
   - Define a renderer-neutral child-operation projection with operation kind, child ID, label, status, last activity, task checklist, failure kind, result summary, and remediation.
   - Adapt Workbench to consume that projection only if current structs are already drifting.

3. **Cancellation parity assessment**
   - Verify delegate cancellation support.
   - If absent, propose `/delegate cancel <task_id>` and control action metadata.

4. **Failure taxonomy assessment**
   - Compare delegate classifier with cleave child failure handling.
   - Extract common operator-facing failure categories if duplication is material.

5. **Named subagent assessment**
   - Verify `.omegon/agents/*.md` load/validation/invocation behavior.
   - Decide whether `AgentSpec` should be promoted to a shared subagent registry type.

## First Implementation Targets If Findings Hold

1. Add missing delegate cancellation/status parity tests.
2. Add a shared Workbench projection test that renders equivalent delegate and cleave child progress without bespoke layout drift.
3. Add a failure-remediation unit test table covering delegate and cleave provider/timeout/scope errors.
4. Add named-agent validation tests for malformed `.omegon/agents/*.md` specs if loader exists.

## Surface Map Update — Cancellation Parity

### Cleave cancellation is first-class

Evidence:

- `core/crates/omegon/src/control_actions.rs` classifies `/cleave cancel <label>` as `CanonicalAction::CleaveCancelChild`, `ControlRole::Edit`, and remote-safe.
- `core/crates/omegon/src/control_runtime.rs` handles `ControlRequest::CleaveCancelChild { label }` via `cleave_cancel_child_response(...)`.
- `core/crates/omegon/src/features/cleave.rs` implements `/cleave [status|cancel <label>]` and calls `cancel_child(label)`.
- Existing tests cover token-backed and persisted-PID fallback cancellation.

Assessment: cleave cancellation is present across slash classification, control runtime, feature handling, and tests.

### Delegate cancellation is not first-class yet

Evidence:

- `core/crates/omegon/src/features/delegate.rs` exposes `delegate`, `delegate_result`, and `delegate_status`.
- `core/crates/omegon/src/control_actions.rs` recognizes `/delegate` and `/delegate status` only as `CanonicalAction::DelegateStatus`, `ControlRole::Read`, remote-safe.
- `core/crates/omegon/src/control_runtime.rs` handles `ControlRequest::DelegateStatus`, but no delegate cancel request was found.
- Search found no `delegate cancel`, `DelegateCancel`, `DELEGATE_CANCEL`, or equivalent control/action route.

Assessment: background delegate tasks can be inspected and results can be retrieved, but the operator-facing cancellation contract does not match cleave. This is a real operational parity gap.

### Implementation target: `/delegate cancel <task_id>`

Minimum coherent change:

1. Add a canonical action for delegate cancellation with `ControlRole::Edit` and remote-safe classification.
2. Extend slash classification so `/delegate cancel <task_id>` is recognized separately from `/delegate status`.
3. Add a control request/runtime response for delegate cancellation.
4. Store or expose per-delegate cancellation tokens in `DelegateFeature`/runner state.
5. Implement feature command handling: `/delegate cancel <task_id>`.
6. Add tests mirroring cleave cancellation coverage:
   - slash classification is edit + remote-safe
   - cancelling a running delegate marks/cancels the task
   - cancelling an unknown delegate returns a clear not-found message
   - cancelled delegate status/result uses a stable failure/cancelled status, not a generic failure string

Open design choice: whether cancellation should be represented as its own `DelegateTaskStatus::Cancelled` variant or as `Failed { error: "cancelled" }`. A first-class `Cancelled` variant is cleaner for Workbench and result surfaces because cancellation is operator intent, not child failure.

## Operation Selection Update — Delegate for Side Quests, Cleave for Coordinated Subagent Work

Operator assessment: most quick side quests should use `delegate` as a one-shot subagent task. `cleave` can technically do everything delegate can do, but its distinguishing value is coordination: multiple Omegon instances, separate worktrees, dependency waves, merge policy, and cross-child result harvesting.

### Refined mental model

| Need | Preferred operation | Reason |
|---|---|---|
| Quick bounded side quest | `delegate` | One subagent, low ceremony, scoped result, parent continues. |
| Read-only scouting while parent works | `delegate` with `worker_profile: scout` | Keeps parent context clean and avoids broad mutation risk. |
| Focused mechanical edit | `delegate` with `worker_profile: patch` | Good when decision is already made and file scope is tight. |
| Focused validation/check run | `delegate` with `worker_profile: verify` | Lets checks run while parent assesses next step. |
| Multi-track implementation | `cleave_run` | Coordinates multiple children across worktrees and merges. |
| Branch integration/merge work, even if phrased as “use subagents” | `cleave_assess`, then `cleave_run` if split-worthy | The operator is asking for subordinate work, but merge sequencing, conflict isolation, and cross-branch synthesis are cleave-shaped. |
| Work requiring dependency ordering | `cleave_run` | Plan JSON can express `depends_on` and wave execution. |
| One child only | Usually `delegate`, not cleave | Cleave adds orchestration overhead without coordination benefit. |
| Architectural judgment or unresolved design choice | Parent agent directly | Delegation should execute bounded tasks, not outsource ownership. |

### Policy finding

The current agent behavior overuses `cleave_assess` because it is visible as a safe decision gate, while `delegate` and `cleave_run` lack equally concrete trigger guidance. This causes assessment without operational follow-through.

The skill/prompt layer should teach this hierarchy:

1. Operator wording such as “use subagents” means “use subordinate execution”; it does not force the `delegate` tool.
2. `cleave_assess` is a gate for whether decomposition is warranted.
3. `delegate` is the default side-quest/subagent operation.
4. `cleave_run` is for coordinated multi-subagent execution, not routine one-off help.
5. The parent remains accountable for synthesis, validation, and final claims.

### Bundled skill / prompt surface recommendation

Add a “Subagent Operations” section to bundled skills and prompt guidance. It should include:

- Definitions:
  - `delegate` = one-shot subagent side quest.
  - `cleave` = coordinated multi-subagent work across isolated worktrees.
  - `cleave_assess` = decomposition gate, not execution.
- Trigger rules:
  - treat “use subagents” as subordinate-execution intent, then choose the primitive by task shape;
  - use delegate scout/patch/verify for bounded side quests;
  - use cleave for branch integration, dependency ordering, merge governance, or any 2+ independent/coordinated child tasks;
  - do not cleave a one-child task unless worktree/merge isolation is explicitly needed.
- Anti-patterns:
  - calling `cleave_assess` repeatedly without acting on its result;
  - delegating vague repo-wide archaeology;
  - spawning duplicate delegates for the same task;
  - claiming completion before retrieving/reconciling delegate results.
- Examples:
  - scout delegate with explicit file scope;
  - verify delegate for focused tests;
  - cleave_run JSON skeleton with two children and dependency labels.

### Design decision candidate

Delegate and cleave should remain separate operator concepts even if they share lower-level child-agent primitives:

- **Delegate** optimizes for low-friction side quests.
- **Cleave** optimizes for coordination, isolation, and merge governance across multiple subagents.

The unification target should be projection/progress/failure/cancellation semantics, not necessarily one execution engine.

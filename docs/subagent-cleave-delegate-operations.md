---
title: Subagents, Cleave, and Delegate Operations
status: exploring
tags: [architecture, subagents, cleave, delegate, workbench, lifecycle]
---

# Subagents, Cleave, and Delegate Operations

## Overview

Assess the operational contract between Omegon's subagent mental model, `delegate` single-worker execution, and `cleave` batch decomposition. The goal is not to collapse them into one mechanism by default; it is to identify which guarantees must be shared so operators get predictable lifecycle state, progress, cancellation, provenance, and failure handling across all clove/subagent work.

## Definitions

| Term | Operational meaning | Current owner |
|---|---|---|
| Subagent | User-facing mental model: a task-scoped clove worker with its own prompt/context/tools/model. | Design/UX layer; partially represented by `AgentSpec` in `features/delegate.rs`. |
| Delegate | On-demand single clove task, usually async, with result retrieval and status. | `core/crates/omegon/src/features/delegate.rs`. |
| Cleave | Batch decomposition and parallel clove execution over a plan, with dependency waves and merge outcomes. | `core/crates/omegon/src/features/cleave.rs` and `core/crates/omegon/src/cleave/orchestrator.rs`. |
| Clove | One scoped unit inside a cleave run, typically worktree-backed and merge-governed. | `core/crates/omegon/src/cleave/*`. |
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

Delegate owns `AgentSpec`, `DelegateTask`, `DelegateTaskStatus`, `DelegateResultStore`, `DelegateProgress`, duplicate-task checks, background result retrieval, failure classification, and the child prompt/run path. It shares clove-agent primitives with cleave but is not merely a thin wrapper over cleave.

### Workbench/progress path

Primary files:

- `core/crates/omegon/src/tui/workbench.rs`
- `core/crates/omegon/src/tui/mod.rs`
- `core/crates/omegon/src/tui/tests.rs`

Existing tests verify active cleave and active delegate states route into the Workbench without requiring the old instruments panel. This is the right direction: clove/subagent operations should be visible as first-class work, not hidden behind tool-result prose.

## Lifecycle Comparison

| Phase | Delegate | Cleave | Shared contract needed? |
|---|---|---|---|
| Intent capture | One scoped task, optional agent/profile/scope/model. | Directive plus plan with cloves/dependencies. | Yes: canonical operation summary and scope disclosure. |
| Start | Stores `DelegateTask`, spawns clove task. | Creates/resumes `CleaveState`, dispatches wave cloves. | Yes: operation ID, clove IDs, started timestamp. |
| Running progress | `DelegateProgressChild` with status, last tool/turn, checklist heuristic. | Cleave progress events/clove statuses/waves. | Yes: common clove progress projection for Workbench. |
| Cancellation | Delegate has task runtime cancellation/failure paths; slash/control surface is status-focused. | Cleave has run cancel and child cancel paths. | Yes: cancellation semantics and operator-facing status should align. |
| Completion | Result stored and retrieved with `delegate_result`; background notifications. | Merge outcomes and run result state. | Yes: completion summary, result/provenance pointer. |
| Failure | Failure classifier, repeated failure hard-disable. | Child failure kinds, timeout/upstream retry/merge conflict outcomes. | Yes: failure taxonomy and remediation rows. |
| Provenance | Task/result store and clove output summary. | State file, worktree paths, merge outcomes, progress. | Yes: durable enough to inspect what child did and why. |

## Adversarial Findings

### 1. Do not force delegate through cleave just for conceptual purity

The subagent design doc says cleave infrastructure can power delegate. Code evidence shows delegate and cleave are separate features sharing lower-level clove-agent primitives. That separation is not inherently bad. A one-off read-only scout delegate should not inherit all cleave ceremony: waves, merge policy, plan JSON, or worktree lifecycle unless it needs mutation isolation.

**Decision pressure:** unify contracts and projections first, not execution engines.

### 2. The product vocabulary is ahead of the runtime vocabulary

Operators think in subagents. Code mostly thinks in `delegate` tasks and `cleave` cloves. `AgentSpec` exists, but the named-agent/subagent concept is not yet the single visible abstraction across CLI/TUI/control surfaces.

**Risk:** docs/prompts may say “subagent” while status surfaces say “delegate” or “cleave clove,” making progress and failures feel like different systems.

### 3. Progress shapes are similar but not obviously shared

Delegate and cleave both expose clove progress fields: label/status/last tool/task checklist. They are represented by separate structs (`DelegateProgressChild`, cleave progress/state child types). This can be acceptable internally, but the Workbench should consume a shared projection to avoid drift.

**Risk:** Workbench polish fixes one operation type and misses the other.

### 4. Cancellation parity needs explicit review

Cleave has explicit clove cancellation control surfaces (`cleave_cancel_child_response`, `/cleave cancel ...`). Delegate has status/result tools and runtime failure handling, but the operator-facing cancellation contract is less visible from the initial scan.

**Risk:** background delegates become stuck or invisible compared with cleave cloves.

### 5. Failure taxonomy is duplicated

Delegate has failure classification and a hard-disable after repeated failures. Cleave has clove failure kinds, provider exhaustion handling, timeout paths, and merge outcomes. These likely evolved separately.

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

## Autonomy and authority target

Subagent autonomy is an operator-selected authority profile, not prompt style. The user-facing knob may be named `manual`, `conservative`, `autonomous`, `orchestrator`, or `batch`, but runtime behavior must derive from an explicit policy that controls whether the harness may spawn cloves, mutate through cloves, create worktrees, spend model budget, use OCI/sandboxed substrates, and commit/reconcile without asking.

Core invariant:

> If the agent asks, the harness asks structurally. If the harness allows, the policy allowed it. If the policy denies, prompt text cannot override it.

### Target authority flow

```text
operator intent
  -> autonomy / authority policy
  -> prompt pressure + tool availability + command safety
  -> approval / modal / required-input system
  -> execution
  -> Workbench / operation projection / audit trail
```

### Policy presets

| Preset | Delegate | Cleave | Approval posture |
|---|---|---|---|
| `manual` | Never unless explicitly requested. | Never unless explicitly requested. | Ask before any subagent execution. |
| `conservative` | Bounded scout/verify allowed; mutating patch may ask. | `cleave_assess` may be used, but `cleave_run` asks. | Default interactive posture. |
| `autonomous` | Bounded scout/patch/verify allowed within scope. | Run when assessment justifies 2+ concrete child scopes. | Ask for budget/cloud/secrets/destructive escalations. |
| `orchestrator` | Proactive side quests, review, verification. | Proactive decomposition, dispatch, merge, verification, reconciliation. | Ask only for strategic/high-risk decisions. |
| `batch` | Queue/task-policy governed. | Queue/task-policy governed. | Non-interactive policy grants only; no conversational consent. |

### Runtime enforcement target

Prompt assembly must describe the active autonomy profile, but tool handlers and command surfaces must enforce it. `cleave_run` in conservative mode should return a structured approval requirement, not rely on the assistant writing “should I proceed?” in prose. The approval payload should include child count, max parallelism, scopes, mutation rights, worktree effects, model/runtime budget risk, OCI/sandbox posture, and choices such as approve once, approve class for session, or deny.

This policy should reuse existing surfaces rather than invent a separate prompt exchange:

- command registry `CommandSafety` / confirmation metadata;
- TUI command modal / command surface rendering;
- ACP required-confirmation path;
- required-input kinds such as approval and permission;
- Workbench operation projection for approved/running/completed subagent work.

### Second-order design constraints

- Tool availability is not permission. The prompt must not imply that `delegate`/`cleave` should be used aggressively merely because they are in the tool schema.
- Higher autonomy should generally imply stronger execution boundaries: OCI clove execution should be preferred or required for orchestrator/batch modes once the substrate is production-ready.
- Delegate is the normal accelerator for one bounded side quest. Cleave remains the coordinated multi-child/worktree orchestration primitive.
- Budget and model routing are part of autonomy. Fanout to paid/cloud cloves should require policy approval unless the profile explicitly grants it.
- Approval grants should be scoped and expiring, e.g. “approve cleave up to 3 cloves for this session,” not global “always allow everything.”

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
   - Compare delegate classifier with cleave clove failure handling.
   - Extract common operator-facing failure categories if duplication is material.

5. **Named subagent assessment**
   - Verify `.omegon/agents/*.md` load/validation/invocation behavior.
   - Decide whether `AgentSpec` should be promoted to a shared subagent registry type.

## First Implementation Targets If Findings Hold

1. Add missing delegate cancellation/status parity tests.
2. Add a shared Workbench projection test that renders equivalent delegate and cleave clove progress without bespoke layout drift.
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

Open design choice: whether cancellation should be represented as its own `DelegateTaskStatus::Cancelled` variant or as `Failed { error: "cancelled" }`. A first-class `Cancelled` variant is cleaner for Workbench and result surfaces because cancellation is operator intent, not clove failure.

## Operation Selection Update — Delegate for Side Quests, Cleave for Coordinated Subagent Work

Operator assessment: most quick side quests should use `delegate` as a one-shot subagent task. `cleave` can technically do everything delegate can do, but its distinguishing value is coordination: multiple Omegon instances, separate worktrees, dependency waves, merge policy, and cross-child result harvesting.

### Refined mental model

| Need | Preferred operation | Reason |
|---|---|---|
| Quick bounded side quest | `delegate` | One subagent, low ceremony, scoped result, parent continues. |
| Read-only scouting while parent works | `delegate` with `worker_profile: scout` | Keeps parent context clean and avoids broad mutation risk. |
| Focused mechanical edit | `delegate` with `worker_profile: patch` | Good when decision is already made and file scope is tight. |
| Focused validation/check run | `delegate` with `worker_profile: verify` | Lets checks run while parent assesses next step. |
| Multi-track implementation | `cleave_run` | Coordinates multiple cloves across worktrees and merges. |
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
  - use cleave for branch integration, dependency ordering, merge governance, or any 2+ independent/coordinated clove tasks;
  - do not cleave a one-clove task unless worktree/merge isolation is explicitly needed.
- Anti-patterns:
  - calling `cleave_assess` repeatedly without acting on its result;
  - delegating vague repo-wide archaeology;
  - spawning duplicate delegates for the same task;
  - claiming completion before retrieving/reconciling delegate results.
- Examples:
  - scout delegate with explicit file scope;
  - verify delegate for focused tests;
  - cleave_run JSON skeleton with two cloves and dependency labels.

### Design decision candidate

Delegate and cleave should remain separate operator concepts even if they share lower-level clove-agent primitives:

- **Delegate** optimizes for low-friction side quests.
- **Cleave** optimizes for coordination, isolation, and merge governance across multiple subagents.

The unification target should be projection/progress/failure/cancellation semantics, not necessarily one execution engine.

## Operation Reporting Paradigm — Parent/Clove Boundary Skeleton

### Core claim

Subagent reporting should be modeled as an explicit operation protocol between a parent Omegon runtime and one or more child Omegon runtimes. The child is another Omegon binary, so the interface does not need to be inferred from prose, tool-call transcript text, or generic lifecycle events. The parent should receive structured operation reports, reduce them into canonical operation state, and project that state into transcript, Workbench, statusline, and detail panes.

### Layer model

1. **Child Omegon runtime** — actual child process: model loop, tools, cwd, environment, sandbox/worktree.
2. **Child operation event stream** — structured events such as operation started, child started, tool activity, progress, failure, result.
3. **Parent operation state reducer** — canonical `OperationState`, `CloveState`, `FailureState`, result refs, counts, timestamps.
4. **Semantic display projections** — transcript milestones, Workbench live rows, detail-pane model, statusline summary.
5. **Renderers** — TUI/CLI/ACP/Web convert projections into glyphs, colors, wrapping, and actions.

The current failure mode comes from bypassing layers 2–4: low-level child/decomposition events are rendered directly as TUI text, causing delegate operations to leak `Cleave: 1 cloves dispatched`.

### Operation is the primary display unit

The display model should start with operation identity, not tool-call identity.

```rust
enum OperationKind {
    Delegate,
    Cleave,
}

struct OperationState {
    id: OperationId,
    kind: OperationKind,
    label: String,
    intent: String,
    status: OperationStatus,
    started_at: DateTime,
    updated_at: DateTime,
    cloves: Vec<CloveState>,
    result: Option<OperationResult>,
    failure: Option<OperationFailure>,
}
```

A `delegate` is an operation created by a tool call. A `cleave` is also an operation created by a tool call. The tool call is not the durable status model.

### Shared child row contract

Delegate and cleave may keep separate execution engines, but their live rows should reduce to a shared child shape:

```rust
struct CloveState {
    id: ChildId,
    operation_id: OperationId,
    operation_kind: OperationKind,
    label: String,
    role: Option<WorkerRole>,
    model: Option<String>,
    cwd: Option<PathBuf>,
    status: ChildStatus,
    last_activity: Option<ChildActivity>,
    progress: Option<ChildProgress>,
    failure: Option<OperationFailure>,
    result_ref: Option<ResultRef>,
}

enum ChildStatus {
    Queued,
    Starting,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}
```

Workbench headers can remain operation-specific, but rows should use this common projection. This prevents delegate and cleave from drifting in glyphs, failure wording, details affordances, and last-activity display.

### Child operation event protocol

Because the child binary is ours, the parent/child boundary should be structured JSONL or IPC, not parsed prose. A minimal protocol can be introduced behind the current runner first:

```json
{"type":"operation.started","operation_id":"delegate_3","kind":"delegate","intent":"prove subagent surfaces","cwd":"/repo","model":"gpt-5.5"}
{"type":"child.started","operation_id":"delegate_3","child_id":"delegate_3","role":"scout"}
{"type":"child.activity","child_id":"delegate_3","activity":{"kind":"tool","name":"bash","summary":"cargo test -p omegon delegate"}}
{"type":"child.progress","child_id":"delegate_3","message":"inspecting TUI workbench projection","tasks_done":1,"tasks_total":3}
{"type":"child.failed","child_id":"delegate_3","failure":{"kind":"idle_timeout","message":"no output for 120s","recoverable":true}}
{"type":"operation.completed","operation_id":"delegate_3","status":"failed"}
```

Cleave emits the same vocabulary with `kind: "cleave"`, multiple cloves, wave metadata, and merge/result metadata.

Control events and prose output must not share the same stream. The child should write operation events to a dedicated pipe/fd or IPC channel; normal stdout/stderr remain logs/tool output and are summarized, not treated as state.

### Projection responsibilities

- **Transcript projection:** durable milestones only — operation started, child failed, result ready, operation completed. It should not mirror every live Workbench row.
- **Workbench projection:** live state — running/done/failed counts, child rows, last tool/activity, failure reason, detail affordance.
- **Detail projection:** forensic view — prompt path, cwd, model, tool transcript, result text, failure payload, merge/worktree data.
- **Statusline projection:** compact aggregate — `running delegate · 2 active · 1 failed` or `running cleave · wave 2/3 · 4 active`.

### Failure taxonomy

Delegate and cleave failures should map to shared operator-facing kinds:

```rust
enum OperationFailureKind {
    IdleTimeout,
    ProcessExit,
    ModelError,
    ToolPermissionDenied,
    ToolExecutionFailed,
    SandboxViolation,
    MergeConflict,
    CancelledByOperator,
    DuplicateTask,
    Unknown,
}
```

The screenshot failure `Delegate idle timeout — no output for 120s` should render consistently as:

- Transcript: `✗ delegate_2 failed · idle timeout — no output for 120s`
- Workbench: `✗ delegate_2 · idle timeout 120s · ⌃O details`
- Detail: full failure payload and remediation.

### First implementation slice

1. Add a renderer-neutral operations projection module, e.g. `core/crates/omegon/src/surfaces/operations.rs`.
2. Map `DelegateProgress` into an `OperationWorkbenchProjection`.
3. Map cleave progress/state into the same row projection.
4. Make Workbench render operation projections rather than bespoke delegate/cleave rows.
5. Stop delegate-originated child dispatch from rendering as `Cleave: 1 cloves dispatched` in transcript.
6. Later, formalize child→parent JSONL/IPC event streaming.

### Design decision candidate

Unify projection/state semantics before unifying execution engines. Delegate and cleave may remain separate tools and runners; the shared contract should be operation identity, child row state, failure taxonomy, cancellation semantics, and result provenance.

## Reality Check — Current Codebase Assessment

### Evidence: delegate emits cleave/decomposition events today

Search shows `core/crates/omegon/src/features/delegate.rs` emits:

- `AgentEvent::FamilyVitalSignsUpdated` around delegate progress (`features/delegate.rs` near current search hit line ~1283).
- `AgentEvent::DecompositionStarted` from delegate execution (`features/delegate.rs` near current search hit line ~1573).
- `AgentEvent::DecompositionChildCompleted` on delegate completion (`features/delegate.rs` near current search hit line ~1864).

Cleave also emits the same `AgentEvent::Decomposition*` variants from `core/crates/omegon/src/features/cleave.rs`.

This directly explains the screenshot: delegate uses decomposition event variants, and the TUI interprets those as cleave.

### Evidence: TUI hardcodes decomposition as cleave

`core/crates/omegon/src/tui/mod.rs` handles:

```rust
AgentEvent::DecompositionStarted { children } => {
    self.conversation.push_lifecycle(
        "⚡",
        &format!("Cleave: {} cloves dispatched", cloves.len()),
    );
}
```

That is structurally wrong once delegate emits `DecompositionStarted`. The event name lacks operation provenance, and the renderer compensates by assuming cleave.

### Evidence: Workbench has separate delegate/cleave render paths

`core/crates/omegon/src/tui/workbench.rs` has separate paths such as `render_delegate_workbench_panel(...)`, while `WorkbenchState` carries `delegate: Option<DelegateProgress>`. This is acceptable as an intermediate state, but it confirms the lack of a shared child-operation projection.

Current delegate Workbench rows are built from `DelegateProgressChild` and can show running/failed counts, but failure reason/detail affordance is not obviously present in the row contract. That matches the screenshot where Workbench says `delegate_2 failed` while the useful reason appears elsewhere.

### Evidence: event vocabulary is already externalized elsewhere

`core/crates/omegon/src/mqtt_bridge.rs` maps `AgentEvent::DecompositionStarted` to `IpcEventPayload::DecompositionStarted`. ACP/Web/MQTT consumers can therefore inherit the same semantic conflation if the event is reused for delegate.

This means a TUI-only string replacement would be insufficient. The durable fix is provenance-bearing operation events/projections.

## Adversarial Assessment

### Finding 1: `AgentEvent::DecompositionStarted` cannot remain the user-facing event for delegate

If delegate continues to emit `DecompositionStarted`, every downstream consumer must guess whether it means cleave or delegate. The code already proves at least one consumer guesses wrong.

**Required correction:** either add operation provenance to the event or introduce distinct operation events. A local TUI guard based on the active tool would be a tactical patch only.

### Finding 2: Workbench projection should be unified before JSONL child protocol

The JSONL child protocol is architecturally cleaner, but the immediate codebase already has enough state to fix the display seam:

- `DelegateProgress` exists.
- Cleave progress/state exists.
- Workbench is already a semantic surface.

Implementing `surfaces::operations` as an adapter layer is lower risk than changing child process protocol first.

### Finding 3: transcript and Workbench must have different responsibilities

The current transcript receives lifecycle spam (`Cleave: 1 cloves dispatched`) while Workbench also reports live delegate state. This duplicates and conflicts. Transcript should record milestones; Workbench should own live aggregate state.

**Required correction:** operation projections should include both milestone events and live rows, and TUI should route them to the correct surface.

### Finding 4: failure reason is currently not first-class in the Workbench row

The screenshot proves failure reason is visible in a transcript/tool row but not in the Workbench summary. A shared `OperationFailure` on child rows would fix this and improve both delegate and cleave.

### Finding 5: IPC/Web surfaces are likely affected

Because `mqtt_bridge.rs` exports decomposition payloads, any ACP/Web/MQTT dashboard consuming those events may also mislabel delegate-originated child work. The projection design must be shared outside TUI.

### Finding 6: one-child cleave wording should be treated as a smell, not just a string

If `kind=Delegate`, one clove is normal. If `kind=Cleave`, one clove should be rare and should only appear when isolation/merge governance was explicitly requested. Projection tests should catch `delegate -> Cleave: 1 cloves dispatched` as invalid.

## New Open Questions

- [assumption] Delegate and cleave can remain separate execution engines while sharing operation projections.
- [assumption] Existing `DelegateProgress` and cleave state are sufficient for the first projection adapter without changing child process protocol.
- Should `AgentEvent::DecompositionStarted` be deprecated in favor of `AgentEvent::OperationStarted { kind, operation_id, children }`, or kept for cleave-only compatibility?
- What is the minimum failure payload currently available from `DelegateProgressChild` and cleave state, and where should missing failure reasons be captured?
- Which non-TUI consumers currently treat `DecompositionStarted` as cleave, especially ACP, MQTT, and dashboard paths?
- Should the first implementation suppress delegate decomposition transcript rows, or should it introduce the operation projection and migrate Workbench/transcript together?

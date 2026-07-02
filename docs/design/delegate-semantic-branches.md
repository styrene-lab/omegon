+++
title = "Delegate Semantic Branches and Workbench Convergence"
tags = ["design","delegate","workbench","ux"]
+++

# Delegate Semantic Branches and Workbench Convergence

# Delegate Semantic Branches and Workbench Convergence

## Problem

Sequential delegate IDs such as `delegate_7` are useful as stable machine addresses, but they do not carry enough meaning for long sessions. Once a session has many delegates, rows become hard to scan and easy to confuse.

There is also a correctness issue in the current surface behavior: a delegate can complete in the conversation/tool stream while the Workbench still shows the same child as `proceeding`. That means the conversation and Workbench are not converging on the same operation projection quickly enough.

## Design intent

Treat every delegate as a small semantic branch/workstream with two identities:

- **Stable task ID:** immutable machine address, e.g. `delegate_2`.
- **Display branch label:** human semantic alias, e.g. `verify/tests`.

The stable task ID remains the canonical handle for `delegate_result`, `delegate_status`, cancellation, logs, and persistence. The branch label becomes the primary Workbench row label.

## Display model

Preferred Workbench row format:

```text
✓ verify/tests · done · result ready: /delegate result delegate_2
× verify/tests-timeout · failed · delegate_1
⇒ scout/workbench-state · proceeding · tool read
```

The header remains aggregate state:

```text
delegate running 0 · done 1 · failed 1 · pending results 1
```

The important rule: **Workbench is authoritative for operation state.** Conversation milestones may announce delegate completion, but they must not become the only place where completion is visible.

## Label scheme

Use branch-style labels:

```text
<role>/<target>
```

Examples:

- `verify/tests`
- `verify/rust-tests`
- `scout/workbench-state`
- `patch/delegate-labels`
- `review/projection-sync`

Role comes from worker profile or task intent:

- `verify`
- `scout`
- `patch`
- `review`
- `trace`
- `plan`

Target is generated from task text, scope paths, or explicit operator/agent label.

Collision rule:

```text
verify/tests
verify/tests-2
verify/tests-3
```

Do not mutate an existing delegate's label after creation except through an explicit rename/relabel operation.

## API shape

Extend delegate creation with an optional label field:

```json
{
  "task": "Run just test-rust and summarize failures",
  "worker_profile": "verify",
  "label": "verify/tests"
}
```

If `label` is omitted, generate one automatically.

Suggested internal fields:

```rust
pub struct DelegateTask {
    pub task_id: String,
    pub label: String,
    pub agent_name: Option<String>,
    pub task_description: String,
    // ...existing fields...
}
```

Projection fields should expose both identities:

```json
{
  "task_id": "delegate_2",
  "label": "verify/tests",
  "status": "completed",
  "status_label": "done",
  "result_ready": true
}
```

## Label generation heuristic

When no label is supplied:

1. Determine role:
   - use `worker_profile` when present;
   - otherwise infer from task verbs: run/check/test → `verify`, inspect/find/read → `scout`, edit/fix/implement → `patch`, assess/review → `review`.
2. Determine target:
   - prefer named scope path stem if specific, e.g. `workbench`, `delegate`, `tests`;
   - otherwise extract key nouns from first task sentence;
   - normalize to lowercase kebab case;
   - cap to 2–3 words.
3. Compose `<role>/<target>`.
4. Deduplicate within current `DelegateResultStore` by appending `-2`, `-3`, etc.

## Workbench convergence requirement

Completion must update the shared delegate result store and refresh the operation projection used by Workbench before or at the same time as a conversation milestone is emitted.

Required invariant:

> If a delegate completion milestone is visible, then the next Workbench render must show the same delegate as terminal (`done`, `failed`, `cancelled`, or `timed_out`), never `proceeding`.

Likely implementation targets:

- `core/crates/omegon/src/features/delegate.rs`
  - store semantic labels;
  - generate default labels;
  - ensure `update_task_status()` is the only completion path;
  - ensure progress snapshots carry terminal state.
- `core/crates/omegon/src/surfaces/operations.rs`
  - keep `task_id` and `label` distinct in `OperationChildRow`.
- `core/crates/omegon/src/tui/workbench.rs`
  - render display label as primary row text and task ID only in result hints/detail.
- app/dashboard event handling
  - refresh `dashboard.delegate` from `DelegateResultStore::progress_snapshot()` after completion notifications.

## Tests

Minimum test coverage:

1. Generated labels:
   - verify task over Rust tests becomes `verify/tests` or `verify/rust-tests`.
   - duplicate labels receive numeric suffixes.
   - explicit labels are preserved.
2. Projection identity:
   - `OperationChildRow.id == delegate_N`.
   - `OperationChildRow.label == semantic branch label`.
3. Workbench rendering:
   - completed delegate row renders `done`, not `proceeding`.
   - result-ready hint includes the stable task ID.
4. State convergence:
   - after delegate completion event, Workbench aggregate counts match result store counts.
   - conversation completion milestone and Workbench projection agree on terminal status.
5. Result viewed:
   - fetching `delegate_result` clears pending-result hint but keeps row terminal.

## Non-goals

- Do not remove `delegate_N` IDs.
- Do not make creative random callsigns primary labels.
- Do not require operators to manually label every delegate.
- Do not let conversation tool segments substitute for Workbench operation state.

## Decision

Adopt semantic branch labels for delegates while preserving sequential task IDs as stable handles. The Workbench should render branch labels first and task IDs only where an exact command/address is needed. Operation state must converge through a shared projection so completed delegates cannot remain displayed as `proceeding` after completion is known.

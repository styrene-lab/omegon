---
title: Subagent Operation Reporting Workstream
status: implementing
tags: [subagents, delegate, cleave, tui, workbench, operations]
---

# Subagent Operation Reporting Workstream

## Goal

Stop leaking internal decomposition events into user-facing subagent UI. Delegate and cleave should report through an explicit operation projection with operation provenance, shared child-row semantics, and separated transcript/Workbench/detail responsibilities.

Primary bug to eliminate:

```text
delegate -> Cleave: 1 children dispatched
```

That is a projection/provenance bug, not a glyph bug.

## Branch

```text
feature/operation-reporting-projection
```

Main must remain clean while this workstream evolves.

## Source design

See [[subagent-cleave-delegate-operations]] for the parent/child operation-reporting skeleton and reality check.

## Constraints

- Do not fix this only in `tui/mod.rs` string rendering.
- Preserve cleave wording for actual cleave operations.
- Delegate-originated operation reports must not render as cleave.
- Workbench remains the live state surface; transcript records sparse milestones.
- Shared projection types should be renderer-neutral and usable by future CLI/ACP/Web surfaces.
- First slice should adapt existing state; do not start by changing child process JSONL/IPC protocol.

## Implementation Phases

### Phase 0 — Baseline evidence and failing tests

- [x] Add/locate a TUI regression proving delegate-originated child dispatch does not render `Cleave: 1 children dispatched`.
- [x] Add/locate a cleave regression proving actual cleave dispatch still renders cleave wording.
- [x] Add/locate Workbench test proving failed delegate rows expose failure reason or details affordance.

Acceptance:

```text
delegate transcript/workbench has no misleading cleave wording
actual cleave transcript keeps cleave wording
failed delegate row carries actionable failure state
```

### Phase 1 — Renderer-neutral projection skeleton

Add a module such as:

```text
core/crates/omegon/src/surfaces/operations.rs
```

Initial DTOs:

- `OperationKind`
- `OperationStatus`
- `ChildStatus`
- `OperationFailureKind`
- `OperationFailure`
- `ChildOperationRow`
- `OperationWorkbenchProjection`

Adapters:

- [x] `DelegateProgress -> OperationWorkbenchProjection`
- [x] cleave progress/state -> `OperationWorkbenchProjection` where current data permits

Acceptance:

```text
unit tests prove delegate and cleave map to the same child-row shape
```

### Phase 2 — Workbench consumes operation projections

- [x] Route delegate Workbench panel through operation projection rows.
- [x] Route cleave Workbench panel through operation projection rows where practical.
- [x] Preserve operation-specific headers while sharing row rendering.
- [x] Include failed delegate reason/detail affordance if available.

Acceptance:

```text
Workbench delegate and cleave rows share status/glyph/detail semantics
failed delegate rows are actionable without transcript archaeology
```

### Phase 3 — Transcript lifecycle provenance

- [x] Stop delegate-originated `DecompositionStarted` from rendering as `Cleave`.
- [x] Decide tactical path:
  - add operation provenance to event variants, or
  - introduce delegate-specific operation events, or
  - suppress decomposition transcript rows while Workbench owns delegate live state.
- [x] Audit ACP/MQTT/Web event consumers that currently map `DecompositionStarted` as cleave.

Acceptance:

```text
delegate operation emits/renders delegate milestone or no transcript spam
actual cleave operation emits/renders cleave milestone
```

### Phase 4 — Cancellation/failure parity follow-up

- [x] Add `DelegateTaskStatus::Cancelled { reason }` so cancellation is represented as terminal non-failure state rather than failure.
- [x] Define delegate cancellation control action via `delegate_cancel`.
- [x] Map delegate and cleave failures to shared operator-facing failure taxonomy.

Acceptance:

```text
operator can understand and act on running/failed/cancelled child operations consistently
```

## First narrow patch target

Initial implementation pivoted to event provenance first because the compiler-confirmed root seam was `AgentEvent::DecompositionStarted` lacking operation identity while delegate and cleave both emitted it. Projection DTOs remain the next slice after the provenance-bearing event contract is committed.

Completed in the first slice:

- runtime decomposition events now require `OperationRef`;
- delegate emits `OperationKind::Delegate`;
- cleave emits `OperationKind::Cleave`;
- TUI renders delegate starts as delegate, not cleave;
- WebSocket, IPC, and MQTT projections carry operation provenance;
- focused tests cover delegate, cleave, WebSocket, IPC, and MQTT provenance.

Completed follow-up slices:

- added renderer-neutral operation projection DTOs in `core/crates/omegon/src/surfaces/operations.rs`;
- mapped `DelegateProgress` and `CleaveProgress` into `OperationWorkbenchProjection`;
- routed delegate and cleave Workbench panels through the shared operation projection renderer;
- canonicalized child status labels so renderers do not leak engine-specific status strings by default;
- surfaced delegate failure summaries through shared operation child rows;
- added first-class delegate cancelled status that projects as terminal non-failure operation state.

Completed additional slice:

- routed `delegate_status` structured output through `OperationWorkbenchProjection::from_delegate`;
- routed `/cleave status` display rows through `OperationWorkbenchProjection::from_cleave`;
- added stable string serializers for `OperationChildStatus` and `OperationFailureKind` so command/API surfaces do not leak Rust debug formatting;
- preserved legacy delegate/cleave status text while adding operation kind/id and projected failure payloads where structured details are available.

Next narrow patch target: extract a shared operation status serializer for command/API details so delegate and future cleave structured outputs do not hand-roll JSON shapes independently.

## Validation plan

Focused tests while iterating:

```bash
cargo test -p omegon operation -- --nocapture
cargo test -p omegon workbench -- --nocapture
cargo test -p omegon delegate -- --nocapture
cargo test -p omegon cleave -- --nocapture
```

Before merge:

```bash
cargo test -p omegon tui -- --nocapture
just test-dev-scripts
```

## Known risks

- `AgentEvent::DecompositionStarted` is consumed beyond TUI, including MQTT/IPC surfaces.
- Delegate currently emits decomposition events, so changing event semantics can break external dashboards if done abruptly.
- The first implementation should create adapters and tests before broad event changes.
- Failure reason may not currently be stored in `DelegateProgressChild`; if missing, capture it in the delegate state reducer before trying to render it.

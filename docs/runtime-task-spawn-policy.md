+++
id = "27e28628-688b-43df-88ad-a5b56a714f7c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Runtime task spawn policy

See also [[interactive-runtime-supervisor-implementation-plan]],
[[auspex-ipc-contract]], and [[error-recovery]].

## Scope

This document defines how Omegon should classify and launch background async
work in the Rust runtime.

The core problem is not "using `tokio::spawn` is bad". The actual failure mode
is **unclear detached-task semantics**:

- some tasks are operator-facing and must not fail silently
- some tasks are infrastructure and should log on failure
- some tasks are advisory/best-effort and should not escalate
- some detached parser tasks must preserve a stream contract even when they
  panic

This policy exists so those categories are explicit in code instead of living in
call-site folklore.

---

## Decision summary

Omegon uses **policy-specific spawn helpers** instead of ad hoc detached tasks.

### Shared task policy module

Authoritative helper module:

- `core/crates/omegon/src/task_spawn.rs`

Provided helpers:

- `spawn_infra(...)`
- `spawn_best_effort(...)`
- `spawn_best_effort_result(...)`
- `spawn_operator_task(...)`
- `spawn_local_operator_task(...)`

### Provider stream policy

Detached provider SSE parser tasks use a dedicated provider-local wrapper in:

- `core/crates/omegon/src/providers.rs`

Authoritative helper:

- `spawn_provider_stream_task(...)`

This wrapper converts parser errors and panics into terminal `LlmEvent::Error`
messages so stream consumers do not hang silently when a detached parser dies.

---

## Policy classes

### 1. Operator-facing background tasks

Use when a detached task is part of operator-visible control flow and silent
failure would mislead the operator.

**Helper:** `spawn_operator_task(...)`

**Failure semantics:**

- normal error → warning log
- panic → error log + `AgentEvent::SystemNotification`

**Use for:**

- background login/auth workflows
- detached work launched from interactive slash/UI flow where the operator
  expects visible completion or failure

**Current example:**

- interactive auth login in `core/crates/omegon/src/main.rs`

### 2. Local operator-facing tasks

Use when the task has the same operator-facing semantics as above but must run
on the local task set because it is not `Send`.

**Helper:** `spawn_local_operator_task(...)`

**Failure semantics:**

- same as `spawn_operator_task(...)`
- runs via `tokio::task::spawn_local`

**Use for:**

- non-`Send` interactive runtime work
- local task set orchestration with visible operator consequences

### 3. Infrastructure tasks

Use for long-lived runtime machinery where failure should be logged clearly but
not escalated directly to the operator as a chat/system event.

**Helper:** `spawn_infra(...)`

**Failure semantics:**

- logs failure as infrastructure/runtime failure
- no direct `SystemNotification`

**Use for:**

- IPC server
- web server
- similar runtime services

**Current examples:**

- `core/crates/omegon/src/ipc/mod.rs`
- `core/crates/omegon/src/web/mod.rs`

### 4. Best-effort / advisory tasks

Use when the task is useful but non-critical and failure should not disrupt
foreground execution.

**Helpers:**

- `spawn_best_effort(...)`
- `spawn_best_effort_result(...)`

**Failure semantics:**

- completion/failure logged at debug level or handled as low-severity runtime
  noise
- no operator escalation

**Use for:**

- release/version checks
- telemetry/webhook hooks
- opportunistic cache refreshes
- maintenance/reindex checks
- simulated background completion helpers in non-critical paths

**Current examples:**

- `core/crates/omegon/src/features/version_check.rs`
- `core/crates/omegon/src/plugins/http_feature.rs`
- `core/crates/omegon/src/features/auth.rs`
- `core/crates/omegon/src/features/delegate.rs`
- `core/crates/omegon/src/tools/codebase_search.rs`
- background update check in `core/crates/omegon/src/update.rs`

### 5. Provider stream parser tasks

These are special.

They are detached continuations of a foreground stream request. They should not
emit operator notifications, but they **must** preserve the stream contract.

**Helper:** `spawn_provider_stream_task(...)`

**Failure semantics:**

- parser error → `LlmEvent::Error`
- parser panic → `LlmEvent::Error`
- warning log with provider context

**Why this is separate:**

A generic infra/best-effort wrapper is insufficient here. The receiver side is
waiting on stream-terminal semantics, so detached parser death must be surfaced
through the stream itself.

**Current examples:**

- Anthropic SSE parser
- OpenAI SSE parser
- OpenAI Codex SSE parser

---

## Production sites intentionally left as raw `tokio::spawn`

Not every remaining `tokio::spawn` is a bug.

These sites remain intentionally unchanged because their lifecycle semantics are
already explicit and owned by surrounding code.

### Paired lifecycle tasks

#### `core/crates/omegon/src/web/ws.rs`

- send task and recv task are created as a pair
- whichever completes first aborts the other
- lifecycle is explicit at the call site

This is not ambiguous detached background work; it is structured connection
ownership.

### Connection plumbing tasks

#### `core/crates/omegon/src/ipc/connection.rs`

- socket writer task and event push task are per-connection plumbing
- they terminate on disconnect/channel closure
- connection `run()` owns and tears them down

Again, this is structured connection machinery rather than orphaned work.

### Handle-owned tasks

#### `core/crates/omegon/src/cleave/orchestrator.rs`

- child monitor tasks are spawned and their `JoinHandle`s are retained
- orchestration awaits them and propagates failure structurally

This is not a silent detached-task hazard.

### Startup / host ownership in `main.rs`

Some remaining spawn sites in `main.rs` are top-level host/runtime tasks where:

- the caller owns the handle, or
- peers are explicitly aborted, or
- the task is part of startup/runtime composition rather than detached utility
  work

These should only be changed if their ownership model becomes unclear.

---

## Anti-patterns

### Bad: ad hoc detached task with unclear semantics

```rust
// ❌ unclear whether failure should log, notify, or be ignored
 tokio::spawn(async move {
     do_work().await;
 });
```

### Good: explicit best-effort task

```rust
crate::task_spawn::spawn_best_effort("version-check", async move {
    check_for_update().await;
});
```

### Good: explicit operator-facing task

```rust
crate::task_spawn::spawn_operator_task(
    "interactive-auth-login",
    events_tx.clone(),
    crate::task_spawn::OperatorTaskOptions {
        panic_notification_prefix:
            "⚠ Background login task crashed — authentication did not complete safely".into(),
    },
    async move {
        run_login_flow().await?;
        Ok(())
    },
);
```

### Good: detached parser preserving stream contract

```rust
spawn_provider_stream_task("openai", tx.clone(), async move {
    parse_openai_stream(response, telemetry, &tx).await
});
```

---

## Review checklist

When adding new background work, decide these questions first:

1. **Who owns the lifecycle?**
   - explicit join/abort owner
   - or detached background work?

2. **Who needs to know if it fails?**
   - operator
   - logs only
   - no one / best-effort
   - stream consumer

3. **What contract must be preserved?**
   - operator-visible completion/failure
   - runtime service availability
   - advisory side effect only
   - terminal stream event

4. **Is the task `Send`?**
   - if not, use local-task semantics

5. **Can failure hang a receiver?**
   - if yes, convert failure into a terminal event or result

---

## Tests

The policy layer is covered with focused tests in:

- `core/crates/omegon/src/task_spawn.rs`
- `core/crates/omegon/src/providers.rs`

Covered behaviors include:

- operator task panic → `SystemNotification`
- local operator task panic → `SystemNotification`
- provider stream task error → `LlmEvent::Error`
- provider stream task panic → `LlmEvent::Error`

---

## Current status

This policy is implemented and partially migrated.

Completed migrations include:

- operator-facing login hardening
- infrastructure server startup tasks
- advisory/background tasks (version check, plugin telemetry, auth refresh,
  delegate simulation, codebase search maintenance, update check)
- provider stream parser tasks

Future work should apply this policy to new runtime code by default rather than
backfilling ad hoc `tokio::spawn` usage later.

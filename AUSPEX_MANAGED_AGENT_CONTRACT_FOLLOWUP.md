# Auspex managed-agent supervisor contract follow-up

Commit `62016655` establishes the authenticated WebSocket/control-method wiring for:

- `delegate_dispatch`
- `delegate_get`
- `delegate_result`
- `delegate_cancel`

The transport and flat envelope direction are correct. Before Auspex integration, the worker response payloads need one contract-alignment pass.

## Required corrections

### 1. Project an explicit wire status DTO

Do not serialize `DelegateTaskStatus` directly in `DelegateResultStore::task_observation`.

Omegon currently emits externally tagged serde values resembling:

```json
"status": "Running"
```

or:

```json
{
  "status": {
    "Failed": {
      "error": "...",
      "kind": "ProviderStartup"
    }
  }
}
```

Auspex expects an internally tagged, snake-case wire contract:

```json
{ "status": { "kind": "running" } }
```

```json
{
  "status": {
    "kind": "failed",
    "failure_kind": "provider_startup",
    "safe_message": "safe operator-facing text"
  }
}
```

Required variants:

```text
running
completed { success: bool }
failed { failure_kind, safe_message }
cancelled { reason, termination_confirmed }
```

Define a supervisor wire DTO and explicitly convert from `DelegateTaskStatus`. Do not change the internal task-state enum merely to satisfy this protocol.

### 2. Keep observed result raw unless Omegon has structured evidence

`task_observation` currently emits `result: Option<String>`. Auspex's domain `ManagedRunResult` has structured fields (`summary`, `changed_files`, `validation`, `commits`, `artifacts`, `questions`), but Omegon cannot truthfully populate those from arbitrary delegate output.

Use this observation contract:

```json
"result": "raw bounded delegate result or null"
```

Auspex will normalize the fetched result into its domain model. Do not fabricate structured fields.

The Auspex observation DTO will be adjusted to accept `Option<String>`.

### 3. Explicitly map failure kinds

Wire names must be:

```text
missing_local_model
missing_credential
provider_startup
workspace_startup
unknown
```

Do not rely on the internal enum's default serde representation.

### 4. Make `delegate_get` a direct store-query operation

`tool_args()` currently maps `delegate_get` to `DELEGATE_STATUS`, but `main.rs` bypasses tool execution and queries `DelegateResultStore::task_observation` directly.

That works but leaves misleading routing metadata. Refactor the supervisor request parser so `delegate_get` is represented as a direct query, not a fake `delegate_status` tool invocation. A small enum is appropriate:

```rust
enum SupervisorOperation {
    Execute { tool: &'static str, args: Value },
    GetObservation { task_id: String },
}
```

### 5. Verify or populate effective policy on dispatch

Auspex's dispatch response accepts:

```text
worker_profile
max_turns
wall_timeout_seconds
idle_timeout_seconds
enabled_tools
model
thinking_level
```

Confirm the delegate tool result actually includes `details.effective_policy`. If it does not, populate it from the effective runtime/profile used for the child. Do not return requested values as though they were effective values unless they are what the runner actually applied.

`effective_policy: null` is protocol-valid temporarily, but a test should make that absence explicit.

### 6. Preserve cancellation semantics

Keep these distinct:

- `acknowledged`: the cancel request was accepted/recorded;
- `termination_confirmed`: live delegate state confirms execution is terminal.

Never infer termination confirmation solely from request acceptance.

## Required golden contract tests

Add tests that serialize the exact response JSON for:

1. running observation;
2. successful completion;
3. unsuccessful completion;
4. typed failure;
5. cancellation acknowledged but not confirmed;
6. cancellation confirmed;
7. dispatch accepted with effective policy (or explicitly null if not available yet);
8. result response;
9. unknown-task rejection;
10. unsupported-schema rejection.

The observation fixtures must use the flat envelope:

```json
{
  "type": "delegate_get_result",
  "schema_version": 1,
  "managed_run_id": "...",
  "worker_id": "...",
  "observation": { ... }
}
```

No method-specific fields may be nested under `body`.

## Cross-repository acceptance gate

After generating the golden JSON in Omegon, copy the fixtures into Auspex contract tests and deserialize them into Auspex's supervisor DTOs. The work is complete only when:

- Omegon serialization tests pass;
- Auspex deserialization tests pass against the exact Omegon-generated fixture JSON;
- `cargo check --message-format=short` passes in both repositories;
- focused supervisor tests pass in both repositories;
- `git diff --check` passes.

## Relevant files

Omegon:

- `core/crates/omegon/src/managed_agent_supervisor.rs`
- `core/crates/omegon/src/features/delegate.rs`
- `core/crates/omegon/src/main.rs`
- `core/crates/omegon/src/web/ws.rs`

Auspex contract reference:

- `../auspex/auspex-core/src/managed_agent_supervisor.rs`
- `../auspex/auspex-core/src/managed_agents.rs`

Do not add A2A behavior. This remains a supervisor-to-worker execution contract.

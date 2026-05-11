+++
id = "b2a6a4a9-7a2d-4a82-9c49-0f27ef6dd089"
kind = "document"
title = "n8n Sentry submission — external workflow task ingress"
status = "planned"
tags = ["sentry", "n8n", "automation", "acp", "mcp", "a2a"]
aliases = ["n8n-sentry-submission", "sentry-submit"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
last_updated = "2026-05-11"
parent = "sentry-push-tasking"
open_questions = ["Should submitted one-shot tasks survive process restart, or remain ephemeral run-history entries only?", "Should the first n8n integration be recipes over HTTP Request nodes or a packaged n8n community node?", "Should streamed progress use SSE in Sentry v1, or stay poll-only until ACP event reuse is extracted?"]
+++

# n8n Sentry Submission

## Goal

Make Omegon easy to use as an n8n automation worker by adding a first-class task submission endpoint to Sentry. n8n should be able to submit a task payload, receive a task id immediately, poll or wait for completion, and correlate the result with the originating workflow execution.

This is the feature gap between the current predefined Sentry task model and a graph-workflow runtime like n8n or Flynt.

## Current Surfaces

### Sentry

Sentry already provides:

- `GET /api/healthz`
- `GET /api/readyz`
- `GET /api/sentry/tasks`
- `GET /api/sentry/tasks/{id}`
- `POST /api/sentry/tasks/{id}/run`
- `POST /api/sentry/trigger/{name}`

Those are enough for n8n to fire known tasks, but not enough for n8n to submit arbitrary one-shot work.

The current executor takes `TriggerEvent` values and routes them into `spawn_task_execution`, which fetches the `TaskSpec` from a `TaskBoard`. That works for `.omegon/tasks/`, `sentry.toml`, and Flynt boards, but it forces all work to be known before submission.

### ACP

ACP is Omegon's strongest reusable protocol surface today:

- session lifecycle and configuration
- prompt submission
- cancellation
- streamed model/tool/plan updates
- host capability delegation
- client-provided MCP server forwarding
- WebSocket transport with token auth
- ext_method RPC for settings and management surfaces

Sentry should not expose ACP as its n8n-facing API in v1. n8n wants a stable automation API, not an editor session protocol. However, the ACP worker has useful implementation patterns that should be extracted or mirrored:

- request/response channels around prompt execution
- cancellation token propagation
- event mapping from `AgentEvent` into transport-neutral progress updates
- MCP server conversion and dynamic tool registration
- redaction before external emission

### MCP

Omegon is currently a strong MCP client, not an MCP server:

- project and extension MCP servers are connected at setup
- stdio, Streamable HTTP, OCI, Docker MCP gateway, and Styrene mesh-oriented config are represented in `McpServerConfig`
- ACP clients can forward MCP servers into the session, and the ACP worker registers their tools dynamically

For n8n, MCP should remain a tool-ingress mechanism for Omegon, not the first external control plane. A future n8n AI workflow could connect to an Omegon MCP server if Omegon later exposes agent actions as tools, but that would be a second surface after HTTP task submission.

### A2A

A2A remains deferred. It is appropriate for agent-to-agent federation with discovery, task lifecycle, and streaming over standard HTTP semantics. It is heavier than needed for n8n v1 because n8n can already call HTTP APIs directly and does not require agent discovery to execute a workflow node.

The Sentry submit contract should be compatible with a future A2A adapter:

- submitted work has a stable task id
- progress/result state is externally queryable
- correlation ids are preserved
- auth and capability metadata are explicit
- result summaries and session ids are structured

## Proposed API

### Submit Task

```http
POST /api/sentry/submit
Content-Type: application/json
```

```json
{
  "prompt": "Review the failed deployment and propose a rollback plan.",
  "model": "auto",
  "max_turns": 20,
  "timeout_secs": 600,
  "token_budget": 500000,
  "cwd": "/workspace/app",
  "correlation_id": "n8n:execution:12345",
  "source": "n8n",
  "metadata": {
    "workflow_id": "wf_abc",
    "node_id": "omegon_submit"
  }
}
```

Response:

```json
{
  "accepted": true,
  "task_id": "submitted-7f4a6d2e",
  "run_id": "submitted-7f4a6d2e-001",
  "status": "queued",
  "correlation_id": "n8n:execution:12345",
  "status_url": "/api/sentry/submissions/submitted-7f4a6d2e"
}
```

### Get Submitted Task

```http
GET /api/sentry/submissions/{task_id}
```

Response:

```json
{
  "task_id": "submitted-7f4a6d2e",
  "correlation_id": "n8n:execution:12345",
  "source": "n8n",
  "status": "completed",
  "exit_code": 0,
  "summary": "Deployment failed after the database migration. Roll back service image only; do not roll back schema.",
  "tokens_used": 38142,
  "duration_secs": 94,
  "session_id": "sentry-20260511T183015",
  "started_at": "2026-05-11T18:30:15Z",
  "finished_at": "2026-05-11T18:31:49Z",
  "error": null,
  "metadata": {
    "workflow_id": "wf_abc",
    "node_id": "omegon_submit"
  }
}
```

### Optional Wait Endpoint

```http
GET /api/sentry/submissions/{task_id}/wait?timeout_secs=55
```

This is convenient for n8n's request/response style nodes. It should long-poll up to the requested timeout and return the same shape as `GET /api/sentry/submissions/{task_id}`.

Streaming can be added later as:

```http
GET /api/sentry/submissions/{task_id}/events
Accept: text/event-stream
```

## Data Model

Add an explicit submitted-task record instead of overloading `task_runs` alone:

```sql
CREATE TABLE IF NOT EXISTS submitted_tasks (
  task_id        TEXT PRIMARY KEY,
  run_id         TEXT NOT NULL,
  source         TEXT NOT NULL,
  correlation_id TEXT,
  status         TEXT NOT NULL DEFAULT 'queued',
  spec_json      TEXT NOT NULL,
  metadata_json  TEXT,
  created_at     TEXT NOT NULL,
  started_at     TEXT,
  finished_at    TEXT
);
CREATE INDEX IF NOT EXISTS idx_submitted_tasks_correlation ON submitted_tasks(correlation_id);
```

`task_runs` remains the run history table and should keep storing execution output. `submitted_tasks` provides discovery for ephemeral work that does not exist on a `TaskBoard`, and preserves metadata needed by n8n or Auspex.

## Executor Contract

Add a `TriggerEvent` variant:

```rust
TaskSubmitted {
    task_id: String,
    run_id: String,
    spec: sentry::types::TaskSpec,
    source: String,
    correlation_id: Option<String>,
    metadata: serde_json::Value,
}
```

The executor path should bypass `TaskBoard` claim/release:

1. Route `TaskSubmitted` through the same in-flight and semaphore controls.
2. Resolve `model = "auto"` through the existing Sentry routing logic.
3. Execute with the same `run_agent_task` path as board-backed tasks.
4. Record start/complete/failure in `StateDb`.
5. Update `submitted_tasks.status`.
6. Preserve `correlation_id` and metadata for API responses.

No board lifecycle hooks should run for submitted tasks unless the submission explicitly references `design_node_id` or `openspec_change`.

## Auth And Exposure

The endpoint should inherit the existing localhost-first Sentry posture:

- bind to `127.0.0.1` by default
- keep health/readiness unauthenticated
- require a control token for mutation endpoints before any non-local binding is allowed
- include the token in startup JSON the same way ACP WebSocket server emits its tokenized URL

Remote use through n8n should prefer one of:

- n8n running on the same host/container network
- reverse proxy with TLS and auth
- future Auspex/Aether routing

Do not expose unauthenticated Sentry submit over `0.0.0.0`.

## n8n Integration Shape

### Immediate Recipes

Use n8n's built-in HTTP Request node:

1. Submit task with `POST /api/sentry/submit`.
2. Store `task_id` and `correlation_id`.
3. Poll `GET /api/sentry/submissions/{task_id}` until terminal.
4. Route on `exit_code` or `status`.

This lets teams adopt Omegon in n8n before maintaining a custom node package.

### Packaged Node

Create `n8n-nodes-omegon` after the HTTP contract is stable.

Operations:

- Submit Task
- Submit And Wait
- Run Known Task
- Fire Trigger
- Get Task
- List Tasks
- Health Check

Credentials:

- Base URL
- Control token

Node output should always include:

- `task_id`
- `run_id`
- `correlation_id`
- `status`
- `exit_code`
- `summary`
- `tokens_used`
- `duration_secs`
- `session_id`

## ACP Reuse Plan

Do not make Sentry submit speak ACP. Instead, extract transport-neutral pieces from ACP and Sentry over time:

1. Introduce a shared `AgentTaskRunner` helper that owns:
   - shared settings initialization
   - `AgentSetup`
   - bridge resolution/shutdown
   - cancellation and timeout
   - token accounting
   - session save
   - `TaskResult` construction

2. Map `AgentEvent` into a generic `TaskProgressEvent` enum:
   - `started`
   - `text_delta`
   - `thinking_delta`
   - `tool_start`
   - `tool_output`
   - `tool_end`
   - `plan_update`
   - `completed`
   - `failed`

3. Let ACP continue translating `TaskProgressEvent` into ACP notifications.

4. Let Sentry translate the same event stream into state-db updates, optional SSE, and future MQTT/Aether result messages.

This keeps ACP as the rich editor/session protocol while making its strongest internal machinery useful to n8n, Flynt, Auspex, and future A2A adapters.

## Implementation Phases

### Phase 1 — HTTP Submit And Poll

- Add request/response structs in `sentry::routes`.
- Add `POST /api/sentry/submit`.
- Add `GET /api/sentry/submissions/{task_id}`.
- Add `TriggerEvent::TaskSubmitted`.
- Add submitted-task persistence to `StateDb`.
- Add executor branch for submitted tasks.
- Tests:
  - route serialization
  - submitted task state transitions
  - queued response preserves correlation id
  - submitted task bypasses `TaskBoard`

### Phase 2 — Auth Hardening

- Add Sentry control token generation.
- Include tokenized mutation URLs in startup JSON.
- Require token for submit, run, and trigger endpoints.
- Keep health/readiness open.
- Tests:
  - missing token rejected
  - bad token rejected
  - valid token accepted

### Phase 3 — Wait Endpoint And n8n Recipe

- Add long-poll wait endpoint.
- Document n8n HTTP Request node workflow.
- Add examples for submit, wait, and failure routing.

### Phase 4 — Progress Events

- Extract a shared progress event mapper from ACP/Sentry.
- Add optional SSE endpoint for submitted tasks.
- Keep polling as the compatibility baseline.

### Phase 5 — Packaged n8n Node

- Implement `n8n-nodes-omegon`.
- Support credentials, operation selector, and typed outputs.
- Keep the node thin; business logic stays in Sentry HTTP API.

## Decisions

### Decision: HTTP submit is the first-class n8n target, not MCP or A2A

**Status:** decided

**Rationale:** n8n is already excellent at calling HTTP APIs. A direct Sentry API has lower adoption cost, is easier to debug, and maps cleanly onto workflow execution ids. MCP and A2A remain useful, but they solve adjacent problems.

### Decision: Submitted tasks are one-shot by default

**Status:** decided

**Rationale:** n8n owns the workflow graph and can resubmit work. Sentry should not silently mutate `.omegon/tasks/` or `sentry.toml` for one-off automation events. Persisting run metadata is enough for audit and polling.

### Decision: ACP internals should be reused below the transport layer

**Status:** decided

**Rationale:** ACP is a session/editor protocol, but its worker and event-mapping patterns are mature. Extracting common runner/progress pieces gives Sentry and future A2A adapters the same behavior without coupling n8n to ACP wire semantics.

### Decision: Omegon should stay an MCP client for this feature

**Status:** decided

**Rationale:** Omegon's MCP client surface is already robust and valuable inside submitted tasks. Exposing Omegon itself as an MCP server can be revisited after Sentry has a stable task submission API.

### Decision: A2A remains a future adapter, not the core implementation

**Status:** decided

**Rationale:** The submit/result/correlation contract can back an A2A server later. Implementing A2A first would add discovery, JSON-RPC, and streaming semantics before the basic workflow automation primitive is proven.

## Acceptance Criteria

- n8n can submit arbitrary one-shot work with one HTTP request.
- Sentry returns a task id without waiting for model completion.
- n8n can poll or wait for terminal status.
- Result payload includes `summary`, `exit_code`, token usage, duration, session id, and correlation id.
- Submitted tasks share Sentry's concurrency limit and cancellation path.
- Board-backed tasks continue to behave unchanged.
- Sentry docs clearly distinguish predefined tasks from submitted one-shot tasks.

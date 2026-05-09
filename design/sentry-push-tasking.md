# Sentry Push-Based Tasking — Inbound Work via HTTP and Aether

## Problem

Sentry is pull-only. It polls a local TaskBoard for work. But the deployment model requires omegon instances to **receive** tasks from sources they have no local knowledge of:

- An Auspex operator dispatching work to a fleet of sentry pods
- A Flynt board on a developer's laptop pushing tasks to a remote omegon in k8s
- A CI pipeline triggering post-deploy verification on a sentry instance
- Another omegon instance (cleave parent) delegating subtasks to a remote sentry
- A monitoring system routing alerts to the nearest available agent

The current webhook trigger (`POST /api/sentry/trigger/{name}`) only fires pre-defined local tasks by name. It can't accept a task the instance has never seen.

## Context: Auspex and Managed Mode

Auspex is the fleet supervisor for Styrene. It maintains:
- An **instance registry** — knows every running omegon pod (`~/.config/auspex/instance-registry.json`)
- An **embedded MQTT broker** (Aether) — the event fabric for fleet-wide pub/sub
- **Three interaction modes** (documented in `auspex/docs/auspex-multi-agent-runtime.md`):
  1. **Operator direct** — human talks to dispatcher (implemented)
  2. **Managed** — local agent directs remote agents (**not yet implemented**)
  3. **Autonomous** — sentry/detached-service runs independently (partially implemented — local pull only)

Push-based tasking implements **managed mode**. An omegon instance (or Auspex itself) publishes a task to a remote sentry's MQTT topic. The sentry receives it, executes it, and publishes the result back.

## Architecture

### Three Inbound Paths, One Event Channel

```
                    ┌──────────────────┐
                    │  Local TaskBoard │  (pull — existing)
                    │  .omegon/tasks/  │
                    │  sentry.toml     │
                    └────────┬─────────┘
                             │ TriggerEvent::Scheduled
                             │
┌─────────────────┐          ▼
│  HTTP API       │──► TriggerEvent::TaskSubmitted ──►┌─────────────┐
│  POST /submit   │                                    │  Sentry     │
└─────────────────┘                                    │  Executor   │
                                                       │  (spawned)  │
┌─────────────────┐          ▲                         └─────────────┘
│  Aether MQTT    │──► TriggerEvent::TaskSubmitted ──►
│  subscribe()    │
└─────────────────┘
```

All three paths produce `TriggerEvent` variants that enter the same executor. The executor doesn't know or care where the task came from.

### New TriggerEvent Variant

```rust
pub enum TriggerEvent {
    Scheduled(TriggerConfig),           // existing — cron/interval/preset
    FileChanged { ... },                // existing — notify watcher
    GitChanged { ... },                 // existing — git ref diff
    Webhook { name, payload },          // existing — named trigger fire
    ForceRun { task_id },               // existing — run a local task by ID
    TaskSubmitted {                     // NEW — full task from external source
        spec: TaskSpec,
        source: String,                 // "http", "mqtt:topic", "auspex:{instance}"
        correlation_id: Option<String>, // for result routing
    },
}
```

### HTTP Push Endpoint

```
POST /api/sentry/submit
Content-Type: application/json

{
  "prompt": "Review all open PRs and leave comments",
  "model": "anthropic:claude-sonnet-4-6",
  "max_turns": 20,
  "timeout_secs": 300,
  "token_budget": 500000,
  "design_node_id": "auth-rewrite-2026",
  "correlation_id": "ci-run-4567"
}

→ 202 Accepted
{
  "task_id": "submitted-a1b2c3d4",
  "status": "queued"
}
```

The submitted task is ephemeral — it doesn't persist to the task tree or sentry.toml. It's a one-shot execution. Results are available via `GET /api/sentry/tasks/{task_id}` (run history in SQLite) and optionally published back to Aether.

### MQTT Push Path

**Subscribe topic:** `styrene/{operator_id}/omegon/{instance_id}/tasks/submit`

**Message format:** JSON-serialized `TaskSpec` with MQTT 5.0 user properties for routing metadata:
- `correlation_id` — for result routing
- `reply_topic` — where to publish the result (e.g., `styrene/{op}/auspex/{auspex_id}/tasks/result`)
- `source_service` — who submitted (e.g., "auspex", "omegon", "flynt")
- `source_instance` — submitter's instance ID

**On receive:** Deserialize to `TaskSpec`, wrap in `TriggerEvent::TaskSubmitted`, send to executor.

**On completion:** Publish `TaskResult` to `reply_topic` (if provided) or to `styrene/{op}/omegon/{instance_id}/tasks/result`.

### Result Publishing

When a submitted task completes, the result should be available via:
1. **HTTP** — `GET /api/sentry/tasks/{task_id}` (already works via run history)
2. **MQTT** — publish to reply topic or default result topic

Result message:
```json
{
  "task_id": "submitted-a1b2c3d4",
  "correlation_id": "ci-run-4567",
  "exit_code": 0,
  "summary": "Reviewed 3 PRs, left 7 comments",
  "tokens_used": 45000,
  "duration_secs": 120,
  "session_id": "sentry-20260509T143022"
}
```

## Executor Changes

The executor's `handle_trigger_event` gains a new arm:

```rust
TriggerEvent::TaskSubmitted { spec, source, correlation_id } => {
    let task_id = format!("submitted-{}", uuid_v4());
    tracing::info!(
        task = %task_id,
        source = %source,
        correlation = ?correlation_id,
        "external task submitted"
    );
    // No board claim needed — submitted tasks bypass the board entirely
    spawn_submitted_task(spec, task_id, source, correlation_id, ...);
}
```

Submitted tasks skip the board entirely — no claim/release cycle, no local task file. They're recorded in `state_db` as runs and available via the API.

## Auspex Integration

### Discovery

When `omegon sentry` starts with MQTT enabled, it:
1. Connects to the Aether broker (same as existing `mqtt_bridge.rs`)
2. Subscribes to `styrene/{op}/omegon/{instance_id}/tasks/submit`
3. Publishes a registration message to `styrene/{op}/omegon/{instance_id}/status` with capabilities (`{ "sentry": true, "accepts_tasks": true, ... }`)

Auspex's instance registry picks up the status publication and marks the instance as task-accepting.

### Fleet Dispatch

An Auspex operator (or automation) can then:
1. Query the instance registry for sentry-capable instances
2. Select one based on load, location, model availability
3. Publish a `TaskSpec` to the selected instance's submit topic
4. Await the result on the reply topic

This is the managed mode contract. Auspex is the router, omegon sentry instances are the workers.

### Topology

```
┌─────────────────────────────────┐
│        Auspex Operator          │
│  ┌──────────────────────────┐   │
│  │  embedded MQTT broker    │   │
│  │  (rumqttd / Aether)      │   │
│  └──────┬──────────┬────────┘   │
│         │          │            │
│         │ publish  │ subscribe  │
│         │          │            │
│  ┌──────▼──┐  ┌────▼────────┐  │
│  │ submit  │  │ result      │  │
│  │ topic   │  │ topic       │  │
│  └─────────┘  └─────────────┘  │
└─────────────────────────────────┘
        │               ▲
        │ MQTT          │ MQTT
        ▼               │
┌───────────────────────┤
│  omegon sentry pod    │
│  ├── subscribe(submit)│
│  ├── executor(spawn)  │
│  └── publish(result) ─┘
└───────────────────────┘
```

## Implementation Phases

### Phase I: HTTP Submit Endpoint

Add `POST /api/sentry/submit` to the control plane. Accepts `TaskSpec` JSON, wraps in `TriggerEvent::TaskSubmitted`, sends to event channel. Returns task ID for polling.

~50 lines in routes.rs + new TriggerEvent variant.

### Phase II: MQTT Subscription

Extend `mqtt_bridge.rs` (or a new `mqtt_inbound.rs`) to subscribe to the task submit topic. On message receive, deserialize `TaskSpec`, send as `TriggerEvent::TaskSubmitted`.

Requires: `styrene-mqtt` `subscribe()` capability (already available).

~80 lines. Depends on Aether broker being reachable (graceful fallback if not).

### Phase III: Result Publishing

On task completion, publish `TaskResult` to the MQTT result topic. The `correlation_id` from the submission flows through to the result for request/response correlation.

~40 lines in executor.rs.

### Phase IV: Auspex Fleet Routing

Auspex gains a "dispatch task" UI/API that:
1. Enumerates sentry-capable instances from the registry
2. Lets the operator (or an automation) select a target
3. Publishes the task to the selected instance's submit topic
4. Shows the result when it arrives

This is Auspex-side work, not omegon. The contract is defined by the MQTT topic hierarchy and message format above.

## Topic Hierarchy Extension

Current (events only, omegon → Aether):
```
styrene/{op}/omegon/{instance}/events/{event_type}
```

New (bidirectional):
```
styrene/{op}/omegon/{instance}/events/{event_type}   # existing — outbound events
styrene/{op}/omegon/{instance}/tasks/submit           # NEW — inbound task specs
styrene/{op}/omegon/{instance}/tasks/result            # NEW — outbound task results
styrene/{op}/omegon/{instance}/status                  # NEW — instance capabilities
```

## Security Considerations

- HTTP submit endpoint inherits sentry's existing auth (query token, same as `/api/sentry/trigger`)
- MQTT submit topic should require authenticated publisher (Aether broker enforces identity via `ServiceIdentity`)
- Submitted tasks run with the sentry instance's local permissions — a remote submitter can't grant capabilities the instance doesn't have
- Rate limiting: the bounded event channel (256) provides backpressure; aggressive submitters get 429 from HTTP or message drops from MQTT

## Open Questions

1. **Should submitted tasks persist to the task tree?** Currently they're ephemeral (run history only). If a submitted task should survive sentry restart, it needs to be written to `.omegon/tasks/` before execution.

2. **Should the result include the full conversation?** The summary is useful but the full session may be needed for auditing. Could store session_id and let the consumer fetch via the session API.

3. **Should Auspex be required for MQTT?** Currently the MQTT bridge is optional (graceful fallback). Push tasking via MQTT should follow the same pattern — works if Aether is available, HTTP-only if not.

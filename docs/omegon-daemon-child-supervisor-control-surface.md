+++
id = "99993dcb-535b-4717-a6e2-f379d5c89ee1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon daemon child supervisor control surface

Related:
- [[omega-daemon-child-supervisor]]
- [[omega-daemon-runtime]]
- [[auspex-omegon-launch-contract]]

## Purpose

Define the **local-first control surface schema** for Omegon-managed cleave child supervision.

This document is not a future identity design. It defines what must be true **now** for:
- local daemon runtime correctness
- restart-path child recovery
- child-scoped cancel/reap control
- consistent semantics across TUI, daemon, VM, and container deployments

The design is explicitly:
- **local-only**
- **same-host trusted**
- **bootstrap/degraded**, not final distributed security

Styrene Identity is a future replacement for the authority layer once supervision crosses process/host trust boundaries.

---

## Layering

### Outer supervisor

**Auspex** or another host/runtime supervisor owns:
- Omegon process lifetime
- restart policy
- placement and deployment environment
- external orchestration

### Inner runtime supervisor

**Omegon** owns:
- cleave child launch semantics
- child registry persistence
- restart-path child adoption rules
- progress reconstruction
- child-scoped cancel/reap semantics
- runtime-facing control surfaces

### Worker layer

**Cleave child processes** are worker subprocesses launched by Omegon.

---

## Control-surface goals

The control surface must let Omegon answer, for every child:

1. **Who owns it?**
2. **Is it still running?**
3. **Was it safely adopted after restart, or only recovered in degraded mode?**
4. **Can it still be cancelled? If so, by what mechanism?**
5. **What is the last known activity?**

No surface should imply stronger continuity than the runtime actually has.

---

## Source of truth

### Durable child registry

The durable registry currently lives in:
- `.omegon/cleave-workspace/state.json`

This file is the restart-path source of truth for:
- child identity
- child ownership lineage
- child execution fingerprint
- child runtime liveness metadata

### Durable child activity replay

Per-child activity replay currently lives in:
- `.omegon/cleave-workspace/child-<label>.activity.log`

These logs are the restart-path source of truth for degraded continuity of:
- last tool
- last turn
- child token usage observations
- last activity time

---

## Durable registry schema

### Run-level fields (`CleaveState`)

| Field | Type | Meaning |
|---|---|---|
| `run_id` | string | Stable identifier for one cleave orchestration run |
| `directive` | string | Original parent directive |
| `repo_path` | string | Repo root path |
| `workspace_path` | string | Cleave workspace path |
| `supervisor_token` | string | Local bootstrap lease token identifying the current supervisor lineage |
| `children` | array | Per-child durable records |
| `plan` | json | Serialized plan payload |

### Child-level fields (`ChildState`)

| Field | Type | Meaning |
|---|---|---|
| `child_id` | integer | Stable child ordinal within the run |
| `label` | string | Human-facing child identifier |
| `description` | string | Child mission description |
| `scope` | string[] | Child file scope |
| `depends_on` | string[] | Child dependency labels |
| `status` | enum | `pending \| running \| completed \| failed \| upstream_exhausted` |
| `error` | string? | Failure detail |
| `branch` | string? | Child branch name |
| `worktree_path` | string? | Child worktree path |
| `backend` | string | Execution backend label |
| `execute_model` | string? | Requested/effective model |
| `provider_id` | string? | Provider if known |
| `duration_secs` | number? | Terminal duration |
| `stdout` | string? | Captured final stdout |
| `runtime` | object? | Child runtime profile |
| `pid` | integer? | Current child PID while the process is believed alive |
| `started_at_unix_ms` | integer? | Spawn timestamp |
| `last_activity_unix_ms` | integer? | Last observed progress timestamp |
| `adoption_worktree_path` | string? | Canonical worktree fingerprint persisted at spawn |
| `adoption_model` | string? | Execute-model fingerprint persisted at spawn |
| `supervisor_token` | string? | Supervisor lineage token persisted at spawn |

---

## Supervision mode schema

### Runtime-only child supervision mode (`ChildProgress.supervision_mode`)

| Value | Meaning |
|---|---|
| `attached` | The current Omegon process spawned/owns the live child monitor and has direct in-process control |
| `recovered_degraded` | The current Omegon process reconstructed child state from durable registry + activity logs after restart; continuity is degraded, not fully attached |

### Semantics

#### `attached`

Guarantees:
- live in-process cancel handle exists
- direct child monitor task exists
- progress is updated from the live subprocess stream
- child PID is current

#### `recovered_degraded`

Guarantees:
- child survived restart and passed local adoption checks
- state is reconstructed from durable registry
- activity context is reconstructed from durable logs
- PID-based cancel fallback is available

Non-guarantees:
- no original pipe/file-descriptor continuity
- no true live stream reattachment to the prior monitor
- no claim that the current daemon owns the same monitor channel as before restart

---

## Adoption contract

A persisted `running` child may remain `running` after restart only if **all** of these checks pass:

1. **PID is alive**
2. **Canonical worktree path matches** persisted `adoption_worktree_path`
3. **Execute model matches** persisted `adoption_model`
4. **Supervisor token matches** run-level `supervisor_token`

If any check fails:
- the child must be demoted to `pending`
- `pid` must be cleared
- adoption metadata must be cleared
- the child must not be treated as adopted/running

This is the first-pass local safe-adoption rule.

---

## Cancel control surface

### In-process preferred path

If the current Omegon process still owns the child monitor:
- cancel uses the in-memory `CancellationToken` registry keyed by child label

### Restart fallback path

If the token registry is gone but the child was safely recovered:
- cancel may use persisted PID fallback semantics
- current behavior:
  - send `SIGTERM`
  - mark child failed/cancelled in durable state
  - clear PID/timestamps
  - refresh progress from persisted state

This is degraded local recovery, not identity-backed remote authority.

---

## Transport commands

### Daemon event ingress

Current typed event:

```json
{
  "event_id": "evt-...",
  "source": "...",
  "trigger_kind": "cancel-cleave-child",
  "payload": {
    "label": "alpha"
  }
}
```

### Web transport command

Current websocket command:

```json
{
  "type": "cancel_cleave_child",
  "label": "alpha"
}
```

### Main-loop routing

Current transport routing converts child cancel requests into the existing feature command path:
- `cleave cancel <label>`

This keeps one execution path for child cancel semantics.

---

## Activity continuity contract

### What is preserved

After restart, Omegon may reconstruct from durable activity logs:
- last tool
- last turn
- token counters observed in child log output
- last activity time

### What is not preserved

After restart, Omegon does **not** currently preserve:
- original `stderr`/`stdout` pipe ownership
- original live monitor task
- full stream continuity from the old process

This means current restart continuity is:
- **stateful degraded continuity via durable logs**
- **not true pipe reattachment**

---

## Deployment invariants

These semantics must hold the same way whether Omegon runs as:
- interactive TUI
- headless daemon/service
- VM-hosted process
- containerized runtime

Only the outer process supervisor changes.

The inner child-supervision semantics must remain stable.

---

## Non-goals for this schema

This control surface does **not** claim:
- cross-host trust
- cryptographic authority continuity
- distributed worker adoption
- mTLS identity-backed leases
- true subprocess stream reattachment after daemon replacement

Those belong to a later identity-backed supervisor model.

---

## Migration path to Styrene Identity

The current local bootstrap token exists to preserve a future seam.

Today:
- `supervisor_token` is a same-host continuity marker

Later:
- it can be replaced by an identity-backed lease/capability model
- likely under Styrene Identity mTLS
- without changing the higher-level runtime questions:
  - who owns the child?
  - who may adopt it?
  - who may cancel or reap it?

---

## Minimum correctness checklist

Before claiming local supervisor semantics are healthy, Omegon should satisfy:

- [ ] Spawn-time PID persistence
- [ ] Explicit supervision mode (`attached` vs `recovered_degraded`)
- [ ] Safe-adoption validation on restart
- [ ] Durable activity replay after restart
- [ ] Child-scoped cancel through feature command
- [ ] Child-scoped cancel through daemon/web transport
- [ ] Restart-path PID cancel fallback
- [ ] No surface implies attached continuity when only degraded continuity exists

---

## Current status

As of the current implementation:
- the local durable registry exists
- safe-adoption checks exist
- bootstrap supervisor token exists
- degraded activity replay exists
- transport child cancel exists
- explicit supervision modes exist

The remaining unsolved problem is **true subprocess reattachment**, not local-first control-surface definition.

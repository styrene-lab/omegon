---
title: Auspex ↔ Omegon daemon launch contract
status: decided
tags: [auspex, daemon, launch, contract, control-plane]
---

# Auspex ↔ Omegon daemon launch contract

This document defines the **process-launch contract** Auspex should use when it
needs to start and attach to a long-running Omegon daemon instance.

This is a different concern from [[auspex-ipc-contract|the native IPC attach contract]].
That document defines how an already-running Omegon process talks to Auspex over
IPC. This document defines how Auspex starts the Omegon process, detects
readiness, and discovers the control-plane metadata needed to attach.

## Scope

This contract covers:

- process invocation
- working directory selection
- provider/model selection
- authentication inputs
- readiness detection
- startup payload retrieval
- bearer-token discovery for the embedded localhost control plane
- recommended process supervision behavior

This contract does **not** define the native IPC message protocol. See
[[auspex-ipc-contract]].

## Normative status

For external launchers such as Auspex, the **public stable launch surface** is:

```bash
omegon serve
```

The hidden `embedded` subcommand currently resolves to the same implementation as
`serve` in `core/crates/omegon/src/main.rs`, but it is an internal alias and
should **not** be treated as the canonical external launcher interface.

Auspex should launch `omegon serve`, not `omegon embedded`, unless Omegon later
publishes a versioned internal contract explicitly intended for launcher use.

## Execution model

A daemon instance is a **single persistent Omegon process** with:

- one long-lived server/control-plane
- one active session/runtime surface
- optional queued event ingress
- local control via IPC and/or localhost compatibility surfaces

Auspex is expected to manage **multiple Omegon processes externally** if it
wants multiple independent long-running instances. Omegon daemon v1 does not
promise multi-instance management inside one process.

See [[omega-daemon-runtime]].

## Launch command

Minimum contract:

```bash
omegon serve --cwd <project-root>
```

Typical explicit launch:

```bash
omegon \
  --cwd <project-root> \
  --model <provider:model> \
  serve \
  --control-port <port> \
  --strict-port
```

### Required launcher-owned inputs

| Input | Required | Meaning |
|---|---:|---|
| `cwd` | yes | Project/workspace root for this daemon instance |
| `model` | no, but strongly recommended | Explicit execution route for this instance |
| `control_port` | no | Preferred localhost port for the embedded control plane |
| `strict_port` | no | Require exact port instead of fallback |
| `log_level` | no | Runtime logging verbosity |
| `log_file` | no | Log sink for supervised daemon instances |

### Strong recommendation: always pass an explicit model

Auspex should prefer an explicit `provider:model` string instead of relying on
provider inference from a bare model ID.

Good:

```bash
--model ollama-cloud:gpt-oss:120b-cloud
--model ollama-cloud:qwen3-coder:480b-cloud
--model anthropic:claude-sonnet-4-6
```

Avoid for launcher contracts:

```bash
--model gpt-oss:120b-cloud
--model qwen3-coder:480b-cloud
```

Those may resolve today, but external launchers should not depend on heuristic
provider inference when they can provide the exact route.

## Hosted Ollama contract

Hosted Ollama is a distinct provider:

- provider id: `ollama-cloud`
- auth env var: `OLLAMA_API_KEY`
- hosted base URL: `https://ollama.com/api`

It is **not** the same as local `ollama`.

### Example: dedicated long-running hosted Ollama daemon

```bash
OLLAMA_API_KEY=... \
omegon \
  --cwd /path/to/project \
  --model ollama-cloud:gpt-oss:120b-cloud \
  serve \
  --control-port 7842 \
  --strict-port \
  --log-file /tmp/omegon-ollama-cloud.log
```

## Authentication inputs

Auspex may choose whatever valid config it wants. Omegon must not require a
specific human-driven setup path when equivalent machine-supplied config exists.

### Supported launcher-side auth strategies

1. **Environment variables** — preferred for supervised launches
2. **Previously stored Omegon credentials** — acceptable when the operator has
   already authenticated
3. **Interactive TUI login flows** — operator-driven fallback, not required for
   Auspex-managed launch

### Known provider env vars relevant to long-running inference

| Provider | Env var |
|---|---|
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |
| OpenRouter | `OPENROUTER_API_KEY` |
| Ollama Cloud | `OLLAMA_API_KEY` |
| Local Ollama | `OLLAMA_HOST` |

For dedicated Auspex-managed daemon instances, environment variables are the
cleanest contract because they are explicit, process-local, and compatible with
service managers.

## Readiness and startup discovery

`omegon serve` is a **blocking long-running process**. Launchers must not use
process exit as a readiness signal.

### Correct readiness flow

1. Spawn `omegon serve` with stdout captured.
2. Read the **first stdout line**.
3. Parse it as JSON startup metadata.
4. Fetch `GET /api/startup`.
5. Poll `GET /api/readyz` until ready if needed.
6. Use the returned discovery URLs and bearer token to attach to the localhost
   compatibility control plane.

This behavior is verified by the daemon blackbox test in
`core/crates/omegon/tests/daemon_serve_blackbox.rs`.

### First stdout line contract

On successful startup, Omegon prints a single JSON line like:

```json
{
  "type": "omegon.startup",
  "schema_version": 2,
  "pid": 12345,
  "http_base": "http://127.0.0.1:7842",
  "startup_url": "http://127.0.0.1:7842/api/startup",
  "health_url": "http://127.0.0.1:7842/api/healthz",
  "ready_url": "http://127.0.0.1:7842/api/readyz",
  "ws_url": "ws://127.0.0.1:7842/ws?token=...",
  "auth_mode": "ephemeral-bearer",
  "auth_source": "generated"
}
```

Auspex should treat this stdout JSON line as the initial discovery envelope,
not as the complete attach payload.

## `/api/startup` contract

After receiving the stdout discovery line, Auspex should fetch:

```text
GET /api/startup
```

The startup payload currently includes:

- `schema_version`
- `addr`
- `http_base`
- `state_url`
- `startup_url`
- `health_url`
- `ready_url`
- `ws_url`
- `token`
- `auth_mode`
- `auth_source`
- `control_plane_state`
- `daemon_status`
- `instance_descriptor`

### Required launcher behavior

Auspex should treat `/api/startup` as the authoritative source for:

- actual bound port
- bearer token
- readiness/state URLs
- websocket URL
- canonical instance descriptor / version identity

Do not reconstruct these values from CLI flags once the process is running.
The daemon may bind a fallback port when `--strict-port` is not set.

## Health and readiness probes

Use:

- `GET /api/healthz` — liveness
- `GET /api/readyz` — readiness

### Launcher policy

- If the process exits before emitting startup JSON, launch failed.
- If startup JSON is emitted but `/api/startup` cannot be fetched before the
  launcher deadline, launch failed.
- If `/api/startup` succeeds but `/api/readyz` is not ready yet, the process is
  alive but not yet attachable.
- If `--strict-port` is set and the port is unavailable, treat startup failure
  as terminal unless the operator/config explicitly allows a retry on another
  port.

## Attach policy

For Auspex-native attachment, the long-term canonical contract remains the IPC
surface described in [[auspex-ipc-contract]].

The embedded localhost HTTP/WebSocket surface is a valid **bootstrap and
compatibility discovery plane** for launchers because it exposes:

- startup metadata
- health/readiness
- event ingress
- websocket discovery URL
- bearer token
- instance descriptor

Launcher takeaway:

- use `omegon serve` to start the process
- use stdout JSON + `/api/startup` + `/api/readyz` to discover and verify it
- then attach using the transport appropriate to the client mode

## Supervision recommendations

Auspex or the host service manager should supervise the Omegon process.

Recommended practices:

- capture stdout and stderr separately
- persist logs via `--log-file` for long-running instances
- kill by PID or process handle, not broad pattern matching
- treat the working directory as part of instance identity
- run one process per project/workspace instance

## Non-goals

This contract does **not** promise:

- multiple named daemon instances inside one Omegon process
- remote/public network exposure of the localhost compatibility surface
- that hidden/internal aliases such as `embedded` are stable for external use
- that provider inference from bare model IDs is a stable launcher contract

## Minimal Auspex algorithm

```text
spawn omegon serve with explicit cwd/model/config
capture stdout
read first line
parse omegon.startup JSON
GET startup_url
extract token + instance_descriptor + URLs
poll ready_url until ready or deadline
attach using IPC/native client or compatibility websocket as appropriate
supervise process lifetime externally
```

## Reference implementation

A minimal reference launcher implementing this contract lives at:

- `scripts/launch_omegon_daemon.py`

It is intentionally simple and suitable for integration tests, fixture wiring,
or as a starting point for Auspex-side process orchestration.

## Source of truth

This launch contract is grounded in:

- `core/crates/omegon/src/main.rs`
- `core/crates/omegon/src/web/mod.rs`
- `core/crates/omegon/src/web/api.rs`
- `core/crates/omegon/tests/daemon_serve_blackbox.rs`

Related docs:

- [[auspex-ipc-contract]]
- [[auspex-startup-version-handoff]]
- [[omega-daemon-runtime]]

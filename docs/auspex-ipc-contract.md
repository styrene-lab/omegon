+++
id = "f7bd8ab4-3747-4ffa-b8dd-de54e36b62ca"
kind = "document"
title = "Auspex native IPC contract — Omegon-side v1"
status = "decided"
tags = ["auspex", "ipc", "msgpack", "unix-socket", "contract"]
aliases = ["auspex-ipc-contract"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
priority = "1"
+++

# Auspex native IPC contract — Omegon-side v1

See also [[auspex-omegon-launch-contract]] for the process-launch and startup
readiness contract Auspex should use before IPC attach.

## Scope

This document defines the **native host IPC contract** between Omegon (server)
and Auspex (client) for rc.20 and forward.

Auspex should implement against this document and the Rust types in
`omegon-traits`. If they disagree, the Rust types are authoritative.

This contract is intentionally **not** the embedded HTTP/WebSocket surface.
HTTP/WebSocket remains available for browser/debug/local-compatibility use.
Auspex-native integration targets this IPC layer exclusively.

## Migration note for legacy `/dash` consumers

If an existing Auspex-adjacent consumer still speaks to Omegon's embedded web
surface, treat that surface as a temporary local browser/debug transport, not
the canonical Auspex backend contract.

Today the canonical guarantees for Auspex are the IPC types above plus the Rust
implementations in `core/crates/omegon/src/ipc/*`.

By contrast, the embedded web surface in `core/crates/omegon/src/web/*` is still
intentionally narrower and UI-oriented:

- `GET /api/state` currently exposes `design`, `openspec`, `cleave`, and
  `session` only; it does **not** expose the IPC-level `harness` or `health`
  sections.
- `GET /api/startup` is discovery metadata for the embedded localhost browser
  compatibility surface (`schema_version`, URLs, auth token/mode/source,
  control-plane state), not an Auspex session handshake.
- `WS /ws` currently emits legacy snake_case event names via a `type` field
  (for example `turn_start`, `turn_end`, `tool_end`, `harness_status_changed`,
  `context_updated`) and sends `state_snapshot` frames on attach or explicit
  snapshot request.
- The web socket command set is limited to `user_prompt`, `slash_command`, and
  `cancel`, with `request_snapshot` as a local browser refresh helper.

Migration guidance:

1. New Auspex integrations should attach over the native IPC contract, not the
   embedded `/dash` HTTP/WebSocket endpoints.
2. Existing `/dash`-style consumers may continue to use the web surface as a
   local compatibility/debug channel, but they should not assume IPC field
   parity.
3. Documentation should frame Auspex as the primary browser surface, with the
   embedded web surface treated as a separate Omegon-local compatibility
   protocol.

---

## Transport

- **Protocol:** Unix domain socket
- **Platform:** macOS and Linux only for v1; Windows is out of scope
- **Socket path:** resolved at startup, written to `HelloResponse.cwd`-relative
  runtime dir or configured via environment/settings
- **Socket permissions:** mode `0600`, same-user only
- **Connections:** single controlling client; a second attach is rejected with
  `IpcErrorCode::Busy`

---

## Framing

Every message is a **length-prefixed MessagePack frame**:

```
[u32 BE: payload length][msgpack bytes: IpcEnvelope]
```

- `u32` big-endian unsigned integer specifying the byte count of the payload
- maximum frame size: **8 MiB** (`IPC_MAX_FRAME_BYTES`)
- frames exceeding 8 MiB are a protocol violation — server disconnects the client
- malformed MessagePack is a protocol violation — server sends an error envelope
  then disconnects
- each frame decodes to exactly one `IpcEnvelope`
- writes are atomic per frame; partial frames are never emitted

---

## Envelope

Every frame encodes one `IpcEnvelope`:

| Field              | Type              | Required | Notes                                         |
|--------------------|-------------------|----------|-----------------------------------------------|
| `protocol_version` | `u16`             | yes      | Must equal `IPC_PROTOCOL_VERSION` (1)          |
| `kind`             | `IpcEnvelopeKind` | yes      | `hello`, `request`, `response`, `event`, `error` |
| `request_id`       | `[u8; 16] \| null` | cond.   | Required on `request`; echoed on `response`/`error` |
| `method`           | `string \| null`  | cond.    | Required on `request`/`response`              |
| `payload`          | `object \| null`  | cond.    | Typed per method/event                        |
| `error`            | `IpcError \| null`| cond.    | Present on `error` kind only                  |

### Correlation rules

- `hello` — omit `request_id`
- `request` — must include `request_id`
- `response` — must echo originating `request_id`
- `event` — omit `request_id`
- `error` — echo `request_id` if tied to a request; null otherwise

---

## Error codes (`IpcErrorCode`)

| Code                          | Meaning                                          |
|-------------------------------|--------------------------------------------------|
| `unsupported_protocol_version`| Client proposed versions that don't include v1  |
| `unknown_method`              | Method name not recognised                       |
| `invalid_payload`             | Payload failed to deserialize                    |
| `internal_error`              | Unexpected server-side failure                   |
| `not_subscribed`              | Unsubscribe for an event the client didn't hold  |
| `busy`                        | A second controller attempted to attach          |
| `turn_in_progress`            | `submit_prompt` while a turn is already running  |
| `shutdown_initiated`          | `shutdown` was accepted; process will exit       |

Unknown error codes must be treated by the client as `internal_error`.

---

## Handshake

The handshake must be the **first exchange** on every connection.

### 1. Client sends `hello`

```
kind = hello
method = "hello"
payload = HelloRequest
```

**`HelloRequest`**

| Field                          | Type       | Notes                                        |
|--------------------------------|------------|----------------------------------------------|
| `client_name`                  | `string`   | e.g. `"auspex"`                              |
| `client_version`               | `string`   | SemVer string                                |
| `supported_protocol_versions`  | `u16[]`    | In preference order; server picks highest    |
| `capabilities`                 | `string[]` | Informational; what the client can handle    |

### 2. Server replies `response`

```
kind = response
method = "hello"
payload = HelloResponse
```

**`HelloResponse`**

| Field                  | Type       | Notes                                                  |
|------------------------|------------|--------------------------------------------------------|
| `protocol_version`     | `u16`      | Negotiated version (≤ client max and ≤ server max)     |
| `omegon_version`       | `string`   | Application version string                             |
| `server_name`          | `string`   | Always `"omegon"`                                      |
| `server_pid`           | `u32`      | OS process ID                                          |
| `cwd`                  | `string`   | Working directory                                      |
| `server_instance_id`   | `string`   | Stable opaque ID for this process lifetime; changes on restart |
| `started_at`           | `string`   | RFC 3339 UTC timestamp when process started            |
| `session_id`           | `string \| null` | Active session ID if one exists                  |
| `capabilities`         | `string[]` | Server-advertised capabilities (see below)             |

If the server cannot agree on a protocol version it returns
`IpcErrorCode::UnsupportedProtocolVersion` and closes the connection.

### Version negotiation rule

Server selects the highest version present in **both** lists.
If the intersection is empty, the handshake fails.

---

## Capabilities (`IpcCapability`)

Capabilities are advertised by the server in `HelloResponse.capabilities`.
The client **must not** rely on any capability not in that list.

| Token               | Meaning                                     |
|---------------------|---------------------------------------------|
| `state.snapshot`    | `get_state` is available                    |
| `events.stream`     | Server pushes `IpcEventPayload` events      |
| `prompt.submit`     | `submit_prompt` is available                |
| `turn.cancel`       | `cancel` is available                       |
| `graph.read`        | `get_graph` is available                    |
| `slash_commands`    | `run_slash_command` is available            |
| `shutdown`          | `shutdown` is available                     |

A v1 server advertises all seven tokens.

### Stability rule for capabilities

- Adding a new capability token is **non-breaking**
- Removing a capability token is a **breaking change** and requires a protocol version bump
- The meaning of a token must not change without a version bump

---

## Methods (v1 required)

### `ping`

Request: `PingRequest { nonce: string }`
Response: `PingResponse { nonce: string }` — echoes the nonce.

### `get_state`

Request: empty object
Response: `IpcStateSnapshot`

Returns a full attach-time snapshot. All sections are guaranteed present.
Subscribe to `state.changed` events to know when sections become stale.

### `submit_prompt`

Request: `SubmitPromptRequest { prompt: string, source?: string }`
Response: `AcceptedResponse { accepted: bool }`

Rejected with `turn_in_progress` if a turn is already running.

### `cancel`

Request: empty object
Response: `AcceptedResponse { accepted: bool }`

Cancels the current turn if one is running. No-op if idle.

### `subscribe`

Request: `SubscriptionRequest { events: string[] }`
Response: `SubscriptionResponse { events: string[] }` — the set that was activated.

Subscribe to one or more event names. Events not in the server's event set
are silently dropped from the response list (not an error).

### `unsubscribe`

Request: `SubscriptionRequest { events: string[] }`
Response: `SubscriptionResponse { events: string[] }`

### `get_graph`

Request: empty object
Response: graph snapshot (shape mirrors existing `/api/graph` JSON — see `web/api.rs`)

### `run_slash_command`

Request: `SlashCommandRequest { name: string, args: string }`
Response: `SlashCommandResponse { accepted: bool, output?: string }`

### `shutdown`

Request: empty object
Response: `AcceptedResponse { accepted: true }`
Then: server closes connection and exits.

---

## State snapshot (`IpcStateSnapshot`)

All fields are required. No `Value` blobs.

```
IpcStateSnapshot {
  schema_version: u16          // always 1 for v1
  omegon_version: string
  session:     IpcSessionSnapshot
  design_tree: IpcDesignTreeSnapshot
  openspec:    IpcOpenSpecSnapshot
  cleave:      IpcCleaveSnapshot
  harness:     IpcHarnessSnapshot
  health:      IpcHealthSnapshot
}
```

### `IpcSessionSnapshot`

| Field          | Type              | Notes                              |
|----------------|-------------------|------------------------------------|
| `cwd`          | `string`          |                                    |
| `pid`          | `u32`             |                                    |
| `started_at`   | `string`          | RFC 3339 UTC                       |
| `turns`        | `u32`             |                                    |
| `tool_calls`   | `u32`             |                                    |
| `compactions`  | `u32`             |                                    |
| `busy`         | `bool`            | true while a turn is in progress   |
| `git_branch`   | `string \| null`  |                                    |
| `git_detached` | `bool`            |                                    |
| `session_id`   | `string \| null`  |                                    |

### `IpcDesignTreeSnapshot`

```
counts:       IpcDesignCounts
focused:      IpcFocusedNode | null
implementing: IpcNodeBrief[]
actionable:   IpcNodeBrief[]
nodes:        IpcNodeBrief[]
```

### `IpcHarnessSnapshot`

Intentionally curated. Not every internal `HarnessStatus` field is exposed.

| Field                  | Type                    |
|------------------------|-------------------------|
| `context_class`        | `string`                |
| `thinking_level`       | `string`                |
| `capability_tier`      | `string`                |
| `memory_available`     | `bool`                  |
| `cleave_available`     | `bool`                  |
| `memory_warning`       | `string \| null`        |
| `memory`               | `IpcMemorySnapshot`     |
| `providers`            | `IpcProviderSnapshot[]` |
| `mcp_server_count`     | `u32`                   |
| `mcp_tool_count`       | `u32`                   |
| `active_persona`       | `string \| null`        |
| `active_tone`          | `string \| null`        |
| `active_delegate_count`| `u32`                   |

### `IpcHealthSnapshot`

| Field          | Type             | Notes               |
|----------------|------------------|---------------------|
| `state`        | `IpcHealthState` | `ready \| degraded \| starting \| failed` |
| `memory_ok`    | `bool`           |                     |
| `provider_ok`  | `bool`           |                     |
| `checked_at`   | `string`         | RFC 3339 UTC        |

---

## Event stream (`IpcEventPayload`)

Events are pushed as `kind = event` envelopes with no `request_id`.

Wire shape uses **adjacent tagging**:
```json
{"name": "turn.started", "data": {"turn": 7}}
```

Unit events (no payload) omit the `data` key:
```json
{"name": "agent.completed"}
```

### Ordering guarantee

Events are delivered **in-order** on a single connection. The client must not
reorder them.

### `state.changed` semantics

`sections` lists which top-level keys of `IpcStateSnapshot` are stale.
The client should call `get_state` and refresh only those sections.

Possible section names: `session`, `design_tree`, `openspec`, `cleave`,
`harness`, `health`.

### Backpressure

If the client cannot consume events fast enough:
- Server maintains a bounded per-client queue
- If the queue fills, the server closes the connection
- Coalescing: multiple `state.changed` events for the same sections may be
  coalesced into one before delivery; `message.delta` and `thinking.delta`
  are never coalesced

### Event names (v1)

| Name                              | Payload fields                              |
|-----------------------------------|---------------------------------------------|
| `turn.started`                    | `turn: u32`                                 |
| `turn.ended`                      | `turn: u32, estimated_tokens: usize`        |
| `message.delta`                   | `text: string`                              |
| `thinking.delta`                  | `text: string`                              |
| `message.completed`               | (none)                                      |
| `tool.started`                    | `id, name, args`                            |
| `tool.updated`                    | `id`                                        |
| `tool.ended`                      | `id, name, is_error, summary?`              |
| `agent.completed`                 | (none)                                      |
| `phase.changed`                   | `phase: string`                             |
| `decomposition.started`           | `children: string[]`                        |
| `decomposition.child_completed`   | `label, success`                            |
| `decomposition.completed`         | `merged: bool`                              |
| `harness.changed`                 | (none) — call `get_state` for details       |
| `state.changed`                   | `sections: string[]`                        |
| `system.notification`             | `message: string`                           |
| `session.reset`                   | (none)                                      |

### Unknown event names

The client must **silently ignore** unknown event names. This is the primary
forward-compatibility mechanism.

---

## Connection and session semantics

### Single controller model

v1 supports exactly one controlling client. A second attach attempt is rejected
immediately with `IpcErrorCode::Busy` in the hello response.

### Reconnect

A disconnected Auspex may reconnect at any time. The server continues running.
On reconnect, Auspex must:
1. Send a new hello
2. Verify `server_instance_id` matches the previous connection
3. If `server_instance_id` changed: treat as a fresh session, clear all local state
4. If `server_instance_id` matches: re-subscribe to desired events and call `get_state`

### Disconnect semantics

If Auspex disconnects (EOF on socket):
- any active turn is **cancelled gracefully**
- the server process continues running
- the socket remains open for reconnect

### Shutdown

If Auspex calls `shutdown`:
- server returns `AcceptedResponse { accepted: true }`
- server closes the connection
- server process exits

---

## Security model

- Socket file mode: `0600` (owner read/write only)
- No token or credential is required on the socket — OS-level same-user
  enforcement is the auth boundary
- The socket must not be group- or world-readable; server verifies this at bind
- Future: peer credential check via `SO_PEERCRED` / `LOCAL_PEERCRED` may be
  added as an optional hardening layer

---

## Protocol evolution rules

These rules govern how the contract may change without breaking existing clients:

| Change type                                    | Breaking? |
|------------------------------------------------|-----------|
| Adding an optional field to any struct         | No        |
| Adding a new event variant                     | No        |
| Adding a new method                            | No        |
| Adding a new capability token                  | No        |
| Adding a new `IpcErrorCode` variant            | No        |
| Removing a field from any struct               | **Yes**   |
| Removing an event variant                      | **Yes**   |
| Removing a method                              | **Yes**   |
| Removing a capability token                    | **Yes**   |
| Changing the type or meaning of any field      | **Yes**   |
| Changing the meaning of an error code          | **Yes**   |
| Changing `IpcEnvelopeKind` variants            | **Yes**   |

All breaking changes require a `IPC_PROTOCOL_VERSION` bump.

---

## Authoritative source

The Rust types in `omegon-traits` are the authoritative definition.
This document describes what those types mean. If they disagree, fix the doc.

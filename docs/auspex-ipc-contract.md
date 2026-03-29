---
id: auspex-ipc-contract
title: Auspex native IPC contract — Omegon-side v1
status: decided
tags: [auspex, ipc, msgpack, unix-socket, contract]
open_questions: []
issue_type: feature
priority: 1
---

# Auspex native IPC contract — Omegon-side v1

## Purpose

Define the **native host IPC contract** that Auspex can implement against for rc.20.

This is the Omegon-owned contract for:
- local native desktop integration
- a supervised child/daemon process boundary
- independent Auspex/Omegon updates through explicit protocol versioning

This contract is intentionally **not** the embedded HTTP/WebSocket contract.
That surface can remain for browser/debug compatibility, but Auspex-native integration targets this IPC layer.

## Transport

v1 transport is:
- **Unix domain socket** on macOS/Linux
- local-only
- one persistent bidirectional connection per client

Recommended default socket path shape:
- `${XDG_RUNTIME_DIR}/omegon/omegon.sock` when available
- fallback under per-user app support/runtime directory

Windows transport is out of scope for v1.

## Framing

Each frame is:
- `u32` big-endian payload length prefix
- followed by a MessagePack-encoded envelope

Unlike Styrene's older split header/body shape, Omegon v1 keeps the whole logical envelope inside the MessagePack body. That keeps the contract easier to evolve while preserving a deterministic framed stream.

## Envelope

Every frame decodes to one `IpcEnvelope`.

```text
[length: u32 be][msgpack envelope bytes]
```

Envelope fields:
- `protocol_version: u16`
- `kind: EnvelopeKind`
- `request_id: [u8; 16] | null`
- `method: string | null`
- `payload: object | null`
- `error: IpcError | null`

### Envelope kind

- `hello`
- `request`
- `response`
- `event`
- `error`

### Correlation rules

- `hello` may omit `request_id`
- `request` must include `request_id`
- `response` must echo the originating `request_id`
- `event` omits `request_id`
- `error` echoes `request_id` when tied to a request, otherwise null

## Handshake

Client sends:
- `kind = hello`
- `method = "hello"`
- payload shaped like `HelloRequest`

Server replies:
- `kind = response`
- `method = "hello"`
- payload shaped like `HelloResponse`

### HelloRequest

Required fields:
- `client_name: string`
- `client_version: string`
- `supported_protocol_versions: number[]`
- `capabilities: string[]`

### HelloResponse

Required fields:
- `protocol_version: number`
- `omegon_version: string`
- `server_name: string`
- `server_pid: number`
- `cwd: string`
- `session_id: string | null`
- `capabilities: string[]`

### Initial server capabilities

v1 capability names:
- `state.snapshot`
- `events.stream`
- `prompt.submit`
- `turn.cancel`
- `graph.read`

## Methods

v1 required request methods:
- `ping`
- `get_state`
- `submit_prompt`
- `cancel`
- `subscribe`
- `unsubscribe`

v1 recommended:
- `get_graph`

## Requests and responses

### `ping`
Request payload:
- `nonce: string`

Response payload:
- `nonce: string`

### `get_state`
Request payload:
- empty object

Response payload:
- `StateSnapshot`

### `submit_prompt`
Request payload:
- `prompt: string`
- `source: string | null`

Response payload:
- `accepted: bool`

### `cancel`
Request payload:
- empty object

Response payload:
- `accepted: bool`

### `subscribe`
Request payload:
- `events: string[]`

Response payload:
- `subscribed: string[]`

### `unsubscribe`
Request payload:
- `events: string[]`

Response payload:
- `unsubscribed: string[]`

### `get_graph`
Request payload:
- empty object

Response payload:
- graph snapshot matching Omegon's current graph domain shape

## Event stream

Events are pushed as `kind = event` envelopes.

v1 event names:
- `turn.started`
- `turn.ended`
- `message.delta`
- `thinking.delta`
- `message.completed`
- `tool.started`
- `tool.updated`
- `tool.ended`
- `agent.completed`
- `phase.changed`
- `decomposition.started`
- `decomposition.child_completed`
- `decomposition.completed`
- `system.notification`
- `harness.changed`
- `session.reset`
- `state.changed`

### Projection rule

Omegon already has an internal `AgentEvent` stream. v1 IPC events are a projection of that stream into stable transport-facing names and payloads.

This is deliberate:
- runtime internals keep their native event model
- the IPC boundary gets explicit, versioned names
- Auspex does not bind directly to Rust enum variant names

## State snapshot contract

`get_state` returns a normalized snapshot suitable for initial attach.

Required top-level fields:
- `schema_version`
- `omegon_version`
- `session`
- `design_tree`
- `openspec`
- `cleave`
- `harness`
- `health`

This is the native IPC equivalent of the HTTP control-plane snapshot and is intended to become the authoritative attach-time state.

## Error contract

Errors are sent as `kind = error` with payload:
- `code: string`
- `message: string`
- `details: object | null`

v1 error codes:
- `unsupported_protocol_version`
- `unknown_method`
- `invalid_payload`
- `internal_error`
- `not_subscribed`
- `busy`

## Stability rule

For rc.20, Auspex should target:
- this document
- and the Rust contract types in `omegon-traits`

If implementation behavior disagrees with this doc, the Rust contract types are authoritative and the doc must be reconciled immediately.

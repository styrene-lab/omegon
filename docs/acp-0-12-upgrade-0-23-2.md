---
id: acp-0-12-upgrade-0-23-2
title: "ACP 0.12 upgrade for 0.23.2"
status: deferred
tags: [acp, zed, 0.23.2, dependency-upgrade]
open_questions:
  - "[assumption] Zed's bundled ACP host supports agent-client-protocol 0.12.x / schema 0.13.x without requiring protocol downgrade behavior from Omegon."
  - "Which new ACP 0.12/schema 0.13 capabilities should Omegon adopt in 0.23.2, if any, beyond a compatibility upgrade? Candidates include unstable protocol v2, cancellation, elicitation, LLM provider/session model metadata, session usage, additional directories, and MCP-over-ACP."
  - "What compile/API breakages occur when changing `agent-client-protocol` from 0.10 to 0.12 with the current feature set, and are they isolated to `acp.rs`/ACP helpers?"
  - "Should Omegon migrate its ACP server implementation to the 0.12 builder/handler API (`Agent.builder().on_receive_request/...`) in-place, or introduce a compatibility adapter module that preserves the current `OmegonAcpAgent` shape while using the new SDK under the hood?"
  - "Can the 0.12 SDK provide a non-`Send`/local handler registration path, or must Omegon introduce a `Send` actor boundary for every ACP request handler?"
  - "Should WebSocket ACP be migrated in the same 0.23.2 slice as stdio ACP, or explicitly deferred behind a compile-preserving compatibility shim after stdio is stable?"
  - "[recurring] Before each 0.23.x patch and at least monthly while ACP is moving, re-scan crates.io and upstream docs for `agent-client-protocol`, `agent-client-protocol-schema`, protocolVersion changes, new capabilities, and plan/status schema changes."
dependencies: []
related: []
---

# ACP 0.12 upgrade for 0.23.2

## Overview

Upgrade Omegon's Zed ACP integration from agent-client-protocol 0.10.x / schema 0.11.x to the current 0.12.x / schema 0.13.x line after completing the immediate plan-surface work. The upgrade should preserve existing Zed behavior, assess newly available ACP capabilities, and decide which richer integration surfaces should be adopted in 0.23.2 versus deferred.

## Research

### Current dependency scan

Omegon currently declares `agent-client-protocol = { version = "0.10", features = ["unstable_session_close"] }` in `core/crates/omegon/Cargo.toml`, resolved as `agent-client-protocol 0.10.4` and `agent-client-protocol-schema 0.11.4` in `Cargo.lock`. Crates.io currently reports `agent-client-protocol 0.12.1` and `agent-client-protocol-schema 0.13.2`.

### Plan status capability check

Downloaded `agent-client-protocol-schema 0.13.2` still defines only `PlanEntryStatus::{Pending, InProgress, Completed}` in both `src/v1/plan.rs` and `src/v2/plan.rs`. Therefore the upgrade does not solve skipped/failed plan status representation; Omegon must continue mapping skipped/failed locally or encode semantics in labels/content if needed.

### Upstream versioning model

Upstream ACP docs state that the current stable ACP protocol version is 1 and that Rust crate/schema package versions describe SDK and schema artifact compatibility, not wire compatibility. ACP wire compatibility is negotiated separately through `initialize.protocolVersion`; optional features are negotiated through capabilities. Therefore upgrading `agent-client-protocol` from 0.10.x to 0.12.x should be treated as an SDK/API surface update unless the negotiated protocol version changes.

### Initial 0.12 upgrade attempt

Changing `agent-client-protocol` to 0.12 with the broad `unstable` feature set does not compile as a drop-in update. The 0.12 SDK moves schema types under `agent_client_protocol::schema::*` and replaces/removes the old root-level `Agent` trait / `AgentSideConnection` style used by Omegon. The new examples use `Agent.builder().on_receive_request(...).connect_to(Stdio::new())`, so upgrading requires an ACP transport adaptation/refactor rather than only import fixes. The attempt was reverted to keep the tree buildable.

### Concurrency boundary finding

A second adversarial spike showed the migration is primarily a concurrency-boundary change. The 0.12 builder registration requires request/notification handler closures to be `Send`; Omegon's current ACP session is intentionally local-threaded around `Rc<RefCell<_>>`, `LocalSet`, and `spawn_local`. Directly capturing `Rc<OmegonAcpAgent>` in `Agent.builder().on_receive_request(...)` fails the handler bounds. Therefore a successful migration must either find a local/non-`Send` handler surface in the upstream SDK or introduce a `Send` actor/channel boundary so builder handlers only send requests into the existing local ACP session owner.

### Client operation API finding

The 0.12 SDK removes or stops exposing the old convenience methods Omegon uses on `AgentSideConnection`, including `session_notification(...)`, `read_text_file(...)`, `write_text_file(...)`, terminal helpers, and `request_permission(...)`. The replacement surface is generic JSON-RPC messaging: `ConnectionTo<Client>::send_notification(SessionNotification::new(...))` for session updates and `ConnectionTo<Client>::send_request(Request).block_task().await` for host requests. The wrapper functions introduced in `acp.rs` and `host_context.rs` are the right seam for this change.

### Session configuration finding

`SetSessionModelRequest` does not map cleanly in the 0.12 schema. The current model/thinking/posture dropdown flow should migrate through `SetSessionConfigOptionRequest`; its `value` is now a `SessionConfigOptionValue` enum (`ValueId { value }` or `Boolean { value }` under the unstable boolean config feature) rather than a tuple wrapper. This requires a small value extraction helper and tests for model/thinking/posture updates.

### WebSocket transport finding

The stdio ACP path can use the 0.12 `Stdio` transport directly. The WebSocket endpoint in `core/crates/omegon/src/web/acp_ws.rs` currently bridges WebSocket frames through duplex streams and the old `AgentSideConnection::new(...)` constructor. Under 0.12 it needs a `ConnectTo` adapter for channel/byte-stream backed transport, or it should be migrated after stdio ACP is stable. Treating stdio and WebSocket as one slice increases risk.

## Decisions

### Keep the first 0.12 upgrade compatibility-focused

**Status:** proposed

**Rationale:** The immediate 0.23.2 goal is stabilizing Zed ACP behavior. Because the latest schema does not add richer plan statuses and slash command polish is a separate UX/plumbing issue, the upgrade should first preserve behavior and only adopt new capabilities after targeted evidence shows low integration risk.

### Upgrade SDK surface while preserving protocolVersion behavior

**Status:** accepted

**Rationale:** The crate is the official Rust SDK, so keeping Omegon close to its current interfaces reduces drift. Because crate version and wire protocol version are decoupled upstream, the upgrade should preserve negotiated `protocolVersion` behavior and advertised capabilities first, then adopt optional capabilities only with explicit tests.

### Use an adapter-first migration instead of rewriting ACP worker logic

**Status:** accepted

**Rationale:** The initial 0.12 attempt showed the main breakage is the SDK connection/handler API, not the worker/session logic. Preserving `OmegonAcpAgent` internals and adapting the transport registration layer reduces risk and keeps behavior comparison possible.

### Migrate through a Send-safe actor boundary

**Status:** proposed

**Rationale:** The 0.12 builder API requires `Send` handlers, while the current ACP session uses `Rc<RefCell<_>>` and `LocalSet`. Converting all ACP state directly to `Arc<Mutex<_>>` would broaden lock/lifetime risk. A narrower actor boundary lets `Send` handlers forward typed requests over channels to the existing local ACP session owner, preserving current semantics while satisfying the SDK boundary.

### Stabilize stdio ACP before WebSocket ACP

**Status:** proposed

**Rationale:** Stdio can use the upstream 0.12 `Stdio` transport directly. WebSocket requires a custom `ConnectTo`/byte-stream adapter and should not block proving the core request/notification migration unless release scope explicitly requires network ACP parity in the same patch.

## Implementation Plan

### Phase 1 — Confirm adapter isolation on 0.10

- Keep `agent-client-protocol 0.10` while finishing seams.
- Ensure SDK-specific calls are isolated behind:
  - `send_session_update(...)` in `core/crates/omegon/src/acp.rs`
  - host operation wrappers in `core/crates/omegon/src/host_context.rs`
  - `connect_acp_agent(...)` for server construction
- Audit for direct uses of `AgentSideConnection::new`, `.session_notification(...)`, `.read_text_file(...)`, `.write_text_file(...)`, `.request_permission(...)`, and terminal helper methods outside wrappers.

Acceptance:

```bash
rg "AgentSideConnection::new|session_notification\\(|\\.read_text_file\\(|\\.write_text_file\\(|\\.request_permission\\(" core/crates/omegon/src
cargo test -p omegon acp --bin omegon
```

### Phase 2 — Introduce ACP request actor

Create a `Send`-safe request handle whose handlers can clone and use from the 0.12 builder:

```rust
struct AcpSessionActor {
    tx: tokio::sync::mpsc::Sender<AcpRequest>,
}

enum AcpRequest {
    Initialize { args: InitializeRequest, reply: oneshot::Sender<Result<InitializeResponse>> },
    Authenticate { args: AuthenticateRequest, reply: oneshot::Sender<Result<AuthenticateResponse>> },
    NewSession { args: NewSessionRequest, reply: oneshot::Sender<Result<NewSessionResponse>> },
    Prompt { args: PromptRequest, reply: oneshot::Sender<Result<PromptResponse>> },
    SetSessionMode { args: SetSessionModeRequest, reply: oneshot::Sender<Result<SetSessionModeResponse>> },
    SetSessionConfigOption { args: SetSessionConfigOptionRequest, reply: oneshot::Sender<Result<SetSessionConfigOptionResponse>> },
    LoadSession { args: LoadSessionRequest, reply: oneshot::Sender<Result<LoadSessionResponse>> },
    Cancel { args: CancelNotification },
}
```

The actor runs on the existing local ACP thread and owns the current `Rc<OmegonAcpAgent>`/local state. Builder handlers only await actor replies.

Acceptance:

- Existing 0.10 build and tests still pass.
- Actor tests cover initialize, new session, slash prompt, and config update roundtrips.

### Phase 3 — Bump to ACP 0.12 for stdio

- Change `core/crates/omegon/Cargo.toml` to `agent-client-protocol = { version = "0.12", features = ["unstable"] }` and pin/update schema as needed.
- Import schema types from `agent_client_protocol::schema::*`.
- Register handlers with `agent_client_protocol::Agent.builder().on_receive_request(...)` and `on_receive_notification(...)`.
- Use `agent_client_protocol::Stdio` for the stdio transport.
- Route every handler through the actor; do not capture `Rc<RefCell<_>>` in builder closures.

Acceptance:

```bash
cargo update -p agent-client-protocol --precise 0.12.1
cargo check -p omegon --bin omegon
cargo test -p omegon acp --bin omegon
```

Manual smoke:

- Zed initializes ACP.
- New session returns modes/config options.
- Prompt streams assistant text.
- Slash command returns a response.
- Plan updates render in Zed native plan UI.

### Phase 4 — Port outbound client operations

Inside wrappers only:

- `send_session_update(...)` becomes `conn.send_notification(SessionNotification::new(...))`.
- Host requests become `conn.send_request(Request).block_task().await`.
- Preserve existing error text shape where practical.

Acceptance:

- Host `@file`/resource read path still works.
- Permission request flow still works.
- File write delegation remains permission-gated.

### Phase 5 — Port session config model/thinking/posture

- Remove dependency on direct `SetSessionModelRequest` if absent in 0.12.
- Use `SetSessionConfigOptionRequest` for model/thinking/posture.
- Add helper to extract string value from `SessionConfigOptionValue`.
- Add/adjust tests for model, thinking, and posture config changes.

Acceptance:

- Zed dropdown can switch model/thinking/posture.
- `/model` slash command still works as text fallback.
- Current unavailable model labels still render correctly.

### Phase 6 — Migrate WebSocket ACP

- Add a 0.12-compatible `ConnectTo` transport for the WebSocket channel/duplex bridge, or explicitly defer WebSocket ACP if not required for 0.23.2.
- Keep stdio ACP stable while implementing this.

Acceptance:

- `core/crates/omegon/src/web/acp_ws.rs` compiles on 0.12.
- Existing WebSocket endpoint tests pass.
- Manual connect/disconnect smoke where available.

## Open Questions

- [assumption] Zed's bundled ACP host supports agent-client-protocol 0.12.x / schema 0.13.x without requiring protocol downgrade behavior from Omegon.
- Which new ACP 0.12/schema 0.13 capabilities should Omegon adopt in 0.23.2, if any, beyond a compatibility upgrade? Candidates include unstable protocol v2, cancellation, elicitation, LLM provider/session model metadata, session usage, additional directories, and MCP-over-ACP.
- What compile/API breakages occur when changing `agent-client-protocol` from 0.10 to 0.12 with the current feature set, and are they isolated to `acp.rs`/ACP helpers?
- Should Omegon migrate its ACP server implementation to the 0.12 builder/handler API (`Agent.builder().on_receive_request/...`) in-place, or introduce a compatibility adapter module that preserves the current `OmegonAcpAgent` shape while using the new SDK under the hood?
- [recurring] Before each 0.23.x patch and at least monthly while ACP is moving, re-scan crates.io and upstream docs for `agent-client-protocol`, `agent-client-protocol-schema`, protocolVersion changes, new capabilities, and plan/status schema changes.

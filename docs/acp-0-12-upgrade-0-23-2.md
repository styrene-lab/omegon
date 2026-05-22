---
id: acp-0-12-upgrade-0-23-2
title: "ACP 0.12 upgrade for 0.23.2"
status: exploring
tags: [acp, zed, 0.23.2, dependency-upgrade]
open_questions:
  - "[assumption] Zed's bundled ACP host supports agent-client-protocol 0.12.x / schema 0.13.x without requiring protocol downgrade behavior from Omegon."
  - "Which new ACP 0.12/schema 0.13 capabilities should Omegon adopt in 0.23.2, if any, beyond a compatibility upgrade? Candidates include unstable protocol v2, cancellation, elicitation, LLM provider/session model metadata, session usage, additional directories, and MCP-over-ACP."
  - "What compile/API breakages occur when changing `agent-client-protocol` from 0.10 to 0.12 with the current feature set, and are they isolated to `acp.rs`/ACP helpers?"
  - "Should Omegon migrate its ACP server implementation to the 0.12 builder/handler API (`Agent.builder().on_receive_request/...`) in-place, or introduce a compatibility adapter module that preserves the current `OmegonAcpAgent` shape while using the new SDK under the hood?"
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

## Open Questions

- [assumption] Zed's bundled ACP host supports agent-client-protocol 0.12.x / schema 0.13.x without requiring protocol downgrade behavior from Omegon.
- Which new ACP 0.12/schema 0.13 capabilities should Omegon adopt in 0.23.2, if any, beyond a compatibility upgrade? Candidates include unstable protocol v2, cancellation, elicitation, LLM provider/session model metadata, session usage, additional directories, and MCP-over-ACP.
- What compile/API breakages occur when changing `agent-client-protocol` from 0.10 to 0.12 with the current feature set, and are they isolated to `acp.rs`/ACP helpers?
- Should Omegon migrate its ACP server implementation to the 0.12 builder/handler API (`Agent.builder().on_receive_request/...`) in-place, or introduce a compatibility adapter module that preserves the current `OmegonAcpAgent` shape while using the new SDK under the hood?
- [recurring] Before each 0.23.x patch and at least monthly while ACP is moving, re-scan crates.io and upstream docs for `agent-client-protocol`, `agent-client-protocol-schema`, protocolVersion changes, new capabilities, and plan/status schema changes.

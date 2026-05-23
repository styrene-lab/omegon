+++
id = "mcp-host-actions-metadata"
tags = ["mcp", "extensions", "host-actions", "issue-77"]
aliases = ["issue-77-host-actions-mcp"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# MCP HostActions metadata bridge — Issue 77

## Overview

Issue #77 maps Omegon HostActions through MCP metadata using the namespaced key:

```text
_meta["omegon/hostActions"]
```

This node scopes #77 as the bridge and validation boundary. Execution policy, manual approval UX, and MCP-server-specific HostAction permissions are explicitly future work under follow-up issue #78.

## Current implementation evidence

Commit `e9fb58c8 feat(mcp): preserve host actions in metadata` established the first slice:

- Native extension result `actions` are exposed by the MCP shim as `_meta["omegon/hostActions"]` while preserving normal MCP `content`.
- MCP tool results consumed by Omegon inspect `_meta["omegon/hostActions"]` and produce HostAction outcomes in tool result details.
- MCP-origin actions are tagged with `HostActionOrigin::mcp(server_name)` before policy evaluation.
- MCP-origin `auto_if_allowed` does not execute under default runtime policy.

Adversarial review found the slice is not complete enough to close #77 because MCP handling currently hardcodes a synthetic manifest allowlist in `plugins/mcp.rs` and reaches into HostAction internals directly.

## Issue 77 acceptance boundary

Issue #77 is complete when the following are true:

1. Native extension `actions` survive MCP exposure as `_meta["omegon/hostActions"]`.
2. Generic MCP clients can ignore `_meta` and still receive useful normal `content`.
3. Omegon recognizes MCP result `_meta["omegon/hostActions"]`.
4. MCP-origin candidates are tagged with `origin = mcp` before validation/policy.
5. MCP-origin actions never auto-execute by default.
6. Invalid or unsupported MCP HostActions become invalid/unsupported/denied outcomes without breaking ordinary MCP content.
7. Non-array or malformed HostAction metadata is visible as an invalid outcome and/or audit event.
8. MCP HostAction policy is deny-by-default unless a future explicit permission source is implemented.
9. The MCP plugin does not construct HostAction pipeline internals directly; metadata processing is centralized behind a narrow HostAction module API.

## Decisions

### Decision: Issue 77 owns bridge + validation, not manual execution UX

**Status:** decided
**Rationale:** #77 should prove the metadata contract and untrusted validation posture. Manual approval/execution semantics require additional policy configuration and UI/headless behavior that would broaden the issue beyond metadata mapping.

### Decision: MCP HostActions are deny-by-default for #77

**Status:** decided
**Rationale:** MCP servers are untrusted until explicit policy exists. A synthetic manifest that allows `terminal.create@1` for every MCP server is too permissive and creates future footguns if an executor is later wired into the MCP path.

### Decision: Centralize MCP HostAction metadata handling in `extensions::host_actions`

**Status:** decided
**Rationale:** `plugins/mcp.rs` should only extract MCP metadata and pass it to a narrow HostAction API. Origin scoping, metadata validation, runtime policy, audit, and outcome serialization belong beside the existing HostAction pipeline.

## Implementation plan for remaining #77 work

### 1. Replace MCP plugin internals with a narrow helper

Add to `core/crates/omegon/src/extensions/host_actions.rs`:

```rust
pub(crate) fn process_mcp_host_actions(
    actions: &serde_json::Value,
    server_name: &str,
    tool_name: &str,
) -> Vec<serde_json::Value>
```

The helper owns:

- array validation for `_meta["omegon/hostActions"]`;
- invalid metadata outcome construction;
- `HostActionOrigin::mcp(server_name)` attachment;
- scoped action IDs;
- MCP deny-by-default manifest/policy;
- serialization of outcomes;
- audit/debug events for invalid metadata.

### 2. Harden MCP policy

Remove the synthetic allowlist currently in `plugins/mcp.rs`:

```toml
[permissions.host_actions]
allowed = ["terminal.create@1"]
```

For #77, use an MCP default with no allowed action types unless an explicit future policy source is added.

### 3. Keep MCP plugin as metadata extraction only

`core/crates/omegon/src/plugins/mcp.rs` should only:

- convert MCP `content` to Omegon content blocks;
- check whether `result.meta` contains `omegon/hostActions`;
- call `extensions::host_actions::process_mcp_host_actions(...)`;
- attach returned outcomes to `ToolResult.details`.

### 4. Add focused tests

Required tests before closing #77:

- native extension shim keeps normal content and emits `_meta["omegon/hostActions"]`;
- shim omits `_meta` for empty `actions`;
- no MCP metadata returns `details = Null`;
- non-array MCP metadata returns invalid outcome;
- malformed MCP action returns invalid outcome;
- unsupported MCP action type returns unsupported outcome;
- supported action type under deny-by-default MCP policy returns denied outcome;
- MCP `auto_if_allowed` is denied and never executed;
- bad metadata does not prevent ordinary content conversion.

## Future work boundary

The following are out of scope for #77 and belong to follow-up issue #78, **MCP HostAction permission and manual approval policy**:

- MCP server config schema for HostAction permissions.
- Project/session policy allowing selected MCP HostAction types.
- Downgrading MCP `auto_if_allowed` to manual/request action cards.
- TUI/ACP rendering for MCP-origin action approval cards.
- Headless/daemon behavior for MCP-origin HostAction requests.
- Audit tests for approved execution decisions.
- Real execution of MCP-origin HostActions after operator approval.

## Open Questions

*No open questions for #77. Future policy and UI questions move to the follow-up issue.*

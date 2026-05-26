---
title: ACP HostAction Approval Routing
status: decided
tags: [extensions, host-actions, acp, flynt, mcp]
date: 2026-05-25
---

# ACP HostAction Approval Routing

## Problem

Omegon 0.24 processes native extension `ToolResult.actions` inside core before ACP clients can review the original request:

```text
ToolResult.actions
→ parse extension envelope
→ process_declarative_host_actions(...)
→ host_actions cleared
→ host_action_outcomes returned in ToolResult.details
→ ACP ToolEnd.raw_output contains outcomes only
```

This means Flynt can render prose or post-processed outcomes, but it cannot own review of the original HostAction candidate.

Issue #78 closure requires permitted manual/request MCP and native-extension HostActions to enter the same approval path before execution.

## Decision

Omegon remains the canonical HostAction policy/execution owner, but ACP clients get decision authority for manual HostActions when connected.

The 0.24.3 target is:

```text
HostAction candidate
→ validate schema/type/manifest/runtime policy
→ if manual review required and ACP client is attached
→ send ACP session/request_permission with original HostAction in _meta
→ client selects allow-once or reject-once
→ allow executes through Omegon HostActionExecutorRegistry
→ reject returns denied outcome
→ no ACP client deterministically denies manual HostAction
```

Flynt does not become the terminal backend in this patch. Backend delegation is a separate future contract.


## Adversarial assessment

### Finding: ACP `request_permission` can approve only if the executing runtime can wait

The initial design says the worker sends a permission request to the ACP I/O task and waits for a response. That is correct architecturally, but dangerous if implemented by blocking inside a synchronous `Feature::execute` helper or by holding extension process locks while waiting. The implementation must keep approval async and must not hold the extension transport mutex across operator review.

Mitigation: first slice only builds request/decision helpers and approval-aware HostAction processing seams. The actual ACP bridge must use `tokio::sync::oneshot` and async wait points outside transport locks.

### Finding: Native extension `auto_if_allowed` currently maps to `Denied`, not `NeedsApproval`

The existing `process_host_action_candidate` returns `Denied(auto_not_allowed)` before execution. That is safe, but it makes manual approval hard to distinguish from permanent denial.

Mitigation: introduce a pre-execution classification for valid supported manifest-allowed candidates. `auto_if_allowed` without full auto policy becomes an approval request only when an approval channel exists; otherwise it remains a deterministic denial.

### Finding: ACP permission approval is not equivalent to execution authorization

ACP client approval must not bypass manifest/type/runtime validation. A malicious or buggy client could approve an action the manifest did not allow if the permission response alone became the authority.

Mitigation: approval decisions feed `RuntimeHostActionPolicy.operator_approved = true`; action still goes through `process_host_action_candidate` and executor registry.

### Finding: Flynt may need candidate data even when execution remains in Omegon

If ACP permission `_meta` omits the original HostAction request, Flynt cannot render an accurate review card. If only a prose title is sent, this repeats the current bug in another form.

Mitigation: `_meta["omegon/hostActionApproval"].action` contains the exact original HostAction candidate plus trusted origin metadata.

### Finding: MCP and native origins need different trust labels

A single generic “host action” label hides the trust boundary. MCP tools are server-originated and remain denied by default; native extensions are manifest-originated.

Mitigation: metadata carries `origin`, `extension`, `server`, `tool`, and `tool_call_id`. Tests must assert those fields.

### Finding: allow-always is out of scope

ACP supports allow-always/reject-always option kinds. Persisting those choices would create a new policy store and audit obligation.

Mitigation: 0.24.3 exposes only `allow-once` and `reject-once`; durable policy can be added later.

### Revised implementation order

1. Build pure approval request/decision helpers with tests.
2. Add approval-aware HostAction processing seam with no ACP transport yet: approved/rejected/unavailable decisions are injectable in tests.
3. Wire ACP bridge once the seam is proven.
4. Converge MCP on the same seam.

## ACP permission payload

Use ACP's existing `session/request_permission` request. Put the HostAction payload in ACP `_meta`:

```json
{
  "omegon/hostActionApproval": {
    "kind": "host_action",
    "origin": "native_extension",
    "extension": "omegon-reader",
    "server": null,
    "tool": "reader_open",
    "tool_call_id": "call_123",
    "action": {
      "id": "open-reader",
      "type": "terminal.create@1",
      "execution": "manual",
      "params": {}
    },
    "policy": {
      "execution": "manual",
      "reason": "host action requires approval"
    }
  }
}
```

Options:

```text
allow-once  PermissionOptionKind::AllowOnce
reject-once PermissionOptionKind::RejectOnce
```

## Native extension behavior

For native extension declarative actions:

1. Parse extension envelope and preserve `host_actions` candidates.
2. Validate candidate using existing HostAction policy machinery.
3. If candidate is denied/invalid/unsupported, return outcome immediately.
4. If candidate requires manual approval, request ACP permission when available.
5. Execute approved actions through the existing executor registry.
6. Return a `HostActionOutcome` in `ToolResult.details.host_action_outcomes`.

## MCP behavior

MCP-origin HostActions remain deny-by-default unless explicit server policy permits them:

```toml
[host_actions]
allowed = ["terminal.create@1"]
tools = ["open"]
manual = true
```

When permitted, MCP actions become approval requests, not auto-executions. `auto_if_allowed` is downgraded to manual unless a later policy explicitly supports MCP auto execution.

## Fallback behavior

If no ACP client/permission bridge is available:

```json
{
  "status": "denied",
  "error": {
    "code": "approval_unavailable",
    "message": "HostAction requires ACP approval, but no approval channel is available"
  }
}
```

No silent execution.

## Audit requirements

Every HostAction approval decision must be auditable with:

- origin kind (`native_extension` or `mcp`)
- extension/server identity
- tool name
- tool call id
- action id
- action type
- decision (`approved`, `denied`, `approval_unavailable`)
- executor outcome status

## Implementation slices

### Slice 1 — approval request model

Add host-side structs/functions for building ACP `RequestPermissionRequest` metadata and interpreting `RequestPermissionResponse`.

Tests:

- metadata includes original HostAction candidate
- allow-once maps to approved
- reject-once maps to denied
- cancelled maps to denied

### Slice 2 — worker bridge

Add worker event/request channel from agent worker to ACP I/O task:

```rust
WorkerEvent::RequestHostActionApproval { request, response_tx }
```

ACP task calls `conn.request_permission(request)` and sends the result back.

Tests:

- ACP bridge forwards request and returns allow
- no connection returns approval unavailable

### Slice 3 — native declarative actions

Replace immediate declarative HostAction execution with approval-aware processing for manual candidates.

Tests:

- native reader action emits ACP permission request before execution
- allow executes canonical terminal backend
- reject does not execute
- no ACP denies deterministically

### Slice 4 — MCP convergence

Route MCP `needs_approval` outcomes into the same approval bridge.

Tests:

- policy-allowed MCP action requests ACP approval
- unconfigured MCP remains denied by default
- approved MCP action executes canonical executor

## Non-goals

- Flynt terminal backend registration.
- MCP auto-execution.
- Long-lived remember/allow-always persistence.
- Arbitrary host action families beyond current executor support.

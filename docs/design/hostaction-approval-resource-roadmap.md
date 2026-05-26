---
title: HostAction roadmap — approval path then resource.open
tags: [host-actions, acp, flynt, reader, roadmap]
date: 2026-05-25
---

# HostAction roadmap — approval path then resource.open

## Summary

Issue #78 and #83 are related but must land in separate patches.

```text
0.24.3 / #78: generic HostAction approval path
next patch / #83: resource.open@1 semantic action family and routing
```

#78 unblocks Flynt review of HostAction candidates. #83 uses that path to add a new high-level resource-opening intent.

## 0.24.3 — #78 closure

Goal: Flynt/ACP can review permitted manual HostAction candidates before Omegon executes them.

### Scope

- Generic approval path for HostAction candidates.
- Native extension candidates preserve original `ToolResult.actions` long enough for review.
- MCP candidates with explicit policy use the same manual approval path.
- ACP `session/request_permission` carries `_meta["omegon/hostActionApproval"]`.
- Approval metadata is action-family generic: `action.type` and `action.params` are preserved as emitted.
- Approved actions execute through Omegon's canonical HostAction executor registry.
- Rejected/cancelled/unavailable approvals produce deterministic denied outcomes.
- MCP `auto_if_allowed` remains downgraded to manual approval; no MCP auto-execution.

### Non-goals

- Flynt as terminal backend/executor.
- `resource.open@1` schema or routing.
- Durable allow-always/reject-always policy persistence.
- New desktop/app routing policy.

### Acceptance

- Native extension `terminal.create@1` candidate causes ACP permission request before execution.
- ACP `_meta` includes original HostAction candidate and trusted origin context.
- Allow executes canonical executor and returns completed outcome.
- Reject/cancel/no-client returns denied outcome and executor is not called.
- MCP policy-allowed candidate uses same approval request path.
- Unconfigured MCP candidate remains denied by default.

## Next patch — #83 closure

Goal: extensions can emit semantic “open this resource” requests without hard-coding Flynt/Zed/Bookokrat routing.

### Scope

- SDK action schema: `resource.open@1`.
- Typed params/result:
  - `ResourceOpenParams`
  - `ResourceOpenResult`
- Host-side validation:
  - file URI/root containment
  - URI scheme allowlist
  - view/edit/read/inspect intent policy
  - external/system opener gating
- Resource backend registry:
  - Flynt backend
  - Zed backend
  - Bookokrat/terminal fallback backend
  - built-in/system/fake test backends
- Routing defaults:
  - markdown/Flynt-native docs → Flynt
  - code/text/config → Zed
  - ebooks/terminal-reader formats → Bookokrat via `terminal.create@1`
- Explicit degradation warnings.
- Deterministic headless behavior.

### Non-goals

- Replacing `terminal.create@1`.
- Making `omegon-reader` depend on Flynt or Zed.
- Full desktop file association system.
- Arbitrary external URI schemes without policy.

## Dependency direction

#83 depends on #78 because `resource.open@1` must be reviewable before execution:

```text
resource.open@1
→ HostAction approval path (#78)
→ host resource router/backend registry (#83)
```

Do not implement #83 routing inside #78. The approval transport must remain generic so #83 can plug into it without changing the ACP permission protocol.

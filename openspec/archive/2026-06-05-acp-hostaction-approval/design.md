# Design — ACP HostAction Approval Routing

See [[docs/design/acp-hostaction-approval-routing]].

## Decision

Use ACP `session/request_permission` for manual HostAction review. The HostAction request is carried in `_meta["omegon/hostActionApproval"]`. Omegon remains the executor; ACP/Flynt gets decision authority for allow/reject.

## Constraints

- Never auto-execute MCP-origin HostActions in this slice.
- Preserve deny-by-default behavior when no explicit MCP policy exists.
- No ACP client means deterministic denial, not fallback execution.
- Do not make Flynt a terminal backend in this change.

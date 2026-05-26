# Tasks — ACP HostAction Approval Routing

## 1. Approval request model
<!-- specs: extensions/hostaction-approval -->

- [x] 1.1 Add host-side ACP HostAction approval request metadata builder.
- [x] 1.2 Add tests for original HostAction payload preservation in `_meta`.
- [x] 1.3 Add tests mapping ACP allow/reject/cancel outcomes to HostAction decisions.

## 2. ACP approval transport
<!-- specs: extensions/hostaction-approval -->

- [x] 2.1 Add host proxy request/response channel for HostAction approval.
- [x] 2.2 ACP pump calls `session/request_permission` and returns the decision.
- [x] 2.3 Add no-client fallback test proving deterministic denial.

## 3. Native extension declarative HostActions
<!-- specs: extensions/hostaction-approval -->

- [x] 3.1 Preserve native HostAction candidates until approval decision.
- [x] 3.2 Approved native action executes through canonical executor.
- [x] 3.3 Rejected native action does not execute.
- [x] 3.4 Existing outcome rendering remains available in ToolResult details.
- [x] 3.5 Route native extension tool execution through `ToolExecutionContext` approval sinks.

## 4. MCP HostAction convergence
<!-- specs: extensions/hostaction-approval -->

- [x] 4.1 Route policy-allowed MCP actions through the ACP approval bridge.
- [x] 4.2 Preserve unconfigured MCP deny-by-default behavior.
- [x] 4.3 Ensure `auto_if_allowed` from MCP downgrades to manual approval.

## 5. Loop integration
<!-- specs: extensions/hostaction-approval -->

- [x] 5.1 Build `ToolExecutionContext` from ACP `HostContext` during tool dispatch.
- [x] 5.2 Route tool calls through `EventBus::execute_tool_with_context`.
- [x] 5.3 Ensure malformed approval request serialization falls back to `approval_unavailable`.

## 6. Validation
<!-- specs: extensions/hostaction-approval -->

- [x] 6.1 Run `cargo test -p omegon-extension host_action -- --nocapture`.
- [x] 6.2 Run `cargo test -p omegon mcp_host_action -- --nocapture`.
- [x] 6.3 Run `cargo test -p omegon acp_host_action -- --nocapture`.
- [x] 6.4 Run `cargo test -p omegon extensions::tests:: -- --nocapture`.
- [x] 6.5 Run `just lint`.

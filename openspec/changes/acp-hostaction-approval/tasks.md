# Tasks — ACP HostAction Approval Routing

## 1. Approval request model
<!-- specs: extensions/hostaction-approval -->

- [x] 1.1 Add host-side ACP HostAction approval request metadata builder.
- [x] 1.2 Add tests for original HostAction payload preservation in `_meta`.
- [x] 1.3 Add tests mapping ACP allow/reject/cancel outcomes to HostAction decisions.

## 2. ACP approval transport
<!-- specs: extensions/hostaction-approval -->

- [ ] 2.1 Add host proxy request/response channel for HostAction approval.
- [ ] 2.2 ACP pump calls `session/request_permission` and returns the decision.
- [x] 2.3 Add no-client fallback test proving deterministic denial.

## 3. Native extension declarative HostActions
<!-- specs: extensions/hostaction-approval -->

- [x] 3.1 Preserve native HostAction candidates until approval decision.
- [x] 3.2 Approved native action executes through canonical executor.
- [x] 3.3 Rejected native action does not execute.
- [x] 3.4 Existing outcome rendering remains available in ToolResult details.

## 4. MCP HostAction convergence
<!-- specs: extensions/hostaction-approval -->

- [ ] 4.1 Route policy-allowed MCP actions through the ACP approval bridge.
- [ ] 4.2 Preserve unconfigured MCP deny-by-default behavior.
- [ ] 4.3 Ensure `auto_if_allowed` from MCP downgrades to manual approval.

## 5. Validation
<!-- specs: extensions/hostaction-approval -->

- [ ] 5.1 Run `cargo test -p omegon-extension host_action -- --nocapture`.
- [ ] 5.2 Run `cargo test -p omegon mcp_host_action -- --nocapture`.
- [ ] 5.3 Run `cargo test -p omegon acp_host_action -- --nocapture`.
- [ ] 5.4 Run `cargo test -p omegon extensions::tests:: -- --nocapture`.
- [ ] 5.5 Run `just lint`.

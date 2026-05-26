# extensions/hostaction-approval — Delta Spec

## ADDED Requirements

### Requirement: ACP client can review HostAction candidates before execution

Manual HostAction candidates from native extensions and explicitly permitted MCP servers MUST be routed to an ACP permission request before execution when an ACP client is connected.

#### Scenario: Native HostAction approval request includes original action
Given a native extension returns a declarative `terminal.create@1` HostAction
And an ACP client is connected
When Omegon processes the tool result
Then Omegon sends `session/request_permission` to the ACP client
And `_meta["omegon/hostActionApproval"].action` contains the original HostAction candidate
And `_meta["omegon/hostActionApproval"].origin` is `native_extension`

#### Scenario: Approved HostAction executes through canonical executor
Given an ACP client approves a manual HostAction permission request with `allow-once`
When Omegon receives the permission response
Then Omegon executes the action through the HostAction executor registry
And the tool result contains a completed HostAction outcome

#### Scenario: Rejected HostAction does not execute
Given an ACP client rejects a manual HostAction permission request with `reject-once`
When Omegon receives the permission response
Then Omegon does not call the HostAction executor
And the tool result contains a denied HostAction outcome

#### Scenario: Missing ACP approval channel denies deterministically
Given a manual HostAction candidate requires ACP approval
And no ACP permission channel is available
When Omegon processes the HostAction candidate
Then Omegon does not execute the action
And the tool result contains a denied HostAction outcome with code `approval_unavailable`

### Requirement: MCP HostActions use the same approval route after explicit policy

MCP-origin HostActions MUST remain denied by default. If an explicit MCP policy permits an action type and tool, the action MUST be downgraded to manual ACP approval rather than auto-executed.

#### Scenario: Unconfigured MCP HostAction remains denied
Given an MCP tool returns `_meta["omegon/hostActions"]`
And the MCP server has no HostAction policy
When Omegon processes the MCP tool result
Then the HostAction outcome status is `denied`
And no ACP approval request is sent

#### Scenario: Policy-allowed MCP HostAction requests approval
Given an MCP server policy allows `terminal.create@1` for tool `open`
And the MCP tool `open` returns a `terminal.create@1` HostAction
When Omegon processes the MCP tool result
Then Omegon sends an ACP permission request before execution
And `_meta["omegon/hostActionApproval"].origin` is `mcp`
And `_meta["omegon/hostActionApproval"].server` identifies the MCP server

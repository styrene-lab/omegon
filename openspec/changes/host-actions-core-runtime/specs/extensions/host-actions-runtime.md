# Extensions HostActions Runtime — Delta Spec

## ADDED Requirements

### Requirement: Manifest HostAction permissions

Omegon SHALL parse HostAction capability and permission declarations from extension manifests.

#### Scenario: Manifest enables HostAction capabilities
Given an extension manifest contains `[capabilities] host_actions = true` and `host_action_execution = true`
When Omegon loads the manifest
Then the manifest model exposes both capability flags as enabled
And existing manifests without those fields still load with both flags disabled.

#### Scenario: Manifest restricts allowed action types
Given an extension manifest contains `[permissions.host_actions] allowed = ["terminal.create@1"]`
When a HostAction candidate has type `terminal.create@1`
Then manifest policy considers the action type allowed
And when a HostAction candidate has type `file.open@1`
Then manifest policy denies the action type.

#### Scenario: Terminal permission policy parses command constraints
Given an extension manifest contains `[permissions.host_actions.terminal_create]` with `allowed_commands`, `allowed_cwd_roots`, `allow_env`, and `interactive`
When Omegon loads the manifest
Then those fields are available to the HostAction policy pipeline.

### Requirement: Structured extension tool-result envelope extraction

Omegon SHALL parse extension tool results into ordinary content, structured data, metadata, and HostAction candidates without losing backward compatibility.

#### Scenario: Legacy raw JSON result still renders
Given an extension returns arbitrary JSON without a `content` or `actions` envelope
When Omegon handles the tool result
Then the user-visible content remains the JSON string representation
And no HostAction candidates are extracted.

#### Scenario: Content array remains ordinary content
Given an extension returns `{ "content": [{ "type": "text", "text": "hello" }] }`
When Omegon handles the tool result
Then the user-visible content contains `hello`
And no HostAction candidates are extracted.

#### Scenario: Valid actions are extracted separately
Given an extension returns a tool result with ordinary content and an `actions` array containing a valid HostAction candidate
When Omegon handles the tool result
Then ordinary content is preserved
And the HostAction candidate is attached separately for validation/policy/rendering.

#### Scenario: Malformed actions do not poison content
Given an extension returns ordinary content and an `actions` array containing malformed entries
When Omegon handles the tool result
Then ordinary content is preserved
And each malformed action produces a typed `invalid` or ignored outcome separate from content.

### Requirement: Canonical HostAction pipeline

Omegon SHALL route all HostAction candidates through one validation, policy, execution, and audit pipeline.

#### Scenario: Unsupported action type returns unsupported outcome
Given a HostAction candidate has a syntactically valid but unregistered type `unknown.action@1`
When the HostAction pipeline processes it
Then the outcome status is `unsupported`
And no executor is invoked.

#### Scenario: Malformed action returns invalid outcome
Given a HostAction candidate is missing required fields or has malformed params
When the HostAction pipeline processes it
Then the outcome status is `invalid`
And ordinary tool content remains renderable.

#### Scenario: Manifest-denied action returns denied outcome
Given an extension manifest only allows `terminal.create@1`
When the extension requests `file.open@1`
Then the outcome status is `denied`
And the decision is auditable.

#### Scenario: Imperative execution uses same policy as declarative action
Given a HostAction would be denied when returned declaratively
When the same extension sends it through `actions/execute`
Then the outcome status is `denied`
And no executor is invoked.

#### Scenario: Conservative auto execution
Given a HostAction requests `auto_if_allowed`
When manifest permissions, project policy, runtime mode, and origin trust do not all permit automatic execution
Then Omegon does not auto-execute the action
And returns or records a non-completed outcome requiring operator approval.

### Requirement: Action identity and origin

Omegon SHALL treat extension and MCP action identifiers as local labels and attach trusted host-side origin identity before policy decisions.

#### Scenario: Action ids are scoped by origin
Given two different extensions both return an action with id `open-reader`
When Omegon records the actions
Then their runtime identities are distinct by origin, extension identity, session, tool call, and local action id.

#### Scenario: MCP actions do not auto-execute by default
Given a HostAction candidate originates from MCP metadata
When the action requests `auto_if_allowed`
Then default policy does not auto-execute it unless project policy explicitly permits that MCP origin.

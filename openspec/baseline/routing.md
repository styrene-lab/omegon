+++
id = "c0e7546a-b16c-481a-b9ec-f67a41c3743b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# routing

### Requirement: Operator-facing model controls describe capability grades and endpoint selection

Provider-neutral model controls SHALL explain `/model grade <F|D|C|B|A|S>`, `/model provider <auto|local|upstream|endpoint-id>`, and `set_model_intent` in terms of capability grades, endpoint selection, and multi-provider routing. Legacy commands `/local`, `/haiku`, `/sonnet`, `/opus` and the `set_model_tier` tool SHALL NOT be required behavior.

#### Scenario: Slash-command help uses grade wording
Given the operator has Omegon loaded with multiple cloud providers available
When help text or command descriptions for model controls are shown
Then the descriptions refer to capability grades such as F, D, C, B, A, or S
And local is described as a provider/endpoint selector rather than a capability grade
And the descriptions do not require Anthropic product names to make sense

#### Scenario: Tool help reflects model intent routing
Given the operator inspects the `set_model_intent` tool
When the tool description is rendered
Then it explains that Omegon resolves the requested grade through the active routing policy
And it does not imply that F/D/C/B/A/S grades always map to one provider family

### Requirement: Sessions restore the last explicitly selected driver model

Omegon SHALL persist the last successfully selected driver model and restore it on the next session start before falling back to effort defaults.

#### Scenario: Session start restores last selected model
Given the operator previously switched to a concrete driver model successfully
And that model is still available in the registry on the next startup
When `session_start` runs
Then Omegon restores that concrete model automatically
And the operator is not forced back to the effort tier default for that session

#### Scenario: Missing persisted model falls back safely
Given the persisted last-used model is no longer available
When `session_start` runs
Then Omegon falls back to the configured effort default model resolution
And startup continues without error

#### Scenario: Only successful explicit switches are persisted
Given a model switch attempt fails because no matching model is available
When Omegon finishes handling the failed switch
Then the persisted last-used model remains unchanged
And a previously working persisted model is not overwritten by the failure

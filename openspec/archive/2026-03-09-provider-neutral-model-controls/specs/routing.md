+++
id = "7c80729d-a0a3-4274-93f9-5a68f2a4c9a0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# routing — Delta Spec

## ADDED Requirements

### Requirement: Operator-facing model controls describe capability tiers, not Anthropic-only products

Provider-neutral model controls SHALL explain `/local`, `/haiku`, `/sonnet`, `/opus`, and `set_model_tier` in terms of capability tiers and multi-provider routing rather than implying Anthropic-exclusive backing.

#### Scenario: Slash-command help uses provider-neutral wording
Given the operator has pi-kit loaded with multiple cloud providers available
When help text or command descriptions for `/haiku`, `/sonnet`, or `/opus` are shown
Then the descriptions refer to capability tiers such as fast, balanced, or deep reasoning
And the descriptions do not require Anthropic product names to make sense
And the descriptions remain compatible with the canonical internal tier keys

#### Scenario: Tool help reflects provider-aware routing
Given the operator inspects the `set_model_tier` tool
When the tool description is rendered
Then it explains that pi-kit resolves the requested tier through the active routing policy
And it does not imply that `haiku`, `sonnet`, or `opus` always map to Anthropic models

### Requirement: Sessions restore the last explicitly selected driver model

pi-kit SHALL persist the last successfully selected driver model and restore it on the next session start before falling back to effort defaults.

#### Scenario: Session start restores last selected model
Given the operator previously switched to a concrete driver model successfully
And that model is still available in the registry on the next startup
When `session_start` runs
Then pi-kit restores that concrete model automatically
And the operator is not forced back to the effort tier default for that session

#### Scenario: Missing persisted model falls back safely
Given the persisted last-used model is no longer available
When `session_start` runs
Then pi-kit falls back to the configured effort default model resolution
And startup continues without error

#### Scenario: Only successful explicit switches are persisted
Given a model switch attempt fails because no matching model is available
When pi-kit finishes handling the failed switch
Then the persisted last-used model remains unchanged
And a previously working persisted model is not overwritten by the failure

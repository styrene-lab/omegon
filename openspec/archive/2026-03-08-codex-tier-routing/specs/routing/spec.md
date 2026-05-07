+++
id = "34a946f9-113d-465a-8c62-ac71f975e1bc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# routing — Delta Spec

## ADDED Requirements

### Requirement: Provider-aware tier resolution uses abstract capability tiers

pi-kit SHALL keep planning-time model tiers abstract while resolving execution to concrete provider models at runtime.

#### Scenario: Abstract tier resolves through provider preference
Given the session routing policy prefers providers in the order "openai", "anthropic", "local"
And the model registry contains an OpenAI model that satisfies the requested "sonnet" tier
When the resolver is asked for the "sonnet" tier
Then it returns that OpenAI model
And the returned result includes the concrete model ID
And the returned result includes the selected provider name

#### Scenario: Resolver skips avoided providers
Given the session routing policy avoids provider "anthropic"
And both Anthropic and OpenAI models satisfy the requested "opus" tier
When the resolver is asked for the "opus" tier
Then it does not choose the Anthropic model
And it chooses the highest-priority non-avoided provider with a matching model

#### Scenario: Resolver falls back across providers
Given the session routing policy prefers providers in the order "openai", "anthropic", "local"
And no OpenAI model satisfies the requested "haiku" tier
And an Anthropic model satisfies the requested "haiku" tier
When the resolver is asked for the "haiku" tier
Then it returns the Anthropic model

#### Scenario: Local tier still resolves locally
Given the session routing policy prefers cheap cloud over local
And a local model is available
When the resolver is asked for the "local" tier
Then it returns the local model
And it does not substitute a cloud model

### Requirement: Cleave dispatch uses explicit model IDs

Cleave SHALL resolve child execution and review tiers to explicit model IDs before spawning child processes.

#### Scenario: Child execution passes resolved model ID
Given a child plan has executeModel "opus"
And the resolver maps that tier to model ID "gpt-5.4"
When Cleave dispatches the child
Then the spawned pi process receives "--model gpt-5.4"
And Cleave does not pass the bare alias "opus"

#### Scenario: Default sonnet execution still becomes explicit
Given a child plan has no explicit model override
And model resolution chooses a concrete model ID for the default "sonnet" tier
When Cleave dispatches the child
Then the spawned pi process receives that concrete model ID explicitly

#### Scenario: Review model also resolves explicitly
Given Cleave review is enabled
And the active review tier resolves to model ID "claude-opus-4-6"
When Cleave launches the reviewer
Then the spawned review process receives "--model claude-opus-4-6"

### Requirement: Session routing policy is operator-driven and lightweight

pi-kit SHALL store a session-scoped routing policy that captures operator budget posture without requiring exact quota accounting.

#### Scenario: Session policy stores provider order and flags
Given the operator sets a routing policy preferring "openai" then "anthropic"
And the operator marks cheap cloud as preferred over local
And the operator enables preflight for large runs
When the policy is stored in shared state
Then shared state contains the provider order
And shared state contains the cheap-cloud-over-local flag
And shared state contains the large-run preflight flag

#### Scenario: Session policy can avoid a provider temporarily
Given the operator indicates Claude budget is low for the current session
When the routing policy is updated
Then shared state records "anthropic" in an avoid-provider list
And future model resolution skips Anthropic unless no acceptable alternative exists

### Requirement: Large Cleave runs request budget posture before dispatch

When a Cleave run is likely to consume significant cloud budget, pi-kit SHALL ask the operator for current provider posture before dispatching children.

#### Scenario: Large run triggers preflight prompt
Given Cleave is about to dispatch a run that exceeds the large-run threshold
And session policy requires preflight for large runs
When dispatch preparation begins
Then the operator is asked which provider should be favored for this run
And dispatch waits for that operator choice before selecting child models

#### Scenario: Small run does not interrupt with preflight
Given Cleave is about to dispatch a run that does not exceed the large-run threshold
When dispatch preparation begins
Then no budget preflight prompt is shown
And routing uses the existing session policy

### Requirement: Operator-facing labels use Servitor/Adept/Magos/Archmagos names

pi-kit SHALL present provider-neutral tier labels in operator-facing UX while preserving internal compatibility in phase 1.

#### Scenario: Model-budget status uses thematic tier labels
Given the active model tier is "haiku"
When pi-kit displays the current tier to the operator
Then the display label is "Adept"
And the display does not require Anthropic product names

#### Scenario: Deep tier uses Archmagos label
Given the active model tier is "opus"
When pi-kit displays the current tier to the operator
Then the display label is "Archmagos"

#### Scenario: Internal compatibility remains intact
Given internal routing logic still stores canonical keys
When a child plan is serialized
Then executeModel remains one of "local", "haiku", "sonnet", or "opus"
And operator-facing display labels are derived separately

### Requirement: Cheap cloud is preferred over local for cloud-eligible leaf work

Background routing SHALL prefer inexpensive cloud models over local inference when policy allows and a cloud match is available.

#### Scenario: Extraction prefers cheap cloud when configured
Given the session routing policy prefers cheap cloud over local
And an OpenAI model satisfies the configured extraction tier
When pi-kit selects a model for extraction work
Then it chooses the OpenAI model instead of a local model

#### Scenario: Offline or unavailable cloud falls back safely
Given the session routing policy prefers cheap cloud over local
And no configured cloud provider has a matching available model
And a local model is available
When pi-kit selects a model for extraction work
Then it falls back to the local model

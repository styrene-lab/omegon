# telemetry/provider-telemetry — Delta Spec

## ADDED Requirements

### Requirement: Capture provider telemetry snapshots from upstream responses

Omegon must capture provider quota or rate-limit telemetry whenever an upstream provider exposes it on inference responses.

#### Scenario: Anthropic unified utilization headers are captured
Given an Anthropic response includes unified utilization headers
When Omegon processes the response
Then it records a provider telemetry snapshot containing the current provider id
And the snapshot preserves the raw header-derived values for five-hour and seven-day utilization

#### Scenario: OpenAI-style rate limit headers are captured
Given an OpenAI-compatible response includes x-ratelimit headers
When Omegon processes the response
Then it records a provider telemetry snapshot containing request/token headroom and reset information when available

### Requirement: Footer/HUD shows current provider telemetry honestly

The operator-facing telemetry display must show the best available current-provider quota/headroom information without pretending providers share identical semantics.

#### Scenario: Anthropic footer shows utilization windows
Given the active provider is Anthropic and unified utilization snapshots exist
When the footer or HUD renders session telemetry
Then it shows the current Anthropic-specific utilization windows in compact form
And it preserves provider-specific semantics instead of relabeling them as generic request limits

#### Scenario: OpenAI-compatible footer shows headroom
Given the active provider exposes request/token rate-limit headers but not subscription utilization windows
When the footer or HUD renders session telemetry
Then it shows the available request/token headroom in compact form
And it does not invent unsupported subscription usage percentages

### Requirement: Session telemetry persists provider snapshots for audit

Per-turn session telemetry must preserve provider quota snapshots alongside tokens so mixed-provider sessions can be audited later.

#### Scenario: Turn end persists provider telemetry snapshot
Given a turn completes with provider telemetry available
When Omegon emits end-of-turn telemetry
Then the turn event includes the latest provider telemetry snapshot
And downstream consumers can correlate provider, model, token counts, and provider telemetry for that turn

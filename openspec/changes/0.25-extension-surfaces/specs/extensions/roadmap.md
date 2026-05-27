# 0.25 Extension Surfaces — Delta Spec

## ADDED Requirements

### Requirement: Roadmap identifies ordered feature lanes

The 0.25 planning artifacts SHALL identify ordered implementation lanes for extension UI contributions, visible terminal fallback sessions, semantic resource opens, ACP terminal delegation, voice UX, and SDK contract stabilization.

#### Scenario: Core 0.25 features are discoverable
Given the roadmap exists
When an operator reviews 0.25 planning
Then #101, #104, and #83 are listed as core 0.25.0 candidates
And dependencies between them are explicit

#### Scenario: Deferred SDK extraction is marked blocked
Given SDK extraction is listed
When an operator reviews its design node
Then it identifies the SDK lockstep contract as a prerequisite
And it does not recommend big-bang extraction

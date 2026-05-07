+++
id = "5eedb226-ebd5-4324-9eb0-fffff9f315a0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# dashboard/publishers — Delta Spec

## ADDED Requirements

### Requirement: OpenSpec dashboard publication uses shared refresh helpers

OpenSpec MUST route dashboard-facing state refresh through a shared helper rather than scattering repeated direct publisher calls across many command and tool paths.

#### Scenario: OpenSpec mutation path refreshes dashboard via shared helper
Given an OpenSpec command mutates change state or lifecycle state
When it triggers dashboard-facing refresh
Then it invokes a shared refresh helper for OpenSpec publication
And the caller does not need to manually duplicate dashboard refresh boilerplate inline

#### Scenario: shared helper preserves existing dashboard semantics
Given a status-affecting OpenSpec mutation completes
When the shared refresh helper runs
Then dashboard-facing OpenSpec state remains updated for the current repository
And lifecycle-sensitive consumers still observe the same refreshed truth they did before consolidation

### Requirement: Design-tree dashboard publication uses shared refresh helpers

Design-tree MUST route dashboard-facing state refresh through a shared helper so mutation paths do not each re-issue the same publisher boilerplate independently.

#### Scenario: design-tree mutation path refreshes dashboard via shared helper
Given a design-tree command changes node status, binding, focus, or implementation notes
When dashboard-facing state is refreshed
Then the command uses a shared design-tree refresh helper
And repeated direct publisher calls across mutation sites are reduced

#### Scenario: focus-aware design-tree refresh still works after consolidation
Given a design-tree node is focused or focus changes during a mutation
When the shared refresh helper publishes state
Then the resulting dashboard-facing state still reflects the correct focused node summary
And consolidation does not break focus-sensitive dashboard behavior

### Requirement: Shared publisher helpers remain incremental and extension-local

Publisher consolidation MUST reduce repeated refresh boilerplate without requiring a full rewrite of extension domain logic or unrelated dashboard rendering code.

#### Scenario: consolidation changes are bounded to publisher seams
Given the consolidation slice is implemented
When a reviewer inspects the change set
Then the new shared helper logic is concentrated around publication/refresh seams
And the change does not require unrelated rewrites of core domain logic to achieve the consolidation

#### Scenario: consolidated publisher paths remain regression-tested
Given publisher refresh helpers are consolidated
When the relevant OpenSpec and design-tree test suites run
Then they verify that dashboard-facing state still refreshes after lifecycle and design mutations
And they exercise the consolidated helper paths rather than only legacy direct-call flows

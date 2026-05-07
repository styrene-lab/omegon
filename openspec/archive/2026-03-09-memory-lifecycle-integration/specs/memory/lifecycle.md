+++
id = "52140358-2975-4d65-a047-0e291767cd1e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# memory/lifecycle — Delta Spec

## ADDED Requirements

### Requirement: Structured lifecycle conclusions create memory candidates

Project memory SHALL generate candidate facts from explicit lifecycle artifacts instead of relying on free-form workflow chatter.

#### Scenario: Design decision produces a decision candidate
Given a design-tree node records a decided decision with rationale
When lifecycle memory integration processes that event
Then it creates a candidate memory fact in the Decisions section
And the candidate references the originating design artifact

#### Scenario: Implementation constraints produce constraint candidates
Given design-tree implementation notes include one or more constraints
When lifecycle memory integration processes those notes
Then it creates candidate memory facts in the Constraints section
And it does not emit candidates for open questions

#### Scenario: OpenSpec archive produces durable spec candidates
Given an OpenSpec change is archived into baseline
When lifecycle memory integration processes the archive event
Then it creates candidate memory facts for durable behavioral truths
And those facts reference the archived spec domain or baseline artifact

### Requirement: Candidate handling respects confidence and authority

Lifecycle-driven memory writes SHALL auto-store explicit structured conclusions and require confirmation for inferred summaries.

#### Scenario: Explicit structured conclusion auto-stores
Given a lifecycle candidate is derived directly from a structured decision, constraint, or archived spec
When the candidate passes deduplication checks
Then it is stored automatically in project memory

#### Scenario: Inferred summary requires confirmation
Given a lifecycle candidate is an inferred architecture or implementation summary rather than an explicit structured statement
When lifecycle memory integration evaluates that candidate
Then it is marked for operator confirmation instead of auto-storage

#### Scenario: Duplicate lifecycle fact supersedes or reuses existing memory
Given a semantically equivalent lifecycle fact already exists in project memory
When lifecycle memory integration processes a newer authoritative version
Then it prefers supersede or reinforcement over storing a duplicate fact
And stale superseded facts remain archived rather than active

### Requirement: Ephemeral workflow chatter does not become durable memory by default

Lifecycle integration SHALL ignore low-signal transient workflow artifacts unless they resolve into a stable conclusion.

#### Scenario: Proposal-stage intent is not auto-stored
Given an OpenSpec change is still in proposal or planning state
When lifecycle memory integration evaluates its artifacts
Then it does not auto-store proposal intent as durable project memory

#### Scenario: Child execution chatter is ignored
Given Cleave child output contains intermediate reasoning or implementation chatter
When lifecycle memory integration evaluates execution artifacts
Then it does not store that chatter as durable memory facts

#### Scenario: Resolved bug stores conclusion not breadcrumbs
Given a bug fix resolves a known issue after review or assessment
When lifecycle memory integration processes the final lifecycle outcome
Then it archives investigation breadcrumbs if present
And it stores one durable conclusion fact describing the fix or workaround

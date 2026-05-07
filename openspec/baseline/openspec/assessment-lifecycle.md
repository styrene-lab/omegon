+++
id = "42ccf8eb-9f3d-47cf-80e3-8382a8f8e596"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Spec

### Requirement: OpenSpec persists latest structured assessment state per change
OpenSpec MUST store the latest lifecycle-relevant structured assessment result with the active change so workflow gating uses a durable authoritative record.

#### Scenario: verify persists latest assessment record
- **Given** an active OpenSpec change
- **When** verification runs a structured assessment for that change
- **Then** OpenSpec writes the latest assessment record into the change directory
- **And** the record includes change name, assessment kind, outcome, timestamp, implementation snapshot identity, and reconciliation hints

#### Scenario: persisted assessment state is attributable to a specific change
- **Given** multiple active OpenSpec changes exist
- **When** one change is assessed
- **Then** only that change's assessment artifact is updated
- **And** lifecycle gating for other changes does not read the wrong assessment state

### Requirement: Verify refreshes or validates assessment state against the current implementation snapshot
`/opsx:verify` MUST actively execute or refresh structured assessment for the current implementation snapshot instead of trusting stale cached output.

#### Scenario: verify refreshes stale assessment state
- **Given** a change has a persisted assessment record for an older implementation snapshot
- **When** `/opsx:verify` runs
- **Then** it detects the stale snapshot
- **And** executes or refreshes the relevant assessment before reporting verification status

#### Scenario: verify can reuse current assessment state
- **Given** a change already has a persisted assessment record for the current implementation snapshot
- **When** `/opsx:verify` runs
- **Then** it may reuse that current record
- **And** reports verification using the structured assessment state without requiring prose parsing

### Requirement: Archive fails closed on missing or non-passing assessment state
Archive MUST refuse to finalize a change unless the latest relevant assessment for the current implementation snapshot is an explicit pass.

#### Scenario: archive is blocked when assessment is missing
- **Given** a change has no persisted relevant assessment state
- **When** archive is requested
- **Then** archive is refused
- **And** the response instructs the operator or agent to run verification first

#### Scenario: archive is blocked when assessment is stale, ambiguous, or reopened
- **Given** a change's latest relevant assessment is stale, ambiguous, or indicates reopened work
- **When** archive is requested
- **Then** archive is refused
- **And** the response explains why the lifecycle gate is not satisfied

#### Scenario: archive succeeds when current assessment explicitly passes
- **Given** a change has a current persisted assessment record with outcome `pass`
- **When** archive is requested
- **Then** archive may proceed past the assessment gate

### Requirement: Structured assessment results compose with reconciliation flows
OpenSpec reconciliation flows MUST consume machine-readable assessment outcomes directly instead of reparsing human-readable assessment text.

#### Scenario: reconciliation uses structured hints from assessment
- **Given** a structured assessment result includes reopened-work or reconciliation hints
- **When** OpenSpec reconciliation runs
- **Then** it uses those structured hints to determine whether tasks, file scope, or constraints need updating
- **And** it does not infer lifecycle state by scraping prose output

#### Scenario: lifecycle commands preserve operator UX while using structured state internally
- **Given** verification or archive is executed interactively
- **When** OpenSpec reads or updates persisted assessment state
- **Then** the operator still receives clear human-readable output
- **And** the underlying lifecycle decisions come from structured state shared with agent workflows

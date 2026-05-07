+++
id = "86ea5d8b-33ce-4593-b664-ac5296ba69ae"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# memory — Delta Spec

## ADDED Requirements

### Requirement: Memory injection metrics are recorded at generation time

The project-memory extension must record structured metrics whenever it builds a memory injection payload so the operator can inspect what was injected into context.

#### Scenario: Full injection metrics are recorded

Given project memory builds a full-dump injection payload
When the payload is generated for before_agent_start
Then the extension records the injection mode as full
And it records the number of injected project facts
And it records the number of injected edges
And it records the number of appended recent episodes
And it records the number of appended global facts
And it records the total payload character count
And it records the estimated token count used for dashboard accounting

#### Scenario: Semantic injection metrics are recorded

Given project memory builds a semantic injection payload
When the payload is generated for before_agent_start
Then the extension records the injection mode as semantic
And it records the number of injected project facts
And it records the number of pinned working-memory facts included
And it records the number of semantic-hit facts included
And it records the number of appended recent episodes
And it records the number of appended global facts
And it records the total payload character count
And it records the estimated token count used for dashboard accounting

### Requirement: Shared state exposes last memory injection metrics

The dashboard must be able to read the last recorded memory injection metrics from shared state.

#### Scenario: Shared state receives the last injection snapshot

Given the project-memory extension generates an injection payload
When it finishes building the payload
Then shared state stores the estimated memory token count
And shared state stores the last injection metrics snapshot

### Requirement: Memory stats report last injection metrics

The `/memory stats` output must surface the most recent injection metrics so the operator can audit memory behavior without reading source code.

#### Scenario: Memory stats include the last injection snapshot

Given the project-memory extension has recorded a last injection snapshot
When the operator runs `/memory stats`
Then the output includes the injection mode
And the output includes injected fact counts
And the output includes appended episode and global-fact counts
And the output includes payload character count
And the output includes estimated token count

### Requirement: Dashboard refreshes from arbitrary on-disk Design Tree and OpenSpec changes

The dashboard event system must treat direct file changes under Design Tree and OpenSpec as first-class state changes so the footer and `/dash` overlay stay current even when files are edited outside extension-managed mutation paths.

#### Scenario: Design Tree disk edits emit dashboard refresh events

Given a Design Tree document under `docs/` changes on disk
When the dashboard producer detects the change
Then it rebuilds the Design Tree dashboard state from disk
And it emits `dashboard:update`
And the footer and overlay render the updated Design Tree state without requiring a session restart

#### Scenario: OpenSpec disk edits emit dashboard refresh events

Given an OpenSpec lifecycle file under `openspec/changes/` changes on disk
When the dashboard producer detects the change
Then it rebuilds the OpenSpec dashboard state from disk
And it emits `dashboard:update`
And the footer and overlay render the updated OpenSpec state without requiring a session restart

#### Scenario: Repeated file saves do not flood the event bus

Given a burst of on-disk writes affects multiple Design Tree or OpenSpec files
When the refresh mechanism observes the changes
Then it coalesces redundant refresh work into bounded event emissions
And it still converges to the latest on-disk state

## MODIFIED Requirements

### Requirement: Injection event records the exact metric set needed for audit

The first instrumentation slice records enough detail to audit current behavior before changing retrieval or dashboard semantics.

#### Scenario: Audit metrics include composition detail

Given an injection payload is generated
When metrics are recorded
Then the snapshot includes fields for project facts, edges, working-memory facts, semantic hits, episodes, global facts, payload characters, and estimated tokens

### Requirement: Dashboard memory bar continues using estimated tokens initially

The first audit slice preserves current dashboard semantics while adding measurement so the team can evaluate whether the estimate should change later.

#### Scenario: Dashboard accounting remains backward compatible

Given memory instrumentation is enabled
When the dashboard renders the memory segment of the context bar
Then it continues using the estimated token count field
And the new metrics are available for inspection without changing the bar semantics yet

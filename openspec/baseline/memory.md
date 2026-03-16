# memory

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

### Requirement: startup import still seeds live memory from tracked transport

The project-memory extension MUST continue to import `.pi/memory/facts.jsonl` into the SQLite fact store automatically at startup so durable knowledge remains portable across clones, branches, and machines.

#### Scenario: startup import seeds an empty or stale DB

Given `.pi/memory/facts.jsonl` contains durable facts
And the local SQLite fact store is empty or stale
When Omegon starts and the project-memory extension initializes
Then the tracked JSONL facts are imported into the live DB without requiring an explicit operator action

### Requirement: tracked facts transport is not rewritten on ordinary session shutdown

The project-memory extension MUST NOT rewrite tracked `.pi/memory/facts.jsonl` as an automatic side effect of ordinary session shutdown.

#### Scenario: branch-local session work does not dirty tracked transport by default

Given the session stores or reinforces durable facts in the SQLite fact store during ordinary branch work
And no explicit memory export or reconciliation action is invoked
When the session ends
Then `.pi/memory/facts.jsonl` is left unchanged on disk

### Requirement: memory transport can be exported explicitly

The system MUST provide an explicit path to rewrite tracked `.pi/memory/facts.jsonl` from the live SQLite fact store when the operator or lifecycle flow intends to reconcile durable memory transport.

#### Scenario: explicit export writes deterministic tracked transport

Given the live SQLite fact store contains durable facts that are not reflected in tracked `.pi/memory/facts.jsonl`
When the operator or a lifecycle reconciliation flow invokes the explicit memory transport export path
Then `.pi/memory/facts.jsonl` is rewritten deterministically from the store export
And repeated exports without intervening durable changes do not rewrite the file again

### Requirement: memory transport drift is reported separately from lifecycle artifact blockers

Readiness and lifecycle checks MUST distinguish `.pi/memory/facts.jsonl` drift from hard blockers involving untracked or missing durable lifecycle artifacts under `docs/` or `openspec/`.

#### Scenario: incidental memory drift does not masquerade as a lifecycle-doc failure

Given the repository has `.pi/memory/facts.jsonl` changes caused by live DB drift
And there are no untracked durable lifecycle artifacts under `docs/` or `openspec/`
When readiness-oriented checks run
Then the result reports memory transport drift as a separate state or warning
And it does not classify that drift as the same hard failure used for missing lifecycle documentation

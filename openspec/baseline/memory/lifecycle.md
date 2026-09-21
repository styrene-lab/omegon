# memory/lifecycle - Baseline

### Requirement: Structured lifecycle conclusions create memory candidates

Lifecycle ingestion SHALL create candidates from explicit decisions, constraints,
and archived behavioral specifications with durable artifact references.

#### Scenario: Decision retains artifact identity
Given a decided lifecycle artifact with rationale and a stable reference
When ingestion stores its decision candidate
Then the fact uses the Decisions section
And the artifact reference and source kind survive database reopen and export/import

#### Scenario: Open question remains outside durable conclusions
Given implementation notes contain an unresolved question and an explicit constraint
When ingestion evaluates those notes
Then only the constraint becomes a durable-conclusion candidate
And its artifact reference is retained

#### Scenario: Archived specification retains its source
Given a completed OpenSpec change has been archived into baseline
When ingestion processes its durable behavioral conclusion
Then the resulting Specs candidate references the baseline or archived artifact

### Requirement: Candidate handling respects confidence and authority

Explicit structured conclusions may be stored automatically after validation.
Inferred lifecycle summaries SHALL require confirmation. Supersession intent and
authority SHALL affect durable state rather than only response text.

#### Scenario: Inferred lifecycle conclusion remains pending
Given lifecycle ingestion receives an inferred summary without confirmation
When candidate admission executes
Then the candidate remains pending confirmation
And it is excluded from automatic context as established knowledge

#### Scenario: Confirmed correction atomically supersedes
Given an admitted lifecycle correction references an existing fact and its expected version
When the correction is committed
Then the original becomes superseded and the replacement retains authority and evidence
And replay returns the recorded outcome without a second replacement

#### Scenario: Conflicting version prevents partial supersession
Given the target fact changed after the correction captured its version
When the correction is committed
Then a version conflict is returned
And no replacement, edge, or success receipt is partially committed

#### Scenario: Model-supplied approval cannot confirm a candidate
Given a pending lifecycle candidate
When a model calls the public review tool with an approval flag or attempts the internal commit invocation
Then the request cannot activate the candidate
And confirmation requires the runtime's operator-response path

#### Scenario: Operator confirms the reviewed snapshot
Given a pending candidate and an affirmative per-request TUI or ACP response
When the runtime commits the reviewed candidate
Then the candidate becomes active with retained inference and review attribution
And an altered candidate digest or stale version prevents admission

#### Scenario: Operator denial or cancellation preserves pending state
Given a pending candidate awaiting operator review
When the operator denies the request or the wait is cancelled before approval
Then no confirmation mutation is dispatched
And the candidate remains pending

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

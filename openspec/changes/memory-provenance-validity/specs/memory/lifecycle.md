# Lifecycle provenance delta

## MODIFIED Requirements

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

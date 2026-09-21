# Candidate reconciliation delta

## ADDED Requirements

### Requirement: Equivalent candidates reuse durable claims

Admission SHALL reconcile scoped semantic equivalence in addition to exact hashes,
preserving source evidence without duplicating an active claim.

#### Scenario: Paraphrase repeats an existing fact
Given an active SQLite-storage fact and an equivalent paraphrase with compatible scope
When candidate reconciliation admits the paraphrase
Then one active claim represents the shared assertion
And evidence references from both candidates remain inspectable

#### Scenario: Same source is processed repeatedly
Given a candidate's evidence identity was already admitted
When that evidence is processed again under a retry
Then independent corroboration and reinforcement are not incremented

### Requirement: Corrections preserve history and applicability

An admitted correction SHALL identify its predecessor and authority. Correction
shall not erase historical evidence or affect incompatible scopes.

#### Scenario: Explicit user correction replaces current knowledge
Given a current claim and an explicit applicable correction with supporting evidence
When the correction is admitted
Then the original becomes superseded and the replacement becomes current
And historical retrieval exposes both claims and the correction relationship

#### Scenario: Different platforms are not contradictory
Given a Linux workaround and a differing macOS procedure
When reconciliation compares them
Then both may remain active in their respective applicability scopes
And neither is superseded solely because its text differs

### Requirement: Unresolved contradictions remain visible

Recency or semantic similarity alone SHALL not select a truth winner. Conflicting
claims without resolving authority SHALL retain an explicit unresolved relationship.

#### Scenario: Newer inference conflicts with an observed fact
Given a supported observation and a newer incompatible inference without resolving evidence
When reconciliation evaluates the inference
Then it records the unresolved conflict without superseding the observation
And context retrieval can expose the disagreement and source distinctions

### Requirement: Reconciliation commits are atomic and replay-safe

Admission SHALL bind a stable operation identity to a validated decision and use
version preconditions for affected durable records.

#### Scenario: Concurrent correction changes the predecessor
Given a reconciliation plan captured a predecessor version that has since changed
When admission commits the plan
Then the operation returns a version conflict
And no partial fact, evidence edge, candidate completion, or success receipt is committed

#### Scenario: Old paraphrase does not resurrect a retired claim
Given a superseded claim and an old equivalent candidate without new authoritative evidence
When reconciliation processes the candidate
Then it does not create a new active copy of the retired claim
And the recorded outcome references the retirement or replacement

### Requirement: Unavailable semantic reconciliation preserves pending work

Classification failures SHALL leave candidates recoverable and report why admission
did not complete. A fallback SHALL not fabricate an equivalence decision.

#### Scenario: Classifier is unavailable
Given a non-exact candidate requires semantic classification and the classifier is unavailable
When reconciliation attempts admission
Then the candidate remains pending with a typed failure reason
And no speculative merge or supersession occurs

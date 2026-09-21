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

#### Scenario: Same evidence under a different operation identity
Given admitted evidence and another candidate derived from that evidence
When admission processes the candidate with a new operation identity
Then it preserves the evidence relationship without adding independent corroboration or reinforcement
And different event IDs, extraction runs, or paraphrases alone do not prove independent support

#### Scenario: Independent support is established
Given equivalent candidates with validated independent source relationships
When admission reconciles their support
Then one active claim retains both sources and the independent-support attribution
And declared provenance alone does not establish that independence

#### Scenario: Applicability is unknown
Given similar claims with insufficient applicability information
When bounded matching evaluates their relationship
Then unknown scope alone permits neither automatic merge nor automatic supersession
And the unresolved decision remains inspectable

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

#### Scenario: Supported refinement adds specificity
Given a broader claim and a compatible candidate whose added detail has supporting evidence
When reconciliation admits a validated refinement under the frozen refinement contract
Then current knowledge exposes the supported specificity and the relationship to the broader claim
And historical evidence is retained without fabricating independent corroboration
And the broader claim follows the version-or-replacement behavior selected in 6A

#### Scenario: Unsupported detail is not a refinement
Given a candidate adds specificity absent from its cited evidence
When reconciliation validates the proposed refinement
Then it does not admit the added detail as supported knowledge
And it records an inspectable unresolved or rejected outcome

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

#### Scenario: Retired claims are absent from active search
Given a superseded claim has no active exact match for an incoming paraphrase
When bounded reconciliation lookup evaluates the candidate
Then lookup includes relevant retired history before deciding it is new
And it does not create an active copy solely because active search missed the predecessor

#### Scenario: Operation identity is reused with a changed payload
Given an operation identity is bound to a prior admission payload
When admission receives that identity with a different payload
Then it returns an operation conflict
And no fact, evidence, candidate-state, or receipt effect is changed

#### Scenario: Commit acknowledgement is lost
Given an admission unit committed but its acknowledgement was lost
When the caller replays the identical operation
Then it receives the recorded outcome
And facts, evidence, and reinforcement are not applied twice

### Requirement: Unavailable semantic reconciliation preserves pending work

Classification failures SHALL leave candidates recoverable and report why admission
did not complete. A fallback SHALL not fabricate an equivalence decision.

#### Scenario: Classifier is unavailable
Given a non-exact candidate requires semantic classification and the classifier is unavailable
When reconciliation attempts admission
Then the candidate remains pending with a typed failure reason
And no speculative merge or supersession occurs

#### Scenario: Classifier output is invalid
Given classification returns malformed output or unsupported evidence references
When reconciliation validates the response
Then it retains recoverable pending work with a typed failure reason
And similarity alone does not authorize admission

### Requirement: Candidate decisions have durable identity and recoverable progress

Admission SHALL distinguish immutable extracted candidates, durable decisions, and
admission completion. Stable identities and the atomic admission unit SHALL be
frozen in 6A before implementation. Completed extraction SHALL not be repeated to
recover decision or admission progress.

#### Scenario: Decision survives a crash before admission
Given a completed formation and a persisted decision with no admission receipt
When recovery resumes after a crash
Then it loads the decision and revalidates target versions and required approval before admission
And it does not re-extract the completed formation or treat the decision as admitted

#### Scenario: Partial progress across admission units
Given one admission unit completed and another unit failed in the same completed formation
When recovery resumes the remaining work
Then the completed unit retains its receipt and effects
And recovery retries only unfinished units without re-extraction
And each retried unit either commits all its effects or none

#### Scenario: Transported capture coverage is not a local receipt
Given an imported formation contains source coverage and decision provenance
When local recovery inspects that formation
Then it does not infer local capture progress or successful local admission from the transported coverage
And local admission requires its own validated operation outcome

### Requirement: Confirmation binds the reviewed reconciliation decision

Review-required admission SHALL bind approval to the candidate snapshot, decision
plan, evidence, applicability, authority, and affected target versions. Inferred
lifecycle admission SHALL retain operator confirmation. Ordinary explicit writes
SHALL NOT acquire a blanket approval requirement from reconciliation.

#### Scenario: Reviewed plan changes
Given an operator approved a review-required decision
When admission receives a changed plan under that approval
Then admission rejects the stale approval and requires renewed review
And no admission effects are committed

#### Scenario: Reviewed target changes
Given approval names a target version that has since changed
When admission applies the reviewed decision
Then it reports a version conflict and invalidates that approval for the revised decision
And it does not silently substitute the current target

#### Scenario: Ordinary explicit write
Given an ordinary explicit write satisfies existing authority and admission policy
When reconciliation validates the write
Then reconciliation does not require blanket operator confirmation
And evidence, scope, and version checks still apply

#### Scenario: Lifecycle inference lacks approval
Given a lifecycle inference has no valid operator confirmation
When the host requests admission
Then it remains pending and excluded from established knowledge
And non-interactive execution does not manufacture approval

### Requirement: Read-side state follows admitted versions and relationships

Read surfaces SHALL distinguish pending decisions, admitted claims, conflicts, and
retired history. Admission SHALL invalidate affected semantic caches. Index work
SHALL remain bound to the current fact version.

#### Scenario: Cached context precedes a correction
Given cached context contains a predecessor before an admitted correction
When context is selected after admission
Then cache validation prevents reuse of the predecessor as current knowledge
And history remains accessible through historical retrieval

#### Scenario: An old indexing job completes after refinement
Given refinement changed a fact version while its old indexing job was running
When the old job attempts completion
Then its stale-version result is rejected
And retrieval cannot present that embedding as the current fact version
And current lexical retrieval remains available without a ready embedding

### Requirement: Reconciliation quality has dedicated evaluation evidence

Acceptance SHALL use new development and held-out reconciliation corpora, not Wave
5 quotation-smoke success. Metrics SHALL cover false merges, duplicate retention,
correction behavior, authority violations, and downstream task regressions.

#### Scenario: Held-out acceptance uses frozen thresholds
Given measured development results and frozen numeric thresholds, corpus digests, and classifier configuration
When the held-out corpus is evaluated
Then results report each reconciliation metric and downstream task regressions against those thresholds
And held-out labels are excluded from classifier inputs
And failures, uncertainty, and unavailable outcomes are reported separately

#### Scenario: Live evaluation needs a fresh budget
Given only the historical Wave 5 live authorization exists
When a new live reconciliation evaluation is prepared
Then dispatch remains blocked until a fresh aggregate token/time budget and route are authorized
And the resulting report records measured usage against that new authorization

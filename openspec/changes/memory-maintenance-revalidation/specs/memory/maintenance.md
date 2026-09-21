# Maintenance delta

## ADDED Requirements

### Requirement: Evidence confidence is distinct from age and attention

Access frequency, references, duplicate writes, and elapsed time SHALL not alone
establish or revoke evidence confidence. Eligibility SHALL consider explicit memory
kind and applicability. Policy SHALL distinguish evidence support, freshness,
usage, validity, and retention before migration. Numeric confidence `1.0` and
reinforcement counts SHALL NOT imply verification.

#### Scenario: Old invariant retains its supporting evidence
Given an old verified architectural invariant whose supporting revision and applicability remain valid
When current retrieval evaluates the invariant after the legacy standard decay cutoff
Then age alone does not exclude it as unsupported knowledge
And its observation and verification dates remain visible

#### Scenario: Repeated access is not independent verification
Given a fact is recalled repeatedly and referenced by a vault note
When maintenance updates its attention metadata
Then evidence confidence and last verified time remain unchanged without new evidence
And usage or reference signals are recorded separately

#### Scenario: Supporting-source inspection is not verification
Given a complete readable source inspection and a successful embedding indexing attempt without claim verification
When maintenance records their outcomes
Then last checked may advance for the complete source check but last verified remains unchanged
And indexing freshness does not establish evidence confidence

#### Scenario: Duplicate stores preserve evidence distinctions
Given a repeated StoreFact request or a vault related_facts reference without independent new evidence
When the managed writer admits the request under the accepted Wave 6 source-independence contract
Then any attention change remains separate from evidence confidence and verification time
And configuration and report compatibility follow the frozen reference-attention mapping

#### Scenario: Old supported selection remains cache-consistent
Given an applicable supported invariant older than the legacy confidence-floor deadline
When selection evaluates it through cold and cached paths and dormancy planning
Then every path preserves its eligibility despite age alone
And validity and support revisions still invalidate cached eligibility

### Requirement: Time-limited knowledge follows explicit applicability policy

Transient guidance SHALL support expiration or revalidation conditions separately
from retention and historical truth. Existing applicability constraints SHALL keep
their platform/workspace/revision/component and inclusive-from/exclusive-until
semantics. Existing archive search SHALL retain its population. A compatible,
explicit all-status retained-history query SHALL support as-of applicability
without requiring a lifecycle transition. This is valid-time assessment of retained
records, not reconstruction of every past database state.

#### Scenario: Temporary workaround expires
Given a workaround whose declared applicability ended before the current time
When current guidance is selected
Then the workaround is excluded as current guidance
And explicit retained-history search can still retrieve it with its validity interval

#### Scenario: Active but expired knowledge remains discoverable
Given an Active fact whose validity has ended and an existing archive-only search client
When an explicit all-status retained-history search queries it as of a time within its validity interval
Then the fact is returned with matching applicability without archiving it
And the existing archive-only client still selects only Archived, Dormant, and Superseded records

#### Scenario: Retained history honors validity boundaries and unknown time
Given retained facts constrained by platform, workspace, revision, component, and a validity interval
When retained-history search assesses them with explicit context or omitted as-of time
Then valid_from is inclusive and valid_until is exclusive for an explicit time
And omitted time remains temporally unknown rather than defaulting to current guidance
And pending or unverified records are labeled without promotion and filters precede result limits

### Requirement: Source changes trigger bounded evidence revalidation

Detected changes to supporting sources SHALL schedule revalidation without
fabricating a verification result or mutating unrelated claims. Work SHALL have
stable identities, bounded paged fanout, fair capacity admission, declared cadence,
backoff, deadlines, cancellation, and durable resume state. Attempt completions
SHALL check source and fact preconditions. Source inspection and indexing SHALL
remain distinct from claim verification. Filesystem cancellation SHALL be described
as cooperative rather than a hard kernel-I/O interrupt.

#### Scenario: Supporting artifact changed
Given a claim references an artifact revision and that artifact has a new revision
When the source-change scan completes
Then the claim is marked for revalidation with the changed reference
And unrelated claims are not marked solely because they share a memory section

#### Scenario: Transient source failure preserves state
Given a supporting vault source cannot be read during a scan
When revalidation processes that source
Then it records evidence unavailability without deleting or archiving the claim
And it does not advance last verified time

#### Scenario: Changed-back content does not bypass checking
Given a source changed away from a recorded revision and later returned to the same content hash
When a queued attempt completes against an obsolete source generation
Then admission rejects the stale completion
And hash equality alone neither erases uncertainty nor advances last verified time

#### Scenario: Incomplete source outcomes cannot become negative findings
Given a missing, denied, unsupported, escaped, oversized, truncated, cancelled, budget-exhausted, or provider-unavailable source attempt
When the attempt reports its outcome
Then prior evidence, last checked, and last verified timestamps remain unchanged
And no negative finding, correction, archive transition, or deletion is inferred from incomplete evidence
And the work record retains a bounded reason and retry or terminal classification

#### Scenario: Readable evidence remains unverified
Given a complete supported-source read that does not establish the claim
When its check result is admitted
Then last checked advances with the source snapshot attribution
And last verified and prior evidence are preserved with unresolved support

#### Scenario: Source flood is paged fairly across restart
Given more linked facts than one source page and more sources than one pass capacity
When the scheduler executes a bounded pass and resumes after reopen
Then persisted watermarks and cursors avoid duplicate admitted work and silent loss
And configured per-source and per-mind quotas permit other eligible sources to progress
And late arrivals are deferred to a later scan with observable pending counts

#### Scenario: Cancellation and outage preserve resumable work
Given an in-flight source or provider attempt with a deadline and a cancellation token
When cancellation, deadline exhaustion, or provider outage ends the attempt
Then no stale completion is admitted and work retains its retry or terminal reason
And retries obey declared backoff, concurrency, byte, token, and queue limits
And storage and lexical retrieval remain available

### Requirement: Maintenance plans preserve concurrency and replay semantics

Plans SHALL identify canonical payloads, operation identity, policy version,
reasons, expected fact versions, source preconditions, and the complete atomic
write-set. They SHALL use existing MemoryMutation/TransitionFacts admission,
FactPrecondition, and payload-bound receipts. Whole-plan admission SHALL reject all
writes on any stale precondition; independent bounded pages are distinct operations.
Replays SHALL preserve the first committed outcome before fresh source/version reads.

#### Scenario: Correction arrives after dormancy planning
Given a maintenance plan targets a fact version and a correction changes that version
When the plan is applied
Then the stale transition is rejected with a conflict
And the corrected record remains unchanged

#### Scenario: Repeated maintenance application
Given a maintenance operation already committed its recorded transitions
When the same operation identity and payload are applied again
Then the original outcome is returned
And no additional transition or evidence reinforcement occurs

#### Scenario: Replay does not depend on source availability
Given a committed maintenance plan whose source was later removed and whose facts now have newer versions
When its exact operation identity and canonical payload are replayed after reopen
Then receipt lookup returns the original effect before fresh source or fact-version reads
And no queue, evidence, or lifecycle state is changed again

#### Scenario: Changed-payload operation reuse is rejected
Given a receipt bound to a maintenance operation identity and payload
When that identity is submitted with a different target, reason, source snapshot, or transition
Then the writer rejects the payload conflict
And no part of the changed plan is applied

#### Scenario: Multi-fact maintenance is whole-plan atomic
Given a plan updates two facts and their evidence, queue completion, and receipt but one fact or source precondition is stale
When the managed writer attempts application
Then neither fact nor any associated evidence, edge, queue completion, or success receipt is committed
And a fresh successful plan commits its entire frozen write-set and selection invalidation atomically

### Requirement: Runtime revalidation uses accepted domain contracts

The host SHALL own source reads, scheduling, optional providers, and command
registration. Durable changes SHALL pass through the managed writer. Domain code
SHALL remain provider-neutral. Correction SHALL consume an accepted integrated
Wave 6 contract reference. A sibling branch SHALL NOT count as dependency acceptance.

#### Scenario: Correction integration remains gated
Given policy DTOs are frozen but Wave 6 correction admission has no accepted integrated reference
When Wave 7 work is scheduled
Then pure policy characterization, readers, and fixtures can proceed
And correction integration remains blocked until its contract and verification references are recorded

#### Scenario: Managed operator surfaces share work outcomes
Given a bounded revalidation operation initiated from a registered operator surface
When its status is inspected from an applicable TUI, CLI, or ACP adapter
Then each adapter reports the same semantic progress, reason, and receipt outcome
And providers and filesystem reads remain host-owned outside domain transactions

#### Scenario: Wave 8 consumes the policy slice
Given the evidence-versus-attention and validity slice is accepted with a versioned contract and verification reference
When Wave 8 checks that dependency
Then it can consume that slice without requiring unrelated Wave 7 scheduler completion

### Requirement: Legacy confidence metadata is migrated without fabricated certainty

Migration SHALL follow the frozen memory-kind and evidence policy and preserve
legacy values distinctly from newly supported evidence confidence. Schema 14 is
the observed baseline; migration numbering SHALL follow the integrated schema.
Transport SHALL follow the field-by-field design matrix, preserving existing
historical metadata and attributed verification without importing local execution
state or resetting destination attention and progress.

#### Scenario: Legacy reinforced fact is migrated
Given a legacy fact with high reinforcement count but no verification evidence
When migration and export/import complete
Then the original reinforcement metadata remains inspectable
And the fact is not labeled independently verified because of that count

#### Scenario: Legacy absence preserves known history
Given an existing fact with operational history and an incoming legacy record lacking that history
When import merges the legacy record
Then absence is treated as unknown rather than an instruction to erase known values
And confidence, reinforcement metadata, last_accessed, and lifecycle timestamps preserve established compatibility semantics

#### Scenario: Portable verification does not import local execution
Given exported evidence and verification attribution plus destination-local attention, queued work, and receipts
When compatible export/import and vault round-trip complete
Then portable evidence and historical metadata retain attribution without fabricating a destination-local verification event
And new local usage, queues, cursors, attempts, backoff, receipts, and check progress are neither imported nor overwritten

### Requirement: Maintenance quality has independent evaluation evidence

Acceptance SHALL use a fresh quality corpus and frozen development thresholds
before held-out evaluation. It SHALL measure stale-guidance precision and retention
of old supported knowledge, plus false verification, retirement, and correction.
Prior Wave 5 evaluation budgets SHALL NOT authorize new live-model spending.

#### Scenario: Held-out evaluation cannot tune its own threshold
Given a development corpus, separate held-out cases, and recorded policy and numerical thresholds
When maintenance quality is evaluated on held-out cases
Then the report uses the previously frozen thresholds and includes per-kind negative cases
And expected labels remain unavailable to the memory writer and retriever

#### Scenario: Live comparison requires a fresh budget
Given only the historical Wave 5 live-comparison authorization
When a Wave 7 live-model quality comparison is proposed
Then execution waits for a new bounded authorization
And deterministic fake-clock and synthetic-source evaluation can proceed independently

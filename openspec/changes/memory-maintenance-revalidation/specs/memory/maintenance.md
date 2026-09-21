# Maintenance delta

## ADDED Requirements

### Requirement: Evidence confidence is distinct from age and attention

Access frequency, references, and elapsed time SHALL not alone establish or revoke
evidence confidence. Eligibility SHALL consider memory kind and applicability.

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

### Requirement: Time-limited knowledge follows explicit applicability policy

Transient guidance SHALL support expiration or revalidation conditions separately
from permanent retention and historical truth.

#### Scenario: Temporary workaround expires
Given a workaround whose declared applicability ended before the current time
When current guidance is selected
Then the workaround is excluded as current guidance
And historical search can still retrieve it with its validity interval

### Requirement: Source changes trigger bounded evidence revalidation

Detected changes to supporting sources SHALL schedule revalidation without
fabricating a verification result or mutating unrelated claims.

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

### Requirement: Maintenance plans preserve concurrency and replay semantics

Plans SHALL identify reasons and expected fact versions. Applying a stale plan
SHALL not overwrite a newer correction. Replays SHALL preserve the first outcome.

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

### Requirement: Legacy confidence metadata is migrated without fabricated certainty

Migration SHALL preserve legacy values and distinguish them from newly supported
evidence confidence. Transport SHALL preserve this distinction.

#### Scenario: Legacy reinforced fact is migrated
Given a legacy fact with high reinforcement count but no verification evidence
When migration and export/import complete
Then the original reinforcement metadata remains inspectable
And the fact is not labeled independently verified because of that count

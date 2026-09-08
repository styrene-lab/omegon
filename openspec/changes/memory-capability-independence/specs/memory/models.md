# Capability readiness delta

## MODIFIED Requirements

### Requirement: Graceful degradation is preserved

Memory SHALL resolve storage, extraction, and embedding availability independently.
Optional provider failure SHALL preserve usable storage and keyword retrieval.

#### Scenario: Extraction works without embeddings
Given durable storage and the configured extraction provider are available but embeddings are unavailable
When a session evidence checkpoint requests extraction
Then extraction can produce durable candidates
And keyword recall remains available
And status reports semantic retrieval as degraded

#### Scenario: Extraction failure does not disable memory tools
Given durable storage is available and the extraction provider is unavailable
When the user stores and recalls a fact
Then storage and keyword recall succeed
And extraction unavailability is distinguishable from an empty extraction result

## ADDED Requirements

### Requirement: Memory capability status is component-specific and read-only

Readiness inspection SHALL report each configured capability without initiating
extraction, writing facts, or probing providers as a rendering side effect.

#### Scenario: Status is requested during an outage
Given extraction is unavailable and an embedding repair is pending
When the operator requests memory status
Then both states are reported independently
And no inference request or durable mutation occurs

### Requirement: Optional indexing work is bounded and repairable

Embedding failure SHALL not roll back an admitted fact. Pending indexing SHALL
be observable and retries SHALL use the selected compatible embedding space.

#### Scenario: Embedding generation times out after storage
Given a fact was committed and its embedding request exceeds the operation deadline
When the operation settles
Then the fact remains available through keyword recall
And indexing is marked incomplete with a retryable or terminal reason
And owned work observes cancellation

#### Scenario: Embeddings recover later
Given a committed fact has retryable pending indexing and the selected embedding service recovers
When bounded indexing repair executes
Then the fact receives a compatible vector
And the fact is not stored or reinforced again

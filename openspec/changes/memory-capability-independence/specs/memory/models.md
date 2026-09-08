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

### Requirement: Invalid extraction configuration does not discard evidence

Invalid optional extraction model configuration SHALL disable that extractor with
a content-free diagnostic while retaining evidence capture and other capabilities.

#### Scenario: Oversized model configuration
Given the configured extraction model exceeds the supported identifier bound
When a session source is captured
Then the evidence episode is retained without invoking the invalid extractor
And the model value is not copied into diagnostics

### Requirement: Extraction models use independently configurable host routing

Parent sessions SHALL configure extraction independently of embedding discovery.
The shipped Rust extraction default SHALL remain explicit, with profile overrides
for model selection and disabling automatic extraction. Child sessions SHALL retain
their existing automatic-extraction-disabled behavior.

#### Scenario: Profile selects an extraction model without embeddings
Given a parent profile selects an extraction model and embeddings are unavailable
When memory capabilities are configured
Then the configured extractor uses that model through existing host completion routing
And no embedding probe result changes the extraction selection

#### Scenario: Operator disables extraction
Given automatic memory extraction is disabled in the profile
When a session ends
Then no extraction inference is requested
And evidence capture and ordinary memory tools remain usable

### Requirement: Embedding selection follows the configured host integration

Embedding model and endpoint selection SHALL follow the existing profile/environment
configuration and supported local fallback rather than impose a new cloud provider.

#### Scenario: Existing embedding configuration remains authoritative
Given the profile specifies an embedding model and endpoint
When memory capabilities are configured
Then embedding discovery uses those values independently of the extraction model

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

## REMOVED Requirements

### Requirement: Default memory extraction uses a cheap GPT cloud model

The historical extension-specific default conflicts with the shipped Rust runtime.
Replace it with the independently configurable host-routing requirement above.

### Requirement: Semantic retrieval uses cloud embeddings by default

The Rust host already supports configured remote and local embedding integrations.
Preserve that policy rather than require a new cloud integration for this repair.

### Requirement: Concrete default memory models are explicit and configurable

Replace historical concrete GPT/embedding defaults with the explicit Rust default
and independently configurable host integrations specified above.

# Inference preparation — Delta Spec

## ADDED Requirements

### Requirement: Validated immutable request preparation

Included requests use one internal validation boundary over existing route owners.
Required evidence succeeds before transport dispatch. Preparation does not grant
tool execution permission, resolve credentials, or acquire session authority.

#### Scenario: Rejected capability or evidence write
Given a request with unsupported capabilities or a failing required evidence writer
When the owner prepares the request
Then no provider stream is opened
And diagnostics contain no credential values or input content

#### Scenario: Exact normal and repair inputs
Given a captured turn tool surface and authority-derived context
When the owner prepares an initial or repaired request
Then dispatch consumes the same system bytes, ordered messages, tools, and options
And selected and serving route identities remain distinct
And repair creates a new request and lease in the same step
And an unchanged transport retry retains its prepared request and joined lease

#### Scenario: Frozen tools and execution permission
Given disabled tools are absent from capture and advertised tools can later be revoked
When a request is prepared
Then later tool additions do not modify its captured surface
And final-response-only requests advertise zero tools
And invocation-time permission and revocation still control execution

#### Scenario: Invalid turn inputs
Given oversized mandatory context, contradictory request ownership, or mismatched authority content
When the owner prepares the request
Then dispatch is rejected before the provider receives inputs

### Requirement: Compaction retains its authority and terminal protocol

Turn, idle, and compatibility compaction prepare no-tools summary inputs through
the common envelope while retaining their existing evidence and collection owners.

#### Scenario: Turn and idle evidence
Given authority-selected compaction input
When the owner prepares the summary request
Then recorded evidence matches selected and serving route identity
And turn compaction retains its turn lease
And idle compaction creates no prompt, turn, or loop step

#### Scenario: Oversized summary input
Given summary input exceeds 100,000 UTF-8 bytes
When the owner prepares compaction
Then authority-backed input is rejected without truncating recorded meaning
And compatibility input is truncated at a character boundary before dispatch

#### Scenario: Summary terminal failure
Given a summary stream ends at EOF, times out, returns a provider error, or completes empty
When the owner collects the response
Then no summary is committed
And successful commitment requires explicit Done and nonempty text

### Requirement: Bounded auxiliary extraction retains sessionless execution

Bounded auxiliary completion has no tools and step-owned route evidence. Source
session attribution does not acquire a session execution binding or append authority.

#### Scenario: Memory extraction acceptance
Given attributed memory evidence and a bounded completion budget
When the extractor requests candidates
Then inference uses the boot-bound route and a sessionless step recorder
And the 30-second extraction deadline and MAX_EXTRACTION_BYTES remain effective
And candidates require explicit Done and successful parser validation

#### Scenario: Cancellation stale generation and overflow
Given cancelled or stale source work, excessive output bytes, EOF without Done, or provider failure
When the memory owner collects or publishes extraction
Then no successful extraction is published
And the existing owner closes or cancels its owned receiver and worker

#### Scenario: Unbounded helper compatibility
Given an existing unbounded quick_completion caller
When its stream ends without Done
Then its existing EOF-return behavior is preserved
And it inherits no interactive retries, tools, or session authority

# Memory formation delta

## ADDED Requirements

### Requirement: Extraction enforces stream limits and terminal completion

Memory extraction SHALL stop accumulating provider text at its byte limit and
require a successful terminal event. Valid JSON followed by EOF alone is not
successful provider completion.

#### Scenario: Oversized provider output
Given provider text deltas exceed the extraction byte budget
When memory extraction collects the stream
Then it rejects the overflowing delta before appending it
And it closes the response receiver while preserving the pending source episode

#### Scenario: Provider closes without Done
Given a provider emits syntactically valid candidate JSON but no Done event
When the stream closes
Then extraction is recorded as unavailable rather than complete

### Requirement: Capture replay identity is stable within a mind and policy

Capture IDs and their source-write payloads SHALL be determined by the mind,
captured evidence, configured extractor, and capture policy. Advisory counters and
the wall clock at retry SHALL not change that payload. Policy upgrades SHALL use
explicitly versioned operation namespaces and retain earlier receipts.

#### Scenario: Delayed repeat changes counters
Given the same source snapshot is processed twice with different advisory counts and durations
When the source write is retried under the same policy
Then the payload remains identical and the recorded operation can replay
And another mind receives a distinct operation identity

### Requirement: Excerpts do not concatenate across unavailable content

An assistant excerpt SHALL remain a contiguous readable prefix. A missing,
restricted, or oversized chunk terminates the excerpt and marks it truncated.

#### Scenario: Unreadable middle chunk
Given an assistant message has a readable prefix, an unreadable middle chunk, and a readable suffix
When an evidence excerpt is captured
Then the suffix is not joined to the prefix
And the excerpt reports truncation

### Requirement: Formation evidence has consistent source identities

Retained evidence sequences SHALL increase strictly and agree with the source
event ID at the frontier. Source identifiers SHALL reject control characters.
Complete extraction SHALL require available nonempty evidence.

#### Scenario: Contradictory source identity
Given two evidence items identify different events at the same sequence
When the formation is validated
Then validation rejects the contradictory evidence before persistence

### Requirement: Session episodes retain substantive committed evidence

Automatic episodes SHALL retain source-linked goals, decisions, attempts, outcomes,
verification, and unresolved work rather than only activity counts.

#### Scenario: Mid-session correction survives finalization
Given an initial plan, a mid-session user correction, and a short final response
When a structured episode is formed
Then the correction and resulting decision are retained with their evidence handles
And retention does not depend on appearing in the first or last message fragment

#### Scenario: Failed attempt is distinguished from verified outcome
Given a failed command followed by a successful verification command
When the episode records the work
Then it identifies both outcomes and their tool evidence
And the failed attempt is not described as a successful workflow

### Requirement: Extraction emits classified evidence-backed candidates

Extractors SHALL produce structured candidates with section/kind and valid source
references. Malformed or unsupported outputs SHALL not be admitted as facts.

#### Scenario: Mixed memory categories are retained
Given evidence contains a decision, a constraint, and an unresolved issue
When the controlled extractor forms candidates
Then each candidate retains its appropriate category and source reference
And the batch is not assigned Architecture uniformly

#### Scenario: Fabricated source reference is rejected
Given extractor output contains one valid candidate and one nonexistent evidence handle
When the output is validated
Then the unsupported candidate is rejected with a reason
And the valid candidate remains available for admission

### Requirement: Evidence checkpoints survive interruption

Committed checkpoint work SHALL be recoverable after process interruption using
stable evidence watermarks and replay-safe operation identities.

#### Scenario: Restart after evidence checkpoint
Given a committed checkpoint whose extraction did not finish before interruption
When the memory pipeline restarts
Then it resumes the pending checkpoint from its retained evidence
And previously completed checkpoints are not admitted twice

#### Scenario: Startup recovery uses the recorded model
Given pending checkpoints for two models and a configured extractor for one model
When the bounded startup recovery pass inventories work
Then it selects only pending checkpoints for the configured model and mind
And completed episodes do not consume the pending-work limit

#### Scenario: Startup recovery reaches its batch limit
Given more than eight matching durable pending checkpoints
When one startup recovery pass runs
Then it considers at most eight checkpoints in creation-time and ID order
And remaining checkpoints stay durable for a later pass

#### Scenario: Crash after a partial candidate batch commit
Given a persisted extraction batch whose first candidate committed before interruption
When batch processing resumes
Then the first candidate replays its prior outcome
And remaining candidates are processed from the persisted extraction result
And reinforcement is not incremented by retry alone

### Requirement: Capture work is bounded and cancellation-aware

Checkpoint scheduling and extraction SHALL obey bounded work limits, preserve
recoverability under backpressure, and settle owned resources on cancellation.

#### Scenario: Checkpoint queue reaches capacity
Given the extraction queue is full and new committed evidence is available
When the next checkpoint is scheduled
Then the unprocessed evidence watermark remains recoverable
And the runtime reports backpressure without losing acknowledged evidence

#### Scenario: Shutdown cancels an in-progress extractor
Given an extractor is running against a durable pending checkpoint
When managed shutdown cancels its work
Then owned resources settle within the declared shutdown contract
And the checkpoint is not falsely marked completed

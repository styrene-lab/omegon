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

### Requirement: Pending recovery progresses while the feature remains active

The hosted recovery worker SHALL discover pending work after startup, process bounded
serial passes, and back off on pass failure. Inspection SHALL distinguish sampled
backlog, unknown backlog, active work, and scheduled retry without exposing evidence.

#### Scenario: Overflow progresses without restart
Given nine matching pending checkpoints and an active configured recovery worker
When bounded recovery passes complete successfully
Then the ninth checkpoint completes without another SessionStart event
And each pass considers at most eight checkpoints

#### Scenario: Later work is discovered from idle
Given the worker observed an empty queue and another checkpoint is committed
When the next 60-second idle interval expires
Then the worker checks the durable pending inventory again
And newly observed backlog schedules another pass after one second

#### Scenario: Repeated pass failure backs off
Given repeated storage failure prevents recovery from completing a pass
When the worker schedules retries
Then the delay doubles from 60 seconds up to a 900-second cap
And successful recovery resets the failure streak
And the queue sample remains unknown after failure

#### Scenario: Discarded feature releases the recovery worker
Given a recovery worker is waiting for its next pass
When its owning feature is dropped
Then cancellation wakes the worker without waiting for the scheduled interval

### Requirement: Interval capture persists committed evidence before finalization

The hosted memory feature SHALL schedule a bounded evidence snapshot after eight
turn-end notifications. Capture SHALL use validated committed replay, preserve
explicit truncation, and persist evidence before any extraction of that checkpoint.

#### Scenario: Long session reaches a capture interval
Given a bound session has committed user-visible evidence and extraction is configured
When its eighth turn-end notification is delivered
Then an owned capture worker persists a pending evidence snapshot before SessionEnd
And the capture worker does not invoke the extractor

#### Scenario: Interval sees the same source snapshot again
Given a source snapshot was already persisted under its capture-policy identity
When a later interval captures that identical source and model
Then the storage operation replays without another episode or reinforcement

#### Scenario: Capture worker is already occupied
Given an interval capture worker is still running
When further turns reach the next interval
Then capture remains due without allocating another worker
And managed shutdown cancels and joins the occupied slot

#### Scenario: Interval source is unavailable
Given the bound source cannot be replayed
When an interval capture worker attempts to read it
Then it returns failure without persisting a fabricated evidence episode

### Requirement: Compaction awaits optional evidence checkpoint acknowledgment

Host compaction and aggressive-decay entrypoints SHALL await published feature
checkpoint hooks before applying their context reduction. Hooks SHALL use a shared
bounded wait and explicit persistence, absence, or unavailability outcomes. Optional
memory failure SHALL NOT veto compaction or fabricate a persistence acknowledgment.

#### Scenario: Snapshot persists before eviction
Given a bound session has committed evidence but has not reached the interval
When the pre-eviction memory hook is awaited
Then it acknowledges persistence only after the durable snapshot write
And the snapshot survives reopening the store

#### Scenario: Host waits for the checkpoint hook
Given a published feature is awaiting checkpoint acknowledgment
When the host prepares context eviction
Then the following eviction action waits for that acknowledgment or the bounded deadline

#### Scenario: Optional hook reaches its deadline
Given an optional checkpoint hook does not finish within the shared ten-second budget
When the deadline expires
Then its wait is dropped and child cancellation is signaled
And compaction may proceed without claiming persistence

#### Scenario: Feature has not been published
Given a registered feature is not in the published contribution graph
When the host prepares context eviction
Then it does not invoke that feature's checkpoint hook

#### Scenario: Interval capture already owns the worker slot
Given a memory capture worker is still active
When a pre-eviction memory checkpoint is requested
Then it reports unavailable with capture_busy
And it does not create another capture worker or label the old snapshot as a new acknowledgment

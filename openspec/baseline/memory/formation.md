# memory/formation - Baseline

### Requirement: Local capture progress is atomic and source-bound

Incremental capture SHALL commit a bounded scanned range and its local resume
cursor atomically. Resume SHALL validate the prior event identity against canonical
replay. Imported coverage declarations SHALL NOT advance local capture progress.

#### Scenario: A page write is interrupted
Given a capture page and cursor update share an operation identity
When receipt persistence fails
Then neither the episode nor the cursor advances
And retry can commit the same page once

#### Scenario: Imported coverage claims a later frontier
Given an imported episode declares coverage beyond locally captured evidence
When the host resumes capture
Then it resumes from local capture receipts rather than the imported frontier

#### Scenario: Two capture workers race
Given two workers read the same local cursor
When they try to append different pages
Then only a contiguous version-checked transition commits
And the loser retains recoverable source evidence

#### Scenario: Middle evidence exceeds one snapshot
Given committed evidence exceeds the item or byte limit of one formation envelope
When bounded incremental capture runs across restart and interval checkpoints
Then successive pages retain eligible evidence without a middle-event gap
And a replaced source frontier cannot be acknowledged as covered

#### Scenario: Replay exceeds its declared resource budget
Given a canonical source exceeds a record, byte, per-file, or replay-time limit
When incremental capture validates its source snapshot
Then it reports unavailable without advancing the capture cursor
And it preserves previously committed pages and canonical source files

#### Scenario: Cancellation during canonical replay
Given a checkpoint worker is validating a canonical source
When its owner cancels the worker
Then replay observes cancellation between bounded read and validation operations
And the worker does not acknowledge persistence of an incomplete snapshot

#### Scenario: Binding is replaced without a generation increase
Given a captured binding target and a replacement with the same session ID and generation
When the old checkpoint attempts to acknowledge coverage
Then its publication identity or snapshot-path mismatch rejects the acknowledgment

#### Scenario: Another session ends during finalization
Given a finalization worker is processing one ended source and its bounded queue has capacity
When another ended source is queued and the active session binding changes
Then the worker captures both source-bound requests without another turn event
And it does not allocate another finalization worker

#### Scenario: Cancelled finalization has queued completion work
Given candidate completion is queued behind another managed storage request
When the owning finalization future is cancelled before execution
Then its service cancellation token is cancelled
And the queued completion cannot later mark the pending episode complete

### Requirement: Incremental coverage is explicit and immutable

Formation version 2 SHALL require an explicit capture-policy version and inclusive
first sequence. The available source frontier supplies the inclusive last sequence.
Evidence SHALL remain within that range. Version 1 snapshots SHALL NOT imply coverage.
Extraction completion and completion import SHALL preserve the version and coverage.

#### Scenario: Legacy snapshot has a recent frontier
Given a version 1 snapshot contains a first goal and a recent suffix
When it is loaded or transported
Then its incremental coverage remains unknown

#### Scenario: Invalid coverage range
Given a version 2 formation has a zero or reversed range, unsupported policy, or evidence outside its range
When the formation is validated
Then validation rejects it before persistence

#### Scenario: Completion changes coverage
Given a persisted pending formation with explicit coverage
When extraction completion or completion import changes its range or version
Then the operation is rejected and the pending formation remains unchanged

#### Scenario: Coverage survives persistence and replay
Given a valid version 2 formation is stored with an operation identity
When its storage operation is replayed and the database is reopened or exported
Then its range and policy remain identical without a duplicate episode

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

#### Scenario: Workflow resumes in another session
Given one session contains failed verification and a later session contains verified repair
When episodes are reopened and transported
Then each outcome retains its own session attribution
And an assistant success assertion cannot replace the earlier failed tool outcome

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

#### Scenario: Crash during candidate batch persistence
Given a pending source checkpoint and a selected extraction result containing multiple candidates
When receipt persistence fails during the atomic candidate batch commit
Then no candidate prefix becomes durable and the source checkpoint remains pending
And retry can persist the selected batch atomically

#### Scenario: Replay after candidate batch persistence
Given a persisted extraction batch and its completion receipt
When the same completion is retried after restart
Then all candidates replay from the persisted batch without another extraction
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

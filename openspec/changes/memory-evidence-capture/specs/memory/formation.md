# Memory formation delta

## ADDED Requirements

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

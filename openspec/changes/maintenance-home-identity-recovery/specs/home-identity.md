# Installation home identity recovery

## ADDED Requirements

### Requirement: Recovery preserves installation authority

Explicit recovery MUST preserve the installation UUID, path-derived deny/session
keys, policy records, and audit chain. Legacy rebinding requires the same
canonical path and inode and an explicit recover command. Unknown historical
causes do not prevent inspection or that explicit decision.

#### Scenario: Read-only inspection
Given a legacy installation with a changed device and matching path and inode
When the operator inspects or dry-runs recovery
Then stored and observed identities and recovery eligibility are reported
And no persistent record is changed

#### Scenario: Policy survives recovery
Given an installation with a denied contribution and denied session
When an eligible recovery settles
Then the original installation UUID and all policy keys remain unchanged
And the contribution and session remain denied
And exactly one recovery audit event is recorded

#### Scenario: Unproven continuity
Given a different home path or inode or conflicting stable volume evidence
When the operator requests recovery
Then recovery refuses and leaves existing authority intact

### Requirement: Recovery excludes concurrent admissions

Recovery MUST take the bootstrap lock and all existing protocol locks without
blocking on inverse lock ordering, and MUST refuse unresolved transactions and
fences. Normal and cached admission MUST refuse a pending recovery journal.

#### Scenario: Busy admission
Given a retained contribution or session admission guard
When recovery attempts to apply
Then recovery reports busy without changing installation state

#### Scenario: Active transaction
Given an unresolved maintenance transaction or fence
When recovery attempts to apply
Then recovery refuses and preserves the transaction and policy records

### Requirement: Interrupted recovery is resumable and audited

Recovery MUST preserve immutable original evidence before replacement and use
atomic durable phase transitions. Ordinary admission MUST reject incomplete
recovery. The original request MUST resume deterministically; conflicting or
tampered journal/state records MUST fail closed.

#### Scenario: Interrupted phases
Given an admitted recovery stopped after intent, state replacement, or audit
When the same request resumes
Then it settles to one target identity and one audit receipt
And no policy record is removed

#### Scenario: Replay and tampering
Given a completed recovery
When the same request is replayed
Then no second audit event is created
And a changed immutable intent or unrelated state is refused

### Requirement: Supported stable identity survives device renumbering

A recorded macOS volume UUID plus canonical directory path and inode MUST permit
subsequent device renumbering without repeated manual rebind. Legacy or
unsupported identities MUST retain explicit recovery requirements.

#### Scenario: Same stable directory after renumbering
Given an installation with persisted matching macOS volume and directory evidence
When the device number changes
Then ordinary bootstrap accepts the same authority without rekeying policy

#### Scenario: Replaced directory or volume
Given a persisted stable identity
When the opened directory inode or volume UUID differs
Then ordinary bootstrap rejects admission

### Requirement: Recovery budgets descriptors without releasing authority locks

Recovery MUST reserve enough process file descriptors for the bounded lock
inventory and a fixed allowance for roots, audit records, and transaction I/O.
It MAY temporarily raise only the companion process soft descriptor limit within
the existing hard limit, and MUST restore the original soft limit afterward.
An insufficient hard limit or failed adjustment MUST refuse before recovery
records are written. Every admitted lock remains held through settlement.

#### Scenario: Low inherited soft limit
Given more maintenance lock files than the inherited soft descriptor limit
And the hard limit permits the bounded recovery descriptor budget
When recovery runs
Then it acquires and retains every required lock and completes recovery
And the original process descriptor limits are restored afterward

#### Scenario: Insufficient hard limit
Given a hard descriptor limit below the bounded recovery budget
When recovery runs
Then it reports a descriptor budget refusal before changing recovery records
And installation identity, policies, audit state, and process limits are unchanged

#### Scenario: Contended lock with an expanded budget
Given recovery can raise its soft limit and a required domain lock is held
When recovery runs
Then it reports busy and leaves authority intact
And expanding descriptor capacity does not relax quiescence checks

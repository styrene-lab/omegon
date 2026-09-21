# Evidence capture design

Depend on provenance and independent extraction readiness. The host captures
committed user-visible messages, tool outcomes, lifecycle conclusions, artifact
references, and finalization state. `loop_session.rs` must expose an evidence
reference/checkpoint rather than only the current 200/300-character fragments.
The session log remains the source of truth. Memory owns structured episodes and
candidate formation contracts.

An episode includes goal, applicable workspace/revision, decisions and rationale,
attempts with observed outcomes, corrections, changed artifacts, verification,
unresolved work, and source references. Keep administrative counts as metadata.
Absence of a tool success must remain unknown verification, not a successful result.

Use evidence watermarks and stable identities `(session, checkpoint, extraction
policy version)` for durable work. Commit the checkpoint reference before scheduling
extraction. Track pending/running/completed/failed work with bounded retries and
recovery after interruption. Content plus operation identity binds a retry. Changed
input or changed extraction policy gets an explicit new work version. Persist a
selected extraction result before replaying its admission mutations so stochastic
retries cannot alter an already committed batch.

Checkpoint on bounded committed-event accumulation and before evidence eviction;
session end flushes the last watermark. Use managed lifecycle resources and
cancellation. Queue saturation retains recoverable source watermarks and reports
backpressure. Do not block the interactive loop on unbounded extraction.

Require structured extractor output: candidate content, section/kind, evidence
references, applicability, and inferred/observed classification. Reject unknown
source handles and malformed candidates independently of valid siblings. Admission
occurs through the later reconciliation contract. Until then retain typed candidates
without representing them as verified durable knowledge.

## Wave 3 implementation decisions

Capture from the host's validated semantic session replay through its deferred
session-view binding. Do not enlarge or reinterpret advisory SessionEnd strings:
they lack source authority and may omit corrections. Capture a fixed session/stream
generation, then read only default-projection committed prompt, assistant text, and
tool-result evidence after the full-spine boundary. Exclude restricted continuity.
Report missing, truncated, or replaced sources explicitly.

Persist a typed optional formation envelope on an episode using a new nullable
SQLite JSON column and schema migration. It holds bounded evidence references and
excerpts, source frontier, validated pending candidates, and extraction outcome.
Original content remains owned by the canonical session log. JSONL transports this
additive field; legacy episodes deserialize with no formation evidence. Pending
generated candidates do not become active facts or acquire verification merely
because a model emitted them. Independent candidate admission and durable extraction
queues remain later-wave work.

Source episodes commit with disabled or pending extraction state before inference.
`CompleteFormation` performs the terminal transition atomically with its receipt,
episode FTS update, and stale episode-vector invalidation. It rejects changes to
the captured source, evidence, truncation state, or selected model. JSONL import can
complete an existing pending envelope under the same checks; it does not downgrade
a completed envelope. Cancellation leaves the pending source durable. Automatic
rescheduling of such records remains Wave 5 scope.

## Wave 5C bounded startup recovery

This section records the initial accepted slice. The continuous scheduler below
supersedes its one-pass lifetime and later-startup-only overflow behavior.

The first recovery slice inventories pending formation envelopes directly from the
storage owner. Filter by mind, pending state, and exact configured extraction model
before applying an eight-record bound. Order by creation time and ID. Completed
episodes cannot hide pending work behind a recent-episode limit. SQLite and the
in-memory backend share this contract; unsupported backends return an explicit error.

On SessionStart, the host starts at most one recovery task per feature instance,
using the existing extraction configuration and child policy. The task has a
120-second total deadline and reuses the 30-second extractor deadline. It consumes
retained evidence, not the current session log. Managed shutdown cancels and joins
the task through the existing owned-task collection. Dropping recovery cancels its
outstanding managed-service request.

Completion uses the existing atomic CompleteFormation contract and a stable
episode-bound recovery receipt. Immutable evidence and selected model must match;
only pending records transition. A concurrent completion cannot be overwritten.
An interrupted provider call may run again because its result was not committed.
This is not an exactly-once provider-execution guarantee or a candidate-admission queue.

The pass preserves overflow and interrupted work for a later startup. Model changes
do not silently reroute old checkpoints. Unavailable terminal outcomes retain their
existing meaning and are not retried here. Continuous scheduling, interval/pre-eviction
capture, retry/backoff state, and full readiness/backpressure projection remain open.
No schema or formation-envelope migration is needed for this slice.

## Wave 5C continuous pending-work scheduler

Keep one owned recovery worker per feature instance. Start its first pass immediately,
then schedule from completion time rather than accumulating missed timer ticks.
Each pass retains the eight-record bound and 120-second deadline. After successful
completion, take another bounded pending inventory sample. A nonempty sample schedules
the next pass after one second; an empty sample schedules it after 60 seconds.

Failed or timed-out passes retain pending evidence and back off for 60, 120, 240,
480, then at most 900 seconds. A successful pass resets the failure streak. This
policy retries pass-level storage/concurrency failures and interruptions. Existing
terminal Unavailable extraction outcomes are not reopened. Queue state is the durable
episode envelope; scheduling delays and telemetry are process-local. Restart begins
with an immediate bounded pass.

Use a latest-value watch channel for content-free runtime observations. Hosted
memory_query exposes enablement, mind, phase, completed pass attempts, failure streak,
scheduled delay, pending sample/cap, and a bounded failure code in its structured
details. A sample is limited to the configured model/mind and is not a live total.
Clear it when a pass begins, so failed inventory does not appear as an empty queue.
Full cross-component readiness and admission backpressure remain separate contracts.

Managed shutdown cancels the worker during work or timer wait and joins its thread.
Feature drop signals cancellation as a fallback. The existing session-end formation
path remains independently owned; concurrent completion is resolved by the domain's
pending-to-terminal precondition. No provider-call exactly-once guarantee is added.

## Wave 5C interval evidence snapshots

Use existing TurnEnd notifications as scheduling signals, not as evidence. After
eight notifications, capture from the validated deferred session binding on an
owned worker thread and persist through StoreEpisode. The tool-result loop records
canonical results before emitting TurnEnd; capture independently validates source
generation and reads only committed records. No new bus event or shared trait is needed.

Allow one interval worker slot per feature. Saturate the due counter at eight while
the slot is occupied. Reap finished work on a later turn; a failed worker leaves
capture due. Spawn failure also preserves the due counter. Reset the interval for a
new session and close admission with managed shutdown. Shutdown cancels and joins
the slot; feature drop cancels it as a fallback. Keep the existing session-end and
recovery workers separately owned.

The worker only commits evidence. It does not invoke the extractor or vault sync.
Reuse the existing capture-policy v2 payload/operation identity, so repeated source
snapshots and matching finalization replay rather than duplicate a checkpoint.
An enabled extractor records Pending with its model; disabled extraction records
Disabled. Reject unavailable source captures and skip empty evidence. A ten-second
async deadline bounds waiting for storage, but synchronous replay is not preemptible.

This slice retains bounded overlapping snapshots rather than introducing a durable
incremental coverage cursor. The source frontier attributes a snapshot, not a promise
that every event before it appears in the excerpt. Canonical source retention, explicit
truncation, and stable receipts preserve existing semantics. Incremental coverage and
pre-eviction acknowledgment remain follow-up contracts.

Hosted memory_query reports the interval, capped event counter, due state, and worker
activity. It does not equate task admission or a finished thread with durable success.

## Wave 5C awaited pre-compaction checkpoints

Add an optional async Feature hook with a default NotApplicable outcome. Shared
traits own its renderer-neutral outcome vocabulary: NotApplicable, Persisted, or
Unavailable with a reason. Existing implementors remain source-compatible through
the default. The event bus invokes only published features and applies one shared
ten-second deadline, cancelling child tokens on return or timeout.

Invoke the hook before the compaction provider can commit its canonical replacement,
not merely before the in-memory conversation projection is updated. Cover automatic
pressure, overflow, feature-requested compaction, manual control/runtime entrypoints,
and aggressive decay. Optional memory unavailability does not veto compaction.

Memory reuses its single owned capture slot and capture-policy v2 receipts. A busy
slot reports Unavailable rather than adding work or treating an older snapshot as
acknowledgment of a new request. A oneshot acknowledgment comes after the managed
storage result. Empty evidence is NotApplicable; source/storage failure is Unavailable.
Wait cancellation signals the worker and leaves its handle owned for managed cleanup.
Synchronous replay itself remains non-preemptible.

Hosted memory_query records the last memory-hook outcome in evidence_checkpoint.
Clear its prior observation before awaiting a new attempt, so a dropped/expired
hook cannot retain an old Persisted label as the new result. A persisted snapshot
still obeys bounded excerpt and explicit truncation semantics; complete incremental
coverage remains open. No persisted schema or extraction authority changes are made.

## Wave 5C durable coverage contract

Introduce formation envelope version 2 with required `coverage` metadata. Its
`first_sequence` and the available source frontier define an inclusive scanned
range. `policy_version` identifies the evidence-selection rules (initially 1).
The producer must inspect every canonical record in that range and retain eligible
evidence, subject to explicit per-excerpt truncation. Sequence gaps in evidence
can represent non-evidence records, so storage validates bounds rather than
pretending it can verify canonical replay completeness.

Version 1 retains unknown coverage and rejects a coverage declaration. Version 2
requires an available source, a nonzero ordered range, a supported policy, and
evidence within the range. Empty evidence is allowed for ranges containing only
non-evidence records, but cannot represent completed extraction. Completion and
completion import cannot change the envelope version or coverage.

The existing JSON formation column carries the additive metadata without a SQL
schema migration. The envelope version prevents older readers from accepting a
new coverage envelope after silently discarding its metadata. Legacy serialization
omits absent coverage and preserves existing v2 capture receipts (which refer to
the host capture policy, not the formation envelope version).

This domain slice does not assign coverage to existing first-goal/recent-suffix
snapshots. Host range selection, stable page identities, durable cursor discovery,
and bounded backlog progression require a subsequent integration slice. Transported
range declarations are not independent proof of source authenticity.

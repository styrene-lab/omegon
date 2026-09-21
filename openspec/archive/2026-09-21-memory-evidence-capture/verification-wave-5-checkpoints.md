# Wave 5C interval evidence snapshots

## Scope

Branch: `feat/memory-wave5-checkpoints`, based on the accepted recovery scheduler
at `de16eb01` and `43804312`.

The host schedules interval evidence persistence after eight TurnEnd notifications.
It reads validated committed replay using the existing session binding and capture
policy. It does not use turn telemetry as source evidence. Canonical tool results
are recorded before the main tool-loop TurnEnd emission; other branches remain safe
because replay only exposes committed facts.

One owned capture slot bounds concurrent interval work. Additional turns preserve
a saturated due counter. Source or storage failure leaves work due for a later turn.
Unavailable sources do not become phantom checkpoints. Managed shutdown cancels and
joins the slot, while feature drop supplies cancellation fallback.

No schema, wire, shared-trait, dependency, or memory-domain mutation changes are made.
Existing capture-policy v2 identities and atomic StoreEpisode receipts provide
idempotency. Disabled extraction still permits source capture. Enabled extraction
leaves a pending envelope for the background scheduler. The capture worker performs
neither inference nor vault synchronization.

These are bounded overlapping snapshots, not complete incremental event coverage.
Existing truncation is explicit. The source frontier identifies the replay snapshot;
it does not certify retention of every preceding event. Pre-eviction acknowledgment
and incremental coverage remain open.

## Red and focused green

The initial `interval_checkpoint` run failed with zero persisted episodes where one
was expected before SessionEnd. All three final focused tests pass:

- `interval_checkpoint_persists_committed_evidence_before_session_end`: validates
  the eight-event threshold, pending model attribution, absence of inline inference,
  repeated-source receipt replay, and SQLite reopen.
- `interval_checkpoint_coalesces_pressure_and_shutdown_joins_single_slot`: uses an
  explicitly signaled occupied worker, verifies one slot and a capped due counter
  after 100 scheduling notifications, checks memory_query observations, and confirms
  managed cancellation/join and closed admission.
- `interval_checkpoint_unavailable_source_does_not_fabricate_episode`: missing source
  replay returns an error and leaves durable episode inventory empty.

## Same-executor review

Review is same-executor, not independent. Checked event/source authority separation,
source-generation guards, bounded task admission, receipt identity, overlap/truncation
semantics, disabled extraction, failure rearming, and shutdown ownership. Existing
formation tests continue to own restricted-content exclusion and attributed outcomes.

No blocking finding remains for interval snapshots. Synchronous canonical replay is
not preemptible, so the async storage deadline is not claimed as a hard CPU/read-time
bound. Full-history replay cost and complete incremental coverage remain follow-up
work before broader checkpoint-retention claims.

## Landing gates

All applicable gates passed:

- `RUST_TEST_THREADS=1 just test-commit`: 5,328 main-crate unit tests passed
  (10 ignored), with passing default integration suites. The opt-in PTY test stayed ignored.
- All five portable `memory_campaign` tests, explicitly run with `--ignored`.
- `just clippy-changed`: main-crate all-targets check passed.
- Named evidence-capture OpenSpec validation and `git diff --check` passed.

This host-only slice did not change domain storage, shared traits, or their feature
configurations. Opt-in live-provider smoke tests do not establish live provider calls.
Existing compact-unwind and dependency future-incompatibility warnings remain.

Execution evidence is recorded in
`sh_0acdd6f1a001rdG51UMQGUUWhl.out` under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.
The interval-snapshot slice is accepted. Wave 5C and the parent corpus remain open
for pre-eviction capture, incremental coverage, and the remaining queue/readiness contracts.

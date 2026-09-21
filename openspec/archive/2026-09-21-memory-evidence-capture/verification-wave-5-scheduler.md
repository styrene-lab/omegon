# Wave 5C continuous pending-work scheduler

## Scope

Branch: `feat/memory-wave5-scheduler`, based on bounded startup recovery at
`7518cc7e` and its accepted record at `b0372d41`.

This host-only slice makes recovery continuous while a feature remains active.
Durable queue state remains the existing pending episode envelope. No schema,
formation wire, domain backend, dependency, or extraction-admission change is needed.

Each serial pass retains the eight-record limit and 120-second deadline. Successful
passes sample remaining work and wait one second for backlog or 60 seconds for an
empty queue. Pass errors/timeouts back off from 60 seconds to a 900-second cap.
Success resets the failure streak. Timers and status are process-local; restart
reconstructs work from durable inventory and runs immediately.

Terminal unavailable extraction outcomes remain terminal. This scheduler retries
pass-level failures and interruption, not every failed provider outcome. Normal
session-end formation can run concurrently; the existing atomic completion contract
prevents overwriting a terminal winner.

## Red and focused green evidence

`cargo test -p omegon --bin omegon recovery_scheduler_drains --locked` initially
failed: eight extractor calls were observed, but nine were required without another
startup. The updated integration test waits for two completed pass attempts and
verifies nine extractions plus an idle/empty observation through hosted memory_query.

Focused filters passed:

- `recovery_scheduler`: overflow progression and feature-drop cancellation.
- `memory::recovery`: paused-clock idle discovery, absence of idle hot polling,
  backlog continuation, bounded exponential backoff, timeout future drop, success
  reset, and cancellation during timer wait.
- `startup_recovery`: SQLite reopen/repeated-owner behavior and managed cancellation
  preserving pending evidence.

The thread-owned integration worker reports progress through a watch channel.
Its tests await state changes with bounded watchdogs rather than polling sleeps.
Timing policy tests use Tokio's paused clock. Structured status distinguishes an
unknown sample from zero and labels capped samples as lower bounds.

## Same-executor adversarial review

Review is same-executor, not independent. Checked bounds, retry delays, clock
behavior, dropped work, queue observation semantics, empty inventories, and lifecycle
ownership. Found that replacing the one-shot task with a long-lived worker required
cancellation when the feature is discarded outside managed shutdown. Added a Drop
fallback and a regression proving an idle worker reaches stopped without waiting
for its timer. Normal managed shutdown still cancels and joins owned threads.

No blocking finding remains in the reviewed slice. Backoff and observations do not
claim persisted retry history, full component readiness, or queue-admission
backpressure. Interval/pre-eviction capture remains pending.

## Landing gates

All applicable gates passed:

- `RUST_TEST_THREADS=1 just test-commit`: 5,325 main-crate unit tests passed,
  10 ignored; default integration suites passed, with the opt-in PTY test ignored.
- All five portable `memory_campaign` tests, explicitly run with `--ignored`.
- `just clippy-changed`: main-crate all-targets check passed.
- Named evidence-capture OpenSpec validation and `git diff --check` passed.

This host-only slice did not change the memory crate or its feature configurations.
Opt-in live-provider smoke tests do not establish live provider execution. Existing
compact-unwind and dependency future-incompatibility warnings remain.

Execution evidence is recorded in
`sh_0acbabe5f001slH2bcBdQgb2W8.out` under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.
The continuous pending-work scheduler slice is accepted. Wave 5C remains implementing
for interval/pre-eviction capture, queue-admission backpressure, and broader readiness.

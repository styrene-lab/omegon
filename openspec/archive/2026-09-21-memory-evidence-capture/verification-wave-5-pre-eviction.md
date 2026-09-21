# Wave 5C awaited pre-compaction checkpoints

## Scope and compatibility

Branch: `feat/memory-wave5-pre-eviction`, based on interval capture at `bbaa51da`
and its acceptance record at `47eb623a`.

The additive Feature hook defaults to NotApplicable. Shared outcome vocabulary is
renderer-neutral and serialized with named states. The host event bus invokes only
published features under a shared ten-second budget, with child cancellation.
Memory reuses its one owned capture slot, canonical replay, and capture-policy v2
receipts. Database schema and formation wire version remain unchanged.

Reviewed call sites:

- `loop.rs`: pressure/manual-request planning path and emergency overflow path,
  before begin_compaction/provider execution.
- `loop_lifecycle.rs`: feature-requested compaction and tier-one aggressive decay.
- `control_runtime.rs`: manual compaction before provider execution.
- `main.rs`: legacy manual compaction before provider execution.

The placement precedes provider-driven canonical CompactionApplied, not just the
later in-memory projection update. This is an awaited bounded-snapshot opportunity,
not a prerequisite that makes optional memory availability mandatory. Busy, missing,
failed, or timed-out capture does not become a false persistence acknowledgment.
Canonical session source remains retained, and incremental coverage remains open.

## Red and focused green

After introducing the compatible no-op hook, the persistence test failed with
NotApplicable where Persisted was expected. This was behavioral red evidence,
not a missing-symbol compilation failure.

Passing focused checks:

- `pre_eviction_checkpoint_acknowledges_durable_snapshot_without_interval`: persisted
  acknowledgment, memory_query outcome, and SQLite reopen before any interval trigger.
- `pre_eviction_hook_is_awaited_before_following_eviction`: controlled oneshot proves
  the following eviction event cannot run before acknowledgment.
- `pre_eviction_hook_timeout_cancels_optional_work_and_allows_progress`: paused-clock
  timeout, child-token cancellation, unpublished-feature exclusion, and absent features.
- Interval checkpoint regressions, including occupied-slot capture_busy reporting.
- `context_checkpoint_outcomes_round_trip_with_named_states`: shared serialized vocabulary.

## Same-executor review

Review is same-executor, not independent. Checked host placement, optional availability,
deadline/cancellation ownership, no inline inference, receipt reuse, source authority,
and false acknowledgment risks. The ordering test exposed a contribution-lifecycle
issue: the hook initially iterated all registered features. It now uses the published
feature prefix, with an explicit unpublished-feature regression.

No blocking finding remains in this scoped review. A cancelled/timed-out waiter does
not abandon worker ownership. Synchronous replay remains non-preemptible, so the
host wait budget is not represented as a hard worker CPU/read-time limit. The last
memory-hook observation is cleared before a new wait and is not a complete ledger
of all compactions.

## Landing gates

The first gate passed shared-trait checks, the memory feature matrix, 5,331 main-crate
unit tests, and default main integration suites. It then failed in the reverse-dependent
native-extension readiness fixture at its 200 ms deadline. That failure also reproduced
on unchanged parent `47eb623a` in an isolated worktree.

Increasing only the fixture deadline to 2,000 ms passed there. A deliberately faulted
handshake that spent the entire startup budget on initialize still failed with the
larger deadline, preserving the test's regression sensitivity. The main worktree
changes only that fixture budget and its explanatory comment; native-extension
production code is unchanged. The diagnostic worktree was removed.

A subsequent attempt reused the deliberately faulted binary from the shared Cargo
target directory. Its unused-variable warning identified the stale artifact. The
affected native-extension and traits build artifacts were cleaned before the final
source rebuild and gate rerun. That attempt is not acceptance evidence.

Portable campaigns and Clippy passed separately after the first gate stopped.
The clean rebuild rerun passed on 2026-09-17:

- Focused native-extension readiness regression passed with the production handshake.
- `RUST_TEST_THREADS=1 just test-commit` passed for all affected crates: omegon,
  omegon-memory, omegon-native-extension-host, omegon-secrets, and omegon-traits.
- Main-crate results: 5,331 unit tests passed (10 ignored), plus default integration
  suites. The opt-in PTY test remained ignored.
- Memory results: 100 unit tests passed (one generator ignored), plus 67 integration tests.
- Shared-trait results: 24 unit and 16 integration tests passed.
- Native-extension results: five tests passed. Secrets results: 89 tests passed.
- `just clippy-changed` passed across all affected crates.

The earlier default/all-features/no-default-features memory gates, explicit traits
gates, and five portable memory campaigns also passed. Opt-in live-provider smoke
tests do not establish live provider execution. Existing compact-unwind and dependency
future-incompatibility warnings remain.

The awaited pre-compaction checkpoint slice is accepted. Wave 5 remains open for
incremental coverage, remaining queue/readiness contracts, and comparative evaluation.

Transcripts: initial gate `sh_0ad7a3571001J3N1HR41keCZ0x.out`, separate campaigns/Clippy
`sh_0ad8155ff001gyOL3ZwQqev6wB.out`, and final clean rebuild
`sh_0ad84b42e001gMA3VptV5CkdlV.out`, under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.
Named OpenSpec validation and whitespace checks passed again after recording acceptance.

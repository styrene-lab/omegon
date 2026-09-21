# Wave 5C final evidence-capture verification

Starting revision: `4785970b`; branch: `feat/memory-wave5-completion`.
The final combined gates are recorded below before acceptance and archival.

## Implementation

Version-2 coverage pages now have local, atomic resume receipts. SQLite and the
in-memory backend validate expected cursors and contiguous ranges. Imports retain
episode evidence but cannot advance a local cursor. Host capture uses policy-v3
operation identities and a frozen, resource-bounded canonical replay per pass.

Startup, interval, pre-eviction, and finalization share range capture. Overflow
drains in eight-page passes. Finalization has one owned worker and an eight-request
queue retaining ended source targets across active-session changes. Full admission
is disclosed without claiming persistence. Remaining source stays in canonical
storage and can resume through its local cursor.

Replay checks cancellation/deadlines during bounded regular-file reads, decoding,
reconstruction, and content validation. It caches verified content for page reads.
Publication IDs, paths, session IDs, and stream identity fence stale snapshots,
including the already-covered path. These are cooperative bounds; kernel file
operations are not claimed to be preemptible.

CompleteFormation persists the selected candidate array and receipt atomically.
The specification now tests failure during the whole-batch commit and replay after
commit. It does not assume a partially acknowledged prefix, which this transaction
cannot produce. Candidate reconciliation and active admission remain Wave 6 work.

## Scenario mapping

| Contract | Evidence |
| --- | --- |
| Local contiguous progress; imports cannot invent a cursor | `capture_cursor_is_local_contiguous_and_replayable` on both backends and both import destinations |
| Formatting-independent receipt lookup | `capture_cursor_uses_semantic_key_fields_after_receipt_reformatting` |
| Receipt-failure rollback and reopen | `capture_cursor_rollback_and_reopen` |
| Concurrent writers | `concurrent_capture_writers_commit_only_one_cursor_transition` with two SQLite connections and a barrier |
| Atomic selected batch and restart replay | `persisted_candidate_batch_replays_atomically_after_restart` |
| Multi-session failed/verified workflow attribution | `multi_session_workflow_preserves_failed_and_verified_attempts` |
| Middle-event coverage and retained corrections | Extended `wave3_capture_retains_goal_correction_and_attributed_outcome` |
| Overflow, reopen, startup resume, finalization replay | `incremental_capture_resumes_bounded_backlog_after_reopen_without_skipping_evidence` with 600 Unicode source statements |
| Source-bound finalization and saturation | `finalization_queue_preserves_rebound_source_and_bounds_workers` |
| Cancelled queued completion | `cancelled_finalization_does_not_complete_a_queued_batch` |
| Bounded replay and cancellation | `bounded_replay_*`, `bounded_blob_*` |
| Same-generation source replacement | `capture_binding_identity_fences_same_generation_rebinds_and_paths` |
| Extraction cancellation/read-only readiness | `cancelled_extraction_publishes_content_free_observation`, `readiness_query_reports_extraction_outage_without_running_inference` |
| Original formation, interval, pre-eviction, recovery and shutdown semantics | Full `features::memory::` suite and earlier accepted verification records |

The evaluation corpus separately records offline stage attribution and the bounded
Codex subscription comparison. Its controlled candidate admission is not described
as automatic active admission by the production capture pipeline.

## Red, review, and fixes

The new cursor tests failed against the initial compatibility implementation with
`formation cursor is unsupported` (`sh_0c4fcc3d8001hHTeej3bN27PIs.out`), then passed
with atomic cursor receipts. Reordered receipt-key JSON produced a separate real
failure before the SQL lookup changed to semantic field predicates.

The first host run exposed loss of the existing sessionless unavailable-source
diagnostic episode. Restored that fallback without using advisory text as evidence.

Independent review by delegated agent `ses_f3adcdbacffeAmdhakopDDY0NK` identified:

1. Dropped SessionEnd requests while another finalizer was busy. Replaced the flag
   gate with the bounded source-bound queue and added the two-source regression.
2. Full synchronous replay per page. Added bounded canonical replay and one frozen
   snapshot per pass, with cancellation/resource tests.
3. Missing snapshot/publication/stream fences and a caught-up early return. Added
   complete identity checks and fresh publication IDs.
4. Uncancelled queued completion/vault work after future drop. Added cancellation
   guards and a blocked-service completion regression.

A fresh independent reviewer, `ses_f3ab0cf30ffe9PeiWbG3LdPAq4`, found no further
confirmed source-layer blocker after those changes. This was static review, not an
independent rerun of the parent's tests. The source-layer fix author and parent also
reviewed their integration boundaries. An oversized assistant-chunk test fixture
initially violated the canonical 64-KiB chunk contract; correcting the fixture
allowed all bounded-replay tests to pass.

## Gates

Rust 1.95.0, `aarch64-apple-darwin`, repository Nix environment.

- Domain default/all-features/no-default-features runs passed during integration.
- The final focused `features::memory::` run passed 71 tests.
- Bounded replay (3), bounded blobs (2), publication identity (1), and the new
  readiness/cancellation filters passed.
- `just clippy-changed --base 4785970b` passed for both affected crates.
- The first broad run stopped on the architecture scanner treating the test-only
  evaluator file as production. Its implementation now uses the recognized
  `#[cfg(test)] mod tests` boundary; the ownership guard remains intact.
- Final broad gates passed in `sh_0c570d5ef0010zuYQ6AlAhaylU.out`: `just lint`,
  serialized `just test-rust`, memory all-features/no-default-features tests,
  and nine optional native-embedding tests. Main unit results: 5,352 passed,
  11 ignored. Default integration suites and workspace doctests also passed.
  The broad workspace gate supersedes a duplicate affected-crate test run.
- All six cursor/workflow tests passed with default and no-default features,
  including the final multi-session case. No additional live model calls were
  made by these gates; the explicit comparison is recorded separately.

Transcripts are under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.
G0–G4 pass for evidence capture on 2026-09-21. Named OpenSpec validation and
archival checks complete the baseline handoff; no capture implementation task
remains open in this change.

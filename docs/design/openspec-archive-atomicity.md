+++
id = "openspec-archive-atomicity"
tags = ["openspec", "lifecycle", "atomicity", "issue-68", "0.23.x"]
aliases = ["issue-68-openspec-archive"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenSpec archive atomicity — Issue 68

## Overview

Issue #68 is a 0.23.x durability fix for OpenSpec archive operations. The current archive path validates the lifecycle FSM, moves `openspec/changes/<name>` to `openspec/archive/<name>`, then persists `ai/lifecycle/state.json`. Ordinary errors are rolled back, but process death after the directory move and before state save can leave content and lifecycle state split.

## Current evidence

- `core/crates/omegon/src/features/lifecycle.rs` handles `openspec_manage archive` by calling `omegon_opsx::Lifecycle::archive_change_with`.
- `core/crates/omegon-opsx/src/fsm.rs` documents the remaining crash caveat in `archive_change_with`.
- `core/crates/omegon/src/lifecycle/doctor.rs` detects archived content whose opsx state is not archived as `openspec_state_drift`.
- Tests already cover normal rollback when state save fails, but not process-death recovery between content move and state save.

## Problem

OpenSpec content and lifecycle JSON are separate filesystem effects. Without a durable intent record, a crash can leave:

```text
openspec/archive/<change> exists
ai/lifecycle/state.json says <change> is verifying/planned/etc.
```

Doctor can detect the drift, but detection alone does not recover the repo or make archive deterministic after interruption.

## Decision: use a small JSON transaction journal for the JSON backend

**Status:** decided
**Rationale:** This is the smallest 0.23.x-safe fix. Moving OpenSpec content under the lifecycle store abstraction or replacing the JSON backend is broader 0.24+ architecture work. A repo-local journal provides deterministic recovery without changing public APIs or storage formats.

## Journal shape

Store pending archive intents under repo-local lifecycle state, for example:

```text
ai/lifecycle/transactions/openspec-archive-<change>.json
```

Suggested payload:

```json
{
  "version": 1,
  "op": "openspec_archive",
  "change": "example-change",
  "from_state": "verifying",
  "to_state": "archived",
  "change_dir": "openspec/changes/example-change",
  "archive_dir": "openspec/archive/example-change",
  "phase": "intent_written"
}
```

Phases should be simple and recoverable:

- `intent_written` — journal exists before content move.
- `content_moved` — archive dir exists and change dir was moved.
- `state_saved` — FSM state save completed; journal can be removed.

## Recovery policy

Recovery should be deterministic and conservative:

| Observed state | Recovery |
|---|---|
| journal exists, change dir exists, archive dir absent | remove stale journal; archive did not begin |
| journal exists, archive dir exists, change dir absent, FSM not archived | mark FSM archived and save, then remove journal |
| journal exists, archive dir exists, FSM archived | remove journal |
| journal exists, both dirs exist | report conflict; do not delete content automatically |
| journal exists, neither dir exists | report conflict; operator intervention required |

Completing the archive is preferred over rollback after `content_moved`, because content has already been moved to the final archive location and the user requested archive. Rollback remains appropriate for ordinary synchronous save errors where the process is still alive and can undo the move immediately.

## Implementation plan

1. Add an `ArchiveTransaction` helper in `core/crates/omegon-opsx` or the lifecycle feature layer.
2. Before `archive_content()`, write the intent journal and fsync/sync the journal file best-effort.
3. After content move succeeds, update journal phase to `content_moved` and sync best-effort.
4. Save lifecycle state as archived.
5. Remove the journal after successful state save.
6. Add recovery entry point used by `lifecycle_doctor` and/or startup before auditing drift.
7. Keep existing rollback behavior for ordinary `save()` errors in the same process.

## Tests required

- Crash after journal write before content move: recovery removes stale journal and leaves state/content unchanged.
- Crash after content move before state save: recovery marks FSM archived and removes journal.
- Crash after state save before journal cleanup: recovery removes journal.
- Both change and archive dirs present: recovery reports conflict and preserves both.
- Neither dir present: recovery reports conflict.
- Existing synchronous save-failure rollback still works.
- `lifecycle_doctor` no longer reports `openspec_state_drift` after recovery of the content-moved crash case.

## Release scope

This is suitable for 0.23.x only if constrained to JSON-backend journaling/recovery and tests. Broader lifecycle-store ownership of OpenSpec content is future architecture work, not a patch release fix.

## Open questions

*No blocking open questions for the scoped 0.23.x fix.*

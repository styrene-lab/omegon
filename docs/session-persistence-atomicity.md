---
id: session-persistence-atomicity
title: "Atomic Session Persistence and Snapshot Compatibility"
status: implemented
tags: [session, upgrade, persistence, 0.23.9]
open_questions: []
dependencies: []
related:
  - session-resume-degradation-visibility
---

# Atomic Session Persistence and Snapshot Compatibility

## Overview

Improve upgrade/restart continuity by making session persistence crash-safe and version-aware. Current session save writes metadata and snapshot JSON with plain fs::write and snapshots lack explicit schema/version metadata, so interrupted updates or schema drift can cause resume failure and fresh-session fallback. This node tracks the 0.23.9 design for atomic session/meta writes and compatible snapshot metadata.

## Research

### Current non-atomic write path

Evidence: core/crates/omegon/src/session.rs save_session writes .meta.json using fs::write and then calls conversation.save_session. ConversationState::save_session in core/crates/omegon/src/conversation.rs serializes SessionSnapshot and writes with std::fs::write. If a process is interrupted during either write, the session directory can contain partial JSON; setup.rs catches load errors and starts fresh with a warning.

### Snapshot compatibility surface

Existing compatibility behavior: SessionSnapshot has #[serde(default)] and contains messages, intent, decay_window, and compaction_summary. That means adding schema_version, omegon_version, and saved_at with defaults should preserve loading of old snapshots. load_session currently has no version-specific migration path; it only tries serde_json::from_str and fails over to fresh session in setup.rs if deserialization fails.

### Atomic write helper suitability

Evidence: core/crates/omegon/src/filelock.rs documents atomic_write_locked as the recommended primitive for shared mutable files. It acquires an advisory lock, writes to a sibling temporary path, then renames over the destination so readers do not observe partial file contents. Session snapshots and metadata are shared mutable files in the same sense as profiles and workspace registry files, so this helper is appropriate for 0.23.9.

### Save ordering

session::save_session now writes the conversation snapshot before writing metadata. This prevents resumed saves from advancing listing metadata when the corresponding snapshot update failed. If metadata write fails after a successful snapshot write, the snapshot remains resumable and stale listing metadata can be corrected on a later save; that is safer than advertising metadata for content that was not persisted.

### Snapshot metadata scope

New snapshots write schema_version = 1, omegon_version = env!("CARGO_PKG_VERSION"), and a lightweight saved_at timestamp. Legacy snapshots deserialize with schema_version = 0 and empty metadata via serde defaults. load_session logs the metadata but does not expose it through ResumeInfo in 0.23.9 to avoid widening UI/API surface for this patch.

## Decisions

### Write session snapshot before metadata

**Status:** accepted

**Rationale:** list_sessions only includes entries with both .meta.json and .json files. Writing the snapshot first and metadata second prevents a new metadata file from advertising a session whose snapshot write failed. Existing old metadata remains in place until the new snapshot is safely written.

### Use atomic locked writes for session JSON and metadata

**Status:** accepted

**Rationale:** Profile persistence already uses filelock::atomic_write_locked to avoid torn writes. Applying the same primitive to session snapshots and metadata reduces resume failures after update/restart interruption without changing session file layout.

### Add backward-compatible snapshot metadata

**Status:** accepted

**Rationale:** schema_version, omegon_version, and saved_at let future upgrades reason about compatibility and produce better recovery messages. With serde defaults, old snapshots remain loadable.

## Open Questions

Resolved for 0.23.9.

## Implementation Notes

### Constraints

- Tests first: add regression coverage for snapshot metadata defaults and atomic-save/listing ordering before implementation.
- Use existing filelock::atomic_write_locked unless research disproves suitability for config-dir session files.
- Keep existing session path and meta filename layout stable.

## Implementation

Implemented in commit scope for 0.23.9:

- core/crates/omegon/src/conversation.rs now writes SessionSnapshot via filelock::atomic_write_locked and includes schema/version metadata.
- core/crates/omegon/src/session.rs now writes the snapshot before metadata and writes metadata via filelock::atomic_write_locked.
- Regression tests cover metadata emission, atomic temp cleanup, and orphan metadata exclusion.

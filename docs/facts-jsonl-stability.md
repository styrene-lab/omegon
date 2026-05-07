+++
id = "602376d3-2fe7-477d-b424-09e74d73b361"
kind = "document"
title = "facts.jsonl stability — durable transport without runtime churn"
status = "implemented"
tags = ["memory", "git", "jsonl", "workflow", "stability"]
aliases = ["facts-jsonl-stability"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
issue_type = "bug"
open_questions = []
openspec_change = "facts-jsonl-stability"
parent = "memory-system-overhaul"
priority = "1"
+++

# facts.jsonl stability — durable transport without runtime churn

## Overview

Reduce .pi/memory/facts.jsonl git churn by separating durable transport state from volatile runtime reinforcement metadata while preserving cross-machine portability and backward-compatible import semantics.

## Research

### Observed churn source

The tracked file .pi/memory/facts.jsonl is exported as a full snapshot of active facts from extensions/project-memory/factstore.ts::exportToJsonl(). Fact and edge lines historically included volatile runtime fields such as confidence, last_reinforced, reinforcement_count, and decay_rate. Normal session use reinforces memory state, so shutdown could rewrite thousands of JSONL lines even when no new durable knowledge was added.

### Current safeguards are insufficient

Project-memory already avoids rewriting facts.jsonl when the exported content is byte-identical by using writeJsonlIfChanged() in extensions/project-memory/jsonl-io.ts. That prevents needless rewrites for unchanged exports, but it does not help when volatile reinforcement metadata changes cause the exported snapshot itself to differ. The result is large git diffs dominated by score/timestamp churn rather than durable memory changes.

### Acceptance shape for a durable transport export

The design is only ready to move to decided if it defines falsifiable acceptance criteria around three properties: (1) reinforcement-only runtime activity must not change exported facts.jsonl bytes, (2) durable knowledge changes must still appear in exported JSONL, and (3) import must continue to accept historical JSONL lines that include richer runtime metadata. This bug is not solved by policy statements alone; the design must encode testable before/after behavior.

## Decisions

### Decision: facts.jsonl should be a durable transport artifact, not a mirror of volatile runtime scoring state

**Status:** decided
**Rationale:** The git-tracked JSONL file exists to move durable project memory between machines and branches. Reinforcement counters, decay scores, and last-access timing are local runtime state that change constantly and do not need to churn in git to preserve portability. Keeping volatile fields in the tracked export turns routine usage into large noisy diffs and obscures real knowledge changes.

### Decision: Import must remain backward-compatible with older JSONL snapshots that include volatile metadata

**Status:** decided
**Rationale:** Existing repositories and archives already contain facts.jsonl lines with confidence, reinforcement_count, last_reinforced, and related fields. The importer should continue to accept these fields when present, but the exporter should stop emitting volatile ones for new snapshots. This gives a clean forward path without migration pain.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/factstore.ts` (modified) — Trim exported JSONL fact and edge records to durable transport fields and keep import tolerant of legacy metadata.
- `extensions/project-memory/factstore.test.ts` (modified) — Add regression coverage proving exports stay stable across reinforcement-only changes, retain the stable durable field set, and remain backward-compatible on import.
- `extensions/project-memory/vectors-episodes.test.ts` (modified) — Update JSONL round-trip expectations if needed for stable transport semantics.
- `docs/facts-jsonl-stability.md` (modified) — Record design rationale and accepted durable field set.
- `openspec/changes/facts-jsonl-stability/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/facts-jsonl-stability/specs/memory/facts-jsonl-stability.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Do not untrack .pi/memory/facts.jsonl; cross-machine portability remains required.
- Do not rely on assume-unchanged/skip-worktree git hacks.
- Exporter should continue to include durable history/identity fields needed for idempotent merge=union import.
- Exporter may trim volatile runtime scoring metadata from both fact and edge lines, but must preserve durable transport semantics.
- Importer must accept both old rich JSONL lines and new stable transport lines.

## Acceptance Criteria

### Falsifiability

- This decision is wrong if: Export stability is falsified if reinforcement-only activity changes the byte content of exported fact JSONL lines.
- This decision is wrong if: Backward compatibility is falsified if import rejects or misprocesses older JSONL lines solely because legacy metadata fields are present.
- This decision is wrong if: Portability is falsified if the stable export removes fields required for idempotent dedup, supersession tracking, or merge=union reconciliation across machines.

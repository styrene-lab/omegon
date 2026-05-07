+++
id = "490765ad-0712-4fac-a89f-0e3588eac0fd"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave checkpoint execution reliability and failure clarity

## Overview

Investigate the confirmed-checkpoint path in cleave so accepting a checkpoint reliably produces a clean worktree and continuation, or surfaces a precise failure cause instead of falling back to a generic dirty-tree blocker.

## Research

### Current post-checkpoint gap

runDirtyTreePreflight() returns "continue" immediately after checkpointRelatedChanges() succeeds, but cleave_run then calls ensureCleanWorktree(). If the checkpoint only staged related files and excluded unrelated/unknown files remain dirty, the operator sees a generic dirty-tree blocker after an apparently accepted checkpoint. The workflow lacks a post-checkpoint cleanliness verification step with precise diagnosis before leaving preflight.

## Decisions

### Decision: Checkpoint attempts must fail closed inside preflight with explicit post-checkpoint diagnosis

**Status:** decided
**Rationale:** A confirmed checkpoint is the operator trust boundary for cleave. If excluded files remain dirty or git commit fails, preflight must keep control and explain the exact reason instead of returning success and letting a later generic clean-worktree error appear.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/index.ts` — checkpoint attempts now re-run `git status --porcelain` before leaving preflight, emit explicit post-checkpoint remaining-dirty diagnosis, and surface git add/commit failures as actionable preflight errors.
- `extensions/cleave/index.test.ts` — regression coverage now includes clean post-checkpoint continuation, remaining excluded dirt after checkpoint, git commit failure, and empty checkpoint scope handling.
- `docs/cleave-checkpoint-failure-clarity.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/cleave-checkpoint-failure-clarity/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

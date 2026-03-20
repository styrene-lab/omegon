---
id: squash-merge-policy
title: Squash-merge policy for feature branches
status: decided
parent: git-harness-integration
tags: [git, cleave, policy]
open_questions: []
---

# Squash-merge policy for feature branches

## Overview

The cleave orchestrator and interactive branch-close flow should squash-merge feature branches by default. Cleave child branches get squash-merged into the base (one commit per child, not N diary commits). Interactive feature branches get squash-merge when the operator merges to main. The diary history stays on the branch for debugging (branch is deleted after merge).

## Decisions

### Decision: Cleave orchestrator uses git2 merge --squash for child branches instead of merge --no-ff

**Status:** decided
**Rationale:** Child diary commits (edit, fix test, re-edit) have no value on main. Squash-merge produces one clean commit per child with the child's label and description as the message. The diary stays on the branch until cleanup. git2's merge + index + commit API supports this natively. For interactive feature branches, the harness should offer squash-merge when the operator closes a branch.

### Decision: Use squash-merge for cleave children (single-task branches) but rebase-cleanup for interactive feature branches

**Status:** decided
**Rationale:** Cleave child branches are single-task, single-session. Their diary has no bisect/revert value — squash is correct. Interactive feature branches (multi-session, multi-step work like vault-fail-closed) contain real fix/feat commits worth preserving for bisect and revert, but also contain ceremony commits (checkpoints, lifecycle, cleave plumbing) that are pure noise. For these, the right policy is rebase-cleanup: drop ceremony commits, keep feat/fix commits, then merge --no-ff. The merge module should offer both: squash_merge (for cleave children) and a cleanup_and_merge that filters out ceremony commits before merging. The ceremony filter matches: chore(cleave), chore(*): checkpoint, chore(*): archive, docs(*): mark * complete, cleave: merge.

## Open Questions

*No open questions.*

+++
id = "a5860068-fd2c-4924-9e39-6b0b594f7d41"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave-dirty-tree-checkpointing — Tasks

Dependencies:
- Group 1 defines shared git-state and preflight types used by later groups.
- Group 2 builds classification and policy logic on top of Group 1.
- Group 3 wires the operator-facing cleave preflight flow after Groups 1-2 exist.
- Group 4 adds tests and docs once implementation behavior is settled.

## 1. Shared git-state + preflight types
<!-- specs: cleave/preflight -->

- [x] 1.1 Add `extensions/lib/git-state.ts` with helpers to inspect changed files, separate tracked/untracked paths, and prepare stashable file sets
- [x] 1.2 Define preflight types in `extensions/cleave/types.ts` for file classes, classification confidence, available operator actions, and preflight outcomes
- [x] 1.3 Define a built-in volatile allowlist including `.pi/memory/facts.jsonl`
- [x] 1.4 Add helpers to prepare checkpoint/stash plans without auto-executing git mutations

## 2. Dirty-tree classification + checkpoint planning
<!-- specs: cleave/preflight -->

- [x] 2.1 Add classification helpers in `extensions/cleave/workspace.ts` for related, unrelated/unknown, and volatile files
- [x] 2.2 Use OpenSpec/design context when available to classify active change artifacts and bound design files with highest confidence
- [x] 2.3 Fall back to generic git-state classification when no OpenSpec change is active
- [x] 2.4 Ensure low-confidence files are excluded from checkpoint scope by default
- [x] 2.5 Generate suggested conventional checkpoint commit messages scoped to the active change when related files are found

## 3. Cleave preflight UX + execution gating
<!-- specs: cleave/preflight -->

- [x] 3.1 Add a dirty-tree preflight in `extensions/cleave/index.ts` before worktree creation begins
- [x] 3.2 Surface a summary that shows related files, unrelated/unknown files, and volatile artifacts separately
- [x] 3.3 Offer explicit operator choices: checkpoint, stash-unrelated, stash-volatile, proceed-without-cleave, or cancel
- [x] 3.4 Ensure checkpoint actions require explicit operator approval before any commit is created
- [x] 3.5 Ensure volatile artifacts are visible but do not block cleave by default
- [x] 3.6 Update dispatcher/pre-execution flow so cleave only proceeds once the preflight outcome permits it

## 4. Tests, docs, and lifecycle reconciliation
<!-- specs: cleave/preflight -->

- [x] 4.1 Add or update cleave tests covering clean-tree bypass, dirty-tree summaries, volatile-only handling, low-confidence unknown files, and generic non-OpenSpec classification
- [x] 4.2 Add tests ensuring checkpoint plans never auto-commit without approval
- [x] 4.3 Add `docs/cleave-dirty-tree-checkpointing.md` describing the checkpoint policy and operator workflow
- [x] 4.4 Reconcile design/OpenSpec artifacts after implementation so the preflight workflow and volatile-file policy reflect reality

+++
id = "a1930ef3-d00a-474f-ac36-662ac4ecb578"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave dirty-tree checkpointing

## Overview

`/cleave` needs a clean git state before it can create worktrees, dispatch child agents, and merge their branches back safely. Dirty-tree checkpointing adds an explicit preflight step so operator intent is captured before parallel execution begins.

The workflow is designed around three policy decisions:

- cleave should surface a **dirty-tree preflight** instead of failing with a bare git error
- **checkpoint commits are lifecycle milestones**, not just end-of-change archive events
- approved **volatile artifacts** such as `ai/memory/facts.jsonl` stay visible but should not block cleave by default

## Research

### Why this exists

OpenSpec and design-tree work often leave the repository in a legitimate in-progress state:

- a proposal, design, or `tasks.md` file was just rewritten
- a previous feature is ready to checkpoint but not yet archived
- tracked operational files changed during the session

Without preflight handling, `/cleave` treats all of that as the same kind of failure. The result is repeated "working tree has uncommitted changes" interruptions at exactly the moment the operator is trying to start parallel work.

### Preflight behavior

When `/cleave` sees a dirty tree, Omegon classifies the changed paths before doing any git mutation.

### Classification buckets

1. **Related**
   - files confidently tied to the active OpenSpec change
   - includes lifecycle artifacts such as `proposal.md`, `design.md`, `tasks.md`, bound design docs, and change-scoped implementation files
2. **Unrelated or unknown**
   - files outside the active change scope
   - low-confidence matches that should not be silently swept into a checkpoint
3. **Volatile**
   - approved operational artifacts such as `ai/memory/facts.jsonl`
   - visible to the operator, but not treated like substantive implementation drift

### Operator actions

The preflight step offers explicit choices:

- **checkpoint**
- **stash-unrelated**
- **stash-volatile**
- **proceed-without-cleave**
- **cancel**

The important property is that Omegon performs the mechanics after the operator makes one policy decision; the operator should not need to manually juggle git commands.

### Checkpoint policy

Checkpointing is intentionally conservative.

- Omegon may prepare a scoped staged set from confidently related files
- Omegon may suggest a conventional commit message scoped to the active change
- Omegon must **not create the commit until the operator explicitly approves it**
- low-confidence or unknown files are excluded from the checkpoint scope by default

That means checkpointing is assisted, not automatic.

### Volatile-file policy

Volatile files are part of the preflight summary so the operator can see them, but they should not block cleave the same way feature drift does.

Expected handling:

- keep volatile paths visible in the summary
- allow a one-step volatile-only stash action
- avoid forcing a full checkpoint or cancel flow when the tree is only dirty because of approved operational artifacts

### Generic mode without OpenSpec

Dirty-tree preflight still matters when there is no active OpenSpec change.

In that case, Omegon should still:

- separate volatile from non-volatile changes
- summarize what it can classify generically from git state
- offer checkpoint, stash, continue-without-cleave, or cancel

The classification is less informed, so the system should bias even harder toward conservative inclusion.

## Decisions

### Decision: Add an explicit cleave preflight checkpoint phase

**Status:** decided
**Rationale:** Before `/cleave` runs, Omegon should detect dirty-tree state and treat it as a first-class workflow step, not just a hard error. The operator should be offered structured choices to checkpoint current work, stash unrelated work, or continue in-session without cleave.

### Decision: Checkpointing should be tied to lifecycle milestones, not only archive

**Status:** decided
**Rationale:** Waiting until an OpenSpec change is fully complete and archived is too late for git hygiene. The useful checkpoint moments are after implementation-complete work, after spec/task rewrites settle, and immediately before a new `/cleave` run begins.

### Decision: Memory sync artifacts should not block cleave by default

**Status:** decided
**Rationale:** Tracked memory files like `ai/memory/facts.jsonl` are operational artifacts that change during normal sessions. Cleave preflight should keep them visible but handle them separately from substantive implementation drift.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/index.ts` (modified) — run dirty-tree preflight before worktree creation and enforce explicit operator choices
- `extensions/cleave/workspace.ts` (modified) — classify related, unrelated, unknown, and volatile paths and build checkpoint plans
- `extensions/cleave/types.ts` (modified) — define preflight result and operator action types
- `extensions/cleave/dispatcher.ts` (modified) — keep execution gated on clean-state preconditions before child dispatch
- `extensions/lib/git-state.ts` (new) — inspect git status, separate volatile artifacts, and support checkpoint/stash planning
- `extensions/cleave/index.test.ts` (modified) — acceptance coverage for clean-tree bypass, dirty-tree summaries, volatile-only handling, generic fallback, and checkpoint approval flow
- `docs/cleave-dirty-tree-checkpointing.md` (modified) — bound design-tree node documenting workflow and policy
- `openspec/changes/cleave-dirty-tree-checkpointing/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Never auto-commit without explicit operator approval.
- Volatile allowlist entries must stay visible in the preflight summary even when they do not block cleave.
- When classification confidence is low, prefer asking, stashing, or canceling over silently bundling files into a checkpoint.
- Preflight must work even when no active OpenSpec change is active, using generic git-state classification.
- Operator choices should minimize manual git steps: one decision, then pi executes the mechanics.
- Generic preflight without OpenSpec context is intentionally conservative: non-volatile files are classified as low-confidence `unknown` and excluded from checkpoint scope by default.
- Cleave dispatch still performs a second dirty-tree guard immediately before child execution; if the repo becomes dirty after preflight, dispatch aborts rather than running worktrees against a changed base.
- Non-volatile dirty trees require interactive input to resolve; in non-interactive contexts runDirtyTreePreflight throws instead of auto-selecting an action (extensions/cleave/index.ts:297-303).
- Cleave now enforces two clean-state guards: preflight before worktree setup and a second git status --porcelain check immediately before child dispatch, aborting if the repo became dirty after preflight (extensions/cleave/index.ts:1838-1857, extensions/cleave/dispatcher.ts:435-445).

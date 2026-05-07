+++
id = "8b788599-2250-4ff2-b679-715368d07961"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave dirty-tree checkpointing and lifecycle commit policy — Design

## Architecture Decisions

### Decision: Add an explicit cleave preflight checkpoint phase

**Status:** decided
**Rationale:** Before /cleave runs, pi-kit should detect dirty-tree state and treat it as a first-class workflow step, not just a hard error. The operator should be offered structured choices: checkpoint commit current work, stash unrelated work, or continue in-session without cleave. This bridges lifecycle state to git state instead of assuming the tree is already clean.

### Decision: Checkpointing should be tied to lifecycle milestones, not only archive

**Status:** decided
**Rationale:** Waiting until an OpenSpec change is fully complete and archived is too late for git hygiene. The useful checkpoint moments are: after a previous feature is implementation-complete, after proposal/spec/tasks rewriting is complete, and before a new /cleave run begins. Archive remains the completion milestone, but intermediate lifecycle checkpoints should be normal and encouraged.

### Decision: Memory sync artifacts should not block cleave by default

**Status:** decided
**Rationale:** Tracked memory files like `.pi/memory/facts.jsonl` are operational artifacts that frequently change during normal sessions. Cleave preflight should either ignore approved volatile paths, auto-stash them, or offer a dedicated 'stash volatile artifacts only' path so they do not repeatedly derail parallel execution.

## Research Context

### Assessment

Yes, there is a recurring workflow disconnect. Cleave requires a clean working tree because it creates worktrees and merges child branches back into the current branch. OpenSpec/design-tree, however, encourage iterative specification and implementation in the same session, and the agent often accumulates legitimate pre-cleave changes (new change scaffolds, rewritten tasks.md, prior completed feature work, memory sync artifacts). This makes 'dirty tree' a predictable lifecycle hazard rather than an incidental mistake.

### Root cause

The fundamental issue is not simply 'forgetting to commit'. It is that cleave's precondition (clean git state) is weaker than the lifecycle's natural cadence. A feature may be logically complete enough to checkpoint but not yet archived, and a new OpenSpec change may be ready for implementation while unrelated local work remains uncommitted. The harness lacks an explicit checkpoint phase between 'implementation/design work accumulated' and 'parallel execution begins'.

### Preflight UX proposal

Before cleave creates worktrees, it should run a structured dirty-tree preflight. If the tree is clean, proceed normally. If dirty, present a concise classification summary: (1) files related to the target OpenSpec change, (2) unrelated feature work, and (3) approved volatile artifacts such as `.pi/memory/facts.jsonl`. Then offer explicit actions: `checkpoint` (commit current relevant work with a suggested conventional message), `stash-unrelated`, `stash-volatile`, `proceed-without-cleave`, or `cancel`. This keeps operator decisions focused on intent rather than manual git mechanics.

### File classification strategy

Preflight can classify changes using a lightweight heuristic stack. Highest confidence: files under the current OpenSpec change path, bound design-tree document, and known lifecycle artifacts (`proposal.md`, `design.md`, `tasks.md`, matching spec domains). Next: files recently touched in the current session that appear in the active change's design file scope. Volatile allowlist: `.pi/memory/facts.jsonl`, optional runtime caches, and other explicitly approved operational artifacts. Everything else is 'unrelated/unknown' and should bias toward stash-or-cancel rather than silent inclusion.

### Checkpoint commit policy

Checkpoint commits should be normal lifecycle artifacts, not signs of failure. When preflight detects relevant completed work, it should suggest a conventional commit message scoped to the active change (for example `feat(models): checkpoint operator capability profile scaffold` or `fix(dashboard): checkpoint truncation and wide overlay`). The system should never auto-commit without operator approval, but it can prepare the exact staged file set and message so the operator makes one judgment call instead of several git commands.

### Recommended behavior for volatile artifacts

Volatile artifacts should be handled by policy, not ad hoc annoyance. V1 can define a small built-in volatile list headed by `.pi/memory/facts.jsonl`. Preflight should exclude these from the 'dirty tree blocks cleave' failure path and either auto-stash them in a named stash entry or offer a one-step 'stash volatile only' action. The key is that volatile files should be visible to the operator but should not be treated like substantive implementation drift.

## File Changes

- `extensions/cleave/index.ts` (modified) — Add dirty-tree preflight before worktree creation and surface operator options
- `extensions/cleave/workspace.ts` (modified) — Add file classification helpers for related/unrelated/volatile changes
- `extensions/cleave/types.ts` (modified) — Define preflight result types and classification enums
- `extensions/cleave/dispatcher.ts` (modified) — Respect preflight resolution before child dispatch begins
- `extensions/lib/git-state.ts` (new) — Optional shared helper for changed-file inspection and stash/commit preparation
- `extensions/cleave/*.test.ts` (modified) — Coverage for dirty-tree preflight, volatile artifact handling, and checkpoint option rendering
- `docs/cleave-dirty-tree-checkpointing.md` (new) — Document lifecycle checkpoint policy and preflight workflow

## Constraints

- Never auto-commit without explicit operator approval.
- Volatile allowlist entries must be visible in the preflight summary even if they do not block cleave.
- When classification confidence is low, prefer asking/stashing/canceling over silently bundling files into a checkpoint.
- Preflight must work even when no OpenSpec change is active, using generic git-state classification.
- Operator choices should minimize manual git steps: one decision, then pi executes the mechanics.

---
id: cleave-checkpoint-parity
title: Cleave checkpoint parity and volatile memory hygiene
status: implemented
parent: repo-consolidation-hardening
tags: [cleave, git, memory, workflow, bugfix]
open_questions: []
---

# Cleave checkpoint parity and volatile memory hygiene

## Overview

Fix the structural gap where cleave_run and /cleave do not share the same dirty-tree preflight behavior, and reduce repeated dirty-tree churn from tracked volatile artifacts such as .pi/memory/facts.jsonl.

## Research

### Observed structural failure mode

The dirty-tree checkpoint workflow exists in `/cleave` command flow (`runDirtyTreePreflight` in `extensions/cleave/index.ts`) but the lower-level `cleave_run` tool path still hits `ensureCleanWorktree()` in `extensions/cleave/worktree.ts` and aborts with a bare dirty-tree error. That means agent-driven and tool-driven execution do not share the same preflight behavior, so the operator can still encounter the old failure mode even after the feature was implemented.

### Why .pi/memory/facts.jsonl keeps reappearing

`extensions/project-memory/index.ts` auto-imports `.pi/memory/facts.jsonl` on session start and unconditionally rewrites the export on session shutdown. Because the file is tracked, ordinary memory reinforcement during a session frequently leaves the working tree dirty. Cleave classification marks the file volatile, but because preflight parity is incomplete and because the file is rewritten often, operators still hit repeated dirty-tree interruptions around parallel execution.

### Probable UX failure in checkpoint approval flow

The current checkpoint flow asks for multiple sequential free-text inputs: action selection, commit message, then explicit `y/yes` approval. That is fragile in terminal UI contexts and easy to mis-handle. A better flow is one structured confirmation step with a suggested message and explicit choices, or an operator-facing confirmation dialog instead of layered text prompts.

## Decisions

### Decision: Unify dirty-tree preflight across /cleave and cleave_run

**Status:** decided
**Rationale:** Dirty-tree checkpointing must live at the execution boundary, not only in the interactive slash-command path. The lower-level tool path and any future callers should invoke the same preflight resolver before worktree creation so operators and agents see one consistent workflow.

### Decision: Treat tracked volatile operational artifacts by policy, not by repeated manual resolution

**Status:** decided
**Rationale:** Tracked files like `.pi/memory/facts.jsonl` are expected to drift during normal operation. The harness should either avoid rewriting them when content is unchanged, support an automatic volatile-only stash/checkpoint policy for preflight, or both, so ordinary background memory sync does not repeatedly interrupt feature execution.

### Decision: Replace multi-prompt checkpoint approval with a single structured confirmation surface

**Status:** decided
**Rationale:** Checkpoint approval should ask the operator for one judgment call, not multiple loosely parsed text replies. A structured confirmation reduces UI fragility and makes the dirty-tree workflow more reliable under both human and agent-driven execution.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/index.ts` (modified) — Unified volatile-only auto-resolution and simplified checkpoint confirmation semantics in dirty-tree preflight
- `extensions/cleave/index.test.ts` (modified) — Updated acceptance coverage for volatile-only auto-stash and single-step checkpoint approval
- `extensions/project-memory/index.ts` (modified) — Use conditional JSONL export write to avoid dirtying facts.jsonl when unchanged
- `extensions/project-memory/jsonl-io.ts` (new) — Shared helper for conditional facts.jsonl writes
- `extensions/project-memory/index.test.ts` (new) — Added unit coverage for unchanged-vs-changed JSONL export writes
- `openspec/changes/cleave-checkpoint-parity/tasks.md` (modified) — Reconciled completed task state after implementation

### Constraints

- Volatile-only dirty trees now auto-stash approved volatile artifacts before cleave continues, avoiding interactive churn for `.pi/memory/facts.jsonl`-only drift.
- Checkpoint confirmation is now a single operator decision: Enter accepts the suggested message, custom text approves with edits, and `cancel` declines without staging or commit side effects.
- Project-memory must not rewrite `.pi/memory/facts.jsonl` when exported JSONL content is byte-identical to the existing file.

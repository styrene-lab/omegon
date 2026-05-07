+++
id = "eecf30b3-1c6e-4d00-985b-b1337afb80b6"
kind = "document"
title = "Cleave task-level progress tracking and operator display"
status = "implemented"
tags = []
aliases = ["cleave-task-progress"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "cleave-child-dispatch-quality"
priority = "3"
+++

# Cleave task-level progress tracking and operator display

## Overview

> Parent: [Cleave child dispatch quality — progress visibility, prerequisite prompting, and submodule awareness](cleave-child-dispatch-quality.md)
> Spawned from: "How should task-level progress be reported? Should children emit structured progress events, or should the orchestrator parse task checkboxes from committed files?"

*To be explored.*

## Research

### Design: task inventory in progress events

The task file already contains a checklist (from OpenSpec tasks.md). The orchestrator can parse this at dispatch time to get a task count per child. The child doesn't need to actively report — the orchestrator already tracks turns and tool calls. The missing piece is a **dashboard widget** that maps what we know (turn count, tool calls, elapsed time, last file touched) against the task inventory.\n\nProposed progress event extension:\n```rust\nProgressEvent::ChildTaskInventory {\n    child: String,\n    total_tasks: usize,\n    scope_files: usize,\n    estimated_loc: Option<usize>,  // from task description heuristics\n}\n```\n\nThe operator display could be:\n```\n│  vault-client [████░░] T4/8  LoC ~800  2m12s │\n│  vault-guards [██████] T3/3  LoC ~30   0m45s │\n│  vault-recipe [░░░░░░] T0/4  pending         │\n```\n\nTurn-to-task mapping is imprecise but useful: if a child has 8 tasks and is on turn 4, it's roughly half done. LoC can be estimated by watching `write` tool calls in the ChildActivity stream (sum target file sizes from the worktree after each write event).\n\nFor a better signal, children could write a `PROGRESS.md` file in their worktree that the orchestrator polls. But this adds complexity and the turn-based heuristic is probably good enough for v1.

## Decisions

### Decision: Use task count from checklist + turn/tool heuristics for progress, not child-reported structured events

**Status:** exploring
**Rationale:** Children are LLM agents — they'll forget to emit progress events or do it inconsistently. The orchestrator has everything it needs: task count parsed from the checklist at dispatch, turn number from stderr parsing, tool calls from ChildActivity, and file sizes from the worktree filesystem. Combine these into a synthetic progress bar rather than adding a reporting contract to the child prompt. This keeps the child focused on implementation and avoids prompt token budget spent on progress reporting instructions.

### Decision: Orchestrator-side heuristics from task count + turns + file stats — no child reporting contract

**Status:** decided
**Rationale:** Decided above — the orchestrator has all the data it needs. Child-reported events add prompt complexity and are unreliable.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/cleave/progress.rs` (modified) — Add ChildTaskInventory event with total_tasks, scope_files, estimated_loc. Add ChildProgress event emitted periodically with completed_tasks (heuristic), loc_written, elapsed_secs.
- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — Parse task checklist count from task file content at dispatch. Emit ChildTaskInventory. Track LoC from write tool targets by stat-ing files in worktree.

### Constraints

- LoC estimation must not block the IO loop — stat files in a background task or on activity events
- Task completion is a heuristic (turn-based) — do not present as exact to the operator
- Progress display should degrade gracefully when task count is unknown (no checklist in task file)

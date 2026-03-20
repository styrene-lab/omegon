# Cleave task-level progress tracking and operator display — Design Spec (extracted)

> Auto-extracted from docs/cleave-task-progress.md at decide-time.

## Decisions

### Use task count from checklist + turn/tool heuristics for progress, not child-reported structured events (exploring)

Children are LLM agents — they'll forget to emit progress events or do it inconsistently. The orchestrator has everything it needs: task count parsed from the checklist at dispatch, turn number from stderr parsing, tool calls from ChildActivity, and file sizes from the worktree filesystem. Combine these into a synthetic progress bar rather than adding a reporting contract to the child prompt. This keeps the child focused on implementation and avoids prompt token budget spent on progress reporting instructions.

### Orchestrator-side heuristics from task count + turns + file stats — no child reporting contract (decided)

Decided above — the orchestrator has all the data it needs. Child-reported events add prompt complexity and are unreliable.

## Research Summary

### Design: task inventory in progress events

The task file already contains a checklist (from OpenSpec tasks.md). The orchestrator can parse this at dispatch time to get a task count per child. The child doesn't need to actively report — the orchestrator already tracks turns and tool calls. The missing piece is a **dashboard widget** that maps what we know (turn count, tool calls, elapsed time, last file touched) against the task inventory.\n\nProposed progress event extension:\n```rust\nProgressEvent::ChildTaskInventory {\n    child: Stri…

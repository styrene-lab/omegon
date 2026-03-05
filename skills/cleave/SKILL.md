---
name: cleave
description: Task decomposition, code assessment, and OpenSpec integration. Use /cleave for parallel execution, /assess for code review (cleave, diff, spec subcommands), cleave_assess tool for complexity checks.
---

# Cleave

Task decomposition is provided by the **cleave extension** (`extensions/cleave/`).

## Tools & Commands

| Surface | Purpose |
|---------|---------|
| `cleave_assess` tool | Assess directive complexity → execute / cleave / needs_assessment |
| `cleave_run` tool | Execute a split plan with git worktree isolation |
| `/cleave <directive>` | Full interactive workflow: assess → plan → confirm → execute → report |
| `/assess cleave` | Adversarial review of last 3 commits → auto-fix all C/W issues |
| `/assess diff [ref]` | Review changes since ref (default: HEAD~1) — analysis only |
| `/assess spec [change]` | Validate implementation against OpenSpec Given/When/Then scenarios |
| `/assess complexity <directive>` | Check if a task needs decomposition |

## Usage

```
/cleave "Implement JWT authentication with refresh tokens"
```

The directive is assessed for complexity. If it exceeds the threshold (default 2.0),
it's decomposed into 2–4 child tasks executed in parallel via git worktrees.

## OpenSpec Integration

When `openspec/changes/*/tasks.md` exists in the repo, `/cleave` uses it as the
split plan instead of invoking the LLM planner:

1. Detects `openspec/` directory in the working tree
2. Finds changes with `tasks.md` files
3. Matches the directive to a change by name (slug matching)
4. Parses task groups → `ChildPlan[]` (skips all-done groups, caps at 4)
5. Infers dependencies from "after X" / "requires X" / "depends on X" markers
6. Falls back to LLM planner if no matching change is found

This makes OpenSpec the upstream planning layer and cleave the downstream
execution engine. OpenSpec is optional — cleave works standalone.

### Full Lifecycle

When OpenSpec is present, the complete lifecycle is:

```
/opsx:propose → /opsx:ff → /cleave → /assess spec → /opsx:verify → /opsx:archive
```

After `/cleave` completes with an OpenSpec change:
- Tasks are automatically marked `[x]` done in `tasks.md`
- The report includes Next Steps guidance
- If all tasks complete: `/assess spec` → `/opsx:verify` → `/opsx:archive`
- If partial: `/opsx:apply` or `/cleave` again

### Session Start

On session start, active OpenSpec changes are surfaced with task progress.
This status is injected into the agent context (not just displayed).

## Complexity Formula

```
complexity = (1 + systems) × (1 + 0.5 × modifiers)
effective  = complexity + 1  (when validation enabled)
```

## Patterns (9)

Full-Stack CRUD, Authentication System, External Service Integration,
Database Migration, Performance Optimization, Breaking API Change,
Simple Refactor, Bug Fix, Refactor.

## Architecture

```
extensions/cleave/
  index.ts        — Extension entry: registers tools + /cleave command
  assessment.ts   — Pattern library, complexity formula, fast-path triage
  planner.ts      — LLM prompt builder, JSON plan parser, wave computation
  openspec.ts     — OpenSpec tasks.md parser → ChildPlan[] conversion
  dispatcher.ts   — Child process dispatch, AsyncSemaphore, wave execution
  conflicts.ts    — 4-step conflict detection (file overlap, decision
                    contradiction, interface mismatch, assumption violation)
  workspace.ts    — Workspace management under ~/.pi/cleave/
  worktree.ts     — Git worktree create/merge/cleanup under ~/.pi/cleave/wt/
  types.ts        — Shared type definitions
```

## Workspace Layout

Workspaces and worktrees live outside the target repo:

```
~/.pi/cleave/
  <slug>/              — Workspace per run
    state.json         — Serialized CleaveState
    0-task.md          — Child task files
    1-task.md
  wt/                  — Git worktrees
    0-api-layer/       — Isolated working copy per child
    1-db-layer/
```

## Execution Flow

The `/cleave` command handles assessment and planning inline (or via LLM delegation).
`CleaveState` tracks execution from dispatch onward:

```
/cleave command:  assess → [openspec | llm plan] → user confirm
cleave_run tool:  DISPATCH → HARVEST → REUNIFY → COMPLETE | FAILED
```

The `assess`, `plan`, `confirm`, and `report` phases exist in `CleavePhase`
but are not currently used in state transitions — they're reserved for
future resume capability.

On merge failure, branches are preserved for manual resolution.
On success, worktrees and branches are cleaned up automatically.

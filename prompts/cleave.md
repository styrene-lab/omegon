---
description: Task decomposition, code assessment, and OpenSpec lifecycle integration
---
# Task Decomposition & Assessment

Route complex directives through the cleave extension.

## Commands

| Command | Purpose |
|---------|---------|
| `/cleave <directive>` | Decompose and execute in parallel via git worktrees |
| `/assess cleave [ref]` | Adversarial review → auto-fix all C/W issues |
| `/assess diff [ref]` | Review changes since ref — analysis only |
| `/assess spec [change]` | Validate implementation against OpenSpec scenarios |
| `/assess complexity <directive>` | Check if a task needs decomposition |

## Tools

- `cleave_assess` — Assess complexity, get execute/cleave/needs_assessment decision
- `cleave_run` — Execute a split plan with git worktree isolation. Pass `openspec_change_path` when OpenSpec is involved.

## Workflow

1. Assess directive complexity (automatic or via `cleave_assess`)
2. If `openspec/` exists with a matching change, use its `tasks.md` as the plan
3. Otherwise, generate split plan via LLM (2–4 children)
4. Confirm plan with user
5. Dispatch children in dependency-ordered waves
6. Harvest results, detect conflicts, merge branches
7. Report status with per-child duration and merge outcomes

## OpenSpec Lifecycle

When `openspec/changes/<name>/tasks.md` exists and matches the directive,
cleave uses the pre-built task groups directly. After successful merge:

- Tasks are automatically marked `[x]` done in `tasks.md`
- Child task files include design.md decisions and spec acceptance criteria
- Post-merge report includes spec verification checklist and next-step guidance

Full lifecycle:
```
/opsx:propose → /opsx:ff → /cleave → /assess spec → /opsx:verify → /opsx:archive
```

On session start, active OpenSpec changes are surfaced with task progress.
OpenSpec is optional — cleave falls back to its LLM planner when absent.

See `skills/cleave/SKILL.md` for the full reference.

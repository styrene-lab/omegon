+++
id = "8d79d172-92c2-4635-9a3d-6ffa4dd924be"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave

> Task decomposition engine — splits complex work into parallel child tasks executed in isolated git worktrees, with skill-aware dispatch, adversarial review, and OpenSpec integration.

## What It Does

Cleave is Omegon's parallel execution system. Given a directive (or an OpenSpec change with tasks.md), it:

1. **Assesses** complexity via `cleave_assess` — pattern matching against multi-system indicators
2. **Plans** decomposition into child tasks with scope, dependencies, and skill annotations
3. **Dispatches** children as isolated `pi -p --no-session` subprocesses in git worktrees
4. **Reviews** each child's work via an adversarial review loop (optional, opus-tier reviewer)
5. **Merges** branches back, detecting and resolving conflicts
6. **Reports** results with structured outcome data

Children run in full pi agent sessions with all extensions loaded. Each gets a tailored prompt including relevant skill files, spec scenarios, and design context.

The `/cleave` command is the primary interface. `/assess` provides code review (cleave, diff, spec subcommands). Both are bridged via `SlashCommandBridge` for agent access.

## Key Files

| File | Role |
|------|------|
| `extensions/cleave/index.ts` | Extension entry — `/cleave` and `/assess` commands, tool registration |
| `extensions/cleave/dispatcher.ts` | Child process spawning, progress tracking, `buildChildPrompt()` |
| `extensions/cleave/planner.ts` | Task decomposition planning from directives |
| `extensions/cleave/workspace.ts` | Git worktree management, branch creation, merge |
| `extensions/cleave/worktree.ts` | Low-level worktree operations |
| `extensions/cleave/conflicts.ts` | Merge conflict detection and resolution |
| `extensions/cleave/review.ts` | Adversarial review loop — `executeWithReview()`, severity gating, churn detection |
| `extensions/cleave/skills.ts` | Skill matching — `matchSkillsToChild()`, `resolveSkillPaths()` |
| `extensions/cleave/assessment.ts` | `/assess` implementation — diff review, spec verification |
| `extensions/cleave/bridge.ts` | SlashCommandBridge registration for `/assess` |
| `extensions/cleave/openspec.ts` | OpenSpec integration — `openspecChangeToSplitPlan()`, spec scenario assignment |
| `extensions/cleave/types.ts` | `ChildPlan`, `SplitPlan`, `CleaveResult` types |
| `extensions/cleave/guardrails.ts` | Pre-merge quality checks |
| `extensions/cleave/lifecycle-emitter.ts` | Design-tree/memory lifecycle event emission |

## Design Decisions

- **Preflight checkpoint phase**: Before dispatching children, cleave checks for dirty git state and offers to stash/commit. Memory sync artifacts (`ai/memory/` or legacy `.omegon/memory/`) don't block by default.
- **Skill-aware dispatch**: Children receive relevant skill files based on scope glob matching (e.g., `*.py` → python skill, `Dockerfile` → OCI skill). Annotation-first, then auto-match.
- **Adversarial review loop**: Optional opus-tier reviewer checks each child for bugs, security issues, spec compliance. Severity-gated: nits → accept, warnings → 1 fix iteration, critical → 2 fix iterations, security → immediate escalation. Churn detection (Jaccard >50%) prevents fix loops.
- **Tier routing**: Child model selection respects effort tiers — local models for simple tasks, sonnet/opus for complex ones via `resolveModelTier()`.
- **OpenSpec fast path**: When an OpenSpec change has `tasks.md`, `/cleave` converts task groups directly into child plans with spec scenario assignment, skipping the planning step.

## Behavioral Contracts

See `openspec/baseline/cleave/preflight.md` and `openspec/baseline/cleave/spec.md` for Given/When/Then scenarios covering:
- Preflight dirty-tree detection and checkpointing
- Child dispatch isolation in worktrees
- Merge conflict detection
- Review loop severity gating
- OpenSpec task-to-child mapping

## Constraints & Known Limitations

- Children are full pi processes — each consumes a model API call budget
- Git worktrees require the repo to not be in a bare state
- Maximum 4 parallel children by default (configurable via `max_parallel`)
- Review loop adds significant latency (opus call per child per review round)
- Merge conflicts between children require manual resolution if auto-merge fails

## Related Subsystems

- [Dashboard](dashboard.md) — receives `CleaveState` updates for progress display
- [OpenSpec](openspec.md) — provides task plans and spec scenarios for execution
- [Model Routing](model-routing.md) — resolves child execution models via effort tiers
- [Project Memory](project-memory.md) — lifecycle events emitted on cleave completion

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

### Spec-Domain Annotations

Task groups in `tasks.md` can declare which spec files they own via HTML comments:

```markdown
## 2. RBAC Enforcement
<!-- specs: relay/rbac -->
- [ ] Wire has_capability() into create_session()
```

Cleave uses these annotations for deterministic scenario-to-child matching (3-tier priority):

1. **Annotation match** — child's `specDomains` includes the scenario's domain
2. **Scope match** — child's file scope includes files referenced in the scenario
3. **Word-overlap fallback** — shared words between child description and scenario text

### Orphan Scenario Safety Net

Any spec scenario matching zero children is auto-injected into the closest child
with a `⚠️ CROSS-CUTTING` marker. This prevents enforcement scenarios from falling
between children when task groups are split by layer instead of by spec domain.
The markers provide observability — if many orphans appear, task grouping needs improvement.

### Full Lifecycle

When OpenSpec is present, the complete lifecycle is:

```
/opsx:propose → /opsx:ff → /cleave → /assess spec → /opsx:verify → /opsx:archive
```

After `/cleave` completes with an OpenSpec change:
- Tasks are automatically reconciled in `tasks.md`
- If completed work cannot be mapped back to task groups, treat that as a lifecycle reconciliation warning and fix the OpenSpec plan before archive
- The report includes Next Steps guidance
- If all tasks complete: `/assess spec` → `/opsx:verify` → `/opsx:archive`
- If partial: `/opsx:apply` or `/cleave` again
- After `/assess spec` or `/assess cleave`, run post-assess reconciliation so failed/partial review can reopen implementation state and append design-tree implementation-note deltas
- Before archive, ensure the bound design-tree node and OpenSpec task state both reflect reality

### Session Start

On session start, active OpenSpec changes are surfaced with task progress.
This status is injected into the agent context (not just displayed).

## Complexity Formula

```
complexity = systems × (1 + 0.5 × modifiers)
effective  = complexity + 1  (when validation enabled)
```

The formula uses bare `systems` (not `1 + systems`) so that single-system,
zero-modifier directives score 1.0 (effective 2.0) — at the default threshold
of 2.0, they get `needs_assessment` rather than being falsely recommended
for decomposition.

## Patterns (12)

Full-Stack CRUD, Authentication System, External Service Integration,
Database Migration, Performance Optimization, Breaking API Change,
Refactor, Bug Fix, Greenfield Project, Multi-Module Library,
Application Bootstrap, Infrastructure & Tooling.

## Adversarial Review Loop

When `review: true` is passed to `cleave_run`, each child's work is reviewed
after execution using a tiered loop:

```
Execute (cheap) → Review (opus) → [pass? done : Fix (cheap) → Review (opus)]
```

### Severity Gating

| Severity | Action | Max Fix Iterations |
|----------|--------|--------------------|
| Nits only | Accept | 0 |
| Warnings | Fix then accept | 1 (configurable) |
| Critical | Fix then escalate if unresolved | 2 (configurable) |
| Critical + security | Immediate escalate | 0 |

### Churn Detection

Between review rounds, issue descriptions are normalized and compared.
If >50% of current issues appeared in the previous round (configurable
threshold), the loop bails — the fix agent is going in circles.

### Review Configuration

Pass to `cleave_run`:
- `review: true` — enable the review loop
- `review_max_warning_fixes: 1` — max fix iterations for warnings
- `review_max_critical_fixes: 2` — max fix iterations for criticals
- `review_churn_threshold: 0.5` — reappearance fraction to trigger bail

### Review State

After execution, each child's state includes:
- `reviewIterations` — number of review rounds completed
- `reviewHistory` — verdict + issue count per round
- `reviewDecision` — `accepted`, `escalated`, or `no_review`
- `reviewEscalationReason` — why the review loop gave up (if escalated)

## Skill-Aware Dispatch

Children automatically receive skill directives based on their file scope.
Skills are matched via file pattern → skill mapping (e.g., `*.py` → python,
`Containerfile` → oci) and can be overridden with task annotations.

### Skill Annotations

Task groups in `tasks.md` can declare skills via HTML comments:

```markdown
## 2. Container Build
<!-- skills: oci, python -->
- [ ] Write Containerfile
```

Annotations override auto-matching for that child.

### Model Tier Routing

Skills can hint at the model complexity needed. The resolution order is:
1. Explicit annotation on the child plan (always respected)
2. Local override (if `prefer_local: true` and Ollama available)
3. Skill-based tier hint (highest `preferredTier` wins)
4. Default: sonnet

## Architecture

```
extensions/cleave/
  index.ts        — Extension entry: registers tools + /cleave command
  assessment.ts   — Pattern library, complexity formula, fast-path triage
  planner.ts      — LLM prompt builder, JSON plan parser, wave computation
  openspec.ts     — OpenSpec tasks.md parser → ChildPlan[] conversion
  dispatcher.ts   — Child process dispatch, AsyncSemaphore, wave execution
  review.ts       — Adversarial review loop, severity gating, churn detection
  skills.ts       — Skill matching, resolution, model tier hints
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

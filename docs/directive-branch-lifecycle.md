---
id: directive-branch-lifecycle
title: Directive-Branch Lifecycle — git branch as the unified task boundary
status: exploring
tags: [architecture, lifecycle, git, workflow, design-tree, openspec, cleave]
open_questions:
  - "Should `implement` auto-checkout the created branch, or is that too disruptive for operators who prefer to stay on main?"
  - How should OpenSpec change artifacts (proposal.md, specs, tasks.md, assessment.json) travel with the branch — committed on the branch, or kept on main as shared state?
  - What is the right relationship between cleave worktree branches (ephemeral, per-child) and the directive branch (durable, per-task)? Should cleave children branch off the directive branch rather than main?
  - Should archive automatically merge the directive branch to main (or open a PR), or should that remain a separate operator decision?
  - How do small fixes (single-commit, no spec needed) fit into this model? Is there a lightweight path that skips the branch ceremony, or does every directive get a branch?
  - What ambient enforcement should the system provide when the operator is on the wrong branch for their active directive — warning, auto-switch, or just dashboard visibility?
---

# Directive-Branch Lifecycle — git branch as the unified task boundary

## Overview

Explore tighter coupling between the directive lifecycle (design → spec → implement → assess → archive) and a single git branch that serves as the durable, observable boundary for the entire unit of work.

Today the system has several lifecycle mechanisms that are loosely coordinated:

1. **Design tree nodes** track exploration → decided → implemented status in markdown frontmatter under docs/
2. **OpenSpec changes** (openspec/changes/) hold proposal → spec → tasks → assessment artifacts
3. **Cleave worktrees** create ephemeral `cleave/*` branches for parallel child execution, merged back to base
4. **Feature branches** are created by `design_tree_update(implement)` but are advisory — the operator may or may not check them out, and nothing enforces that work happens on them
5. **Branch cleanup** is an afterthought (archive-branch-cleanup extension)

The gaps this creates:

- Work frequently happens on `main` directly even for multi-commit features, because there's no mechanism that naturally pushes work onto the branch that `implement` created
- OpenSpec changes have no binding to a specific branch — they exist in the repo's working tree regardless of which branch is checked out
- Cleave children create and merge branches, but the parent directive's branch is disconnected from this
- Assessment and archive don't verify that the work was actually done on the expected branch
- Session continuity across restarts loses track of which directive was in progress
- The "properly" path (design → decide → implement → branch → spec → cleave → assess → archive → merge) has too many manual steps where the operator can drift off-track

The hypothesis: if a directive's entire lifecycle is bound to a single branch (created at implement, worked on exclusively, assessed on, archived on, and merged/deleted at completion), the system can provide stronger guarantees, better observability, and less manual ceremony.

## Research

### Current lifecycle flow audit

Traced the full path through the codebase. The current lifecycle has six independent mechanisms that don't enforce a coherent branch discipline:

**1. `design_tree_update(implement)`** (design-tree/index.ts:175–230)
- Creates a `feature/{node-id}` branch and checks it out via `git checkout -b`
- Sets node status to `implementing`, records branch in `branches[]` frontmatter
- Scaffolds an OpenSpec change directory under `openspec/changes/{node-id}/`
- **Gap**: Nothing prevents the operator from switching back to `main` immediately. No subsequent operation checks that work is happening on this branch.

**2. Cleave child dispatch** (cleave/index.ts:2405, worktree.ts:80)
- `getCurrentBranch()` is called at cleave start to determine the base branch
- Children branch from whatever branch is current — usually `main` even when a directive branch exists
- Children merge back to the base branch, not to the directive branch
- **Gap**: Cleave has no awareness of the directive branch. If the operator is on `main`, all cleave work bypasses the directive branch entirely.

**3. OpenSpec artifacts** (openspec/changes/*)
- `proposal.md`, specs, `design.md`, `tasks.md`, `assessment.json` live in the repo working tree
- They're committed to whatever branch happens to be current
- **Gap**: No binding between the change directory and a branch. Artifacts can end up on `main` or split across branches.

**4. Assessment** (cleave/index.ts:1159–1400)
- `/assess spec` evaluates scenarios against the current working tree
- `/assess cleave` does adversarial review of recent changes
- **Gap**: Assessment doesn't check which branch the work was done on, or whether the assessed code matches the directive branch.

**5. Archive** (openspec/index.ts, archive-gate.ts)
- Moves change directory to `openspec/baseline/`
- Transitions bound design-tree nodes to `implemented`
- `deleteMergedBranches()` cleans up branches that are ancestors of HEAD
- **Gap**: Archive only cleans up branches that are already merged. It doesn't merge the directive branch — that's left to the operator.

**6. Dashboard** (design-tree/dashboard-state.ts, dashboard/)
- Shows design-tree node status and OpenSpec change status
- Shows branch bindings in node details
- **Gap**: No indication of whether the operator is on the right branch for their active directive.

### Observed failure modes from recent work

Evidence from the last two weeks of Omegon development:

**Direct-to-main drift**: The majority of recent work (0.6.x–0.7.x releases) was committed directly to `main` despite `implement` creating feature branches. The branches accumulated as stale debris — three were just deleted in this session, all 110+ commits behind main.

**Orphaned branches from cleave**: Cleave creates `cleave/*` branches that merge back to the current branch (usually `main`). When a directive branch exists, cleave doesn't know about it, so work bypasses the directive branch entirely.

**Assessment on wrong branch**: `/assess spec` and `/assess cleave` operate on the working tree of whatever branch is checked out. If the operator is on `main` with all the changes already committed there, the assessment works fine but the directive branch is left untouched.

**Archive without merge**: Archive transitions design nodes to `implemented` and cleans up merged branches, but the directive branch was never merged because the work was done on `main`. The branch stays around as noise.

**Session discontinuity**: When a session ends and a new one starts, there's no mechanism to detect "you were working on directive X on branch Y" and resume that context. The focused design node is persisted, but the branch association is advisory.

### Cleave worktree model as prior art

Cleave already demonstrates the right pattern at the child level:

1. **Branch creation is automatic** — each child gets `cleave/{childId}-{label}`
2. **Isolation is enforced** — git worktrees guarantee children can't step on each other
3. **Merge is part of the lifecycle** — harvest phase merges children back to base
4. **Cleanup is automatic** — worktrees and branches are removed after merge

The question is whether this pattern should be lifted to the directive level:

| Cleave child | Proposed directive |
|---|---|
| `cleave/{childId}` branch | `feature/{node-id}` branch |
| Worktree isolation | Checkout enforcement (softer) |
| Merge to base on harvest | Merge to main on archive |
| Auto-cleanup after merge | Auto-cleanup after archive |

The key difference: cleave children are ephemeral (minutes), directives are durable (hours to days). Worktree isolation makes sense for ephemeral parallelism but is too heavyweight for durable work. A checkout-based model with ambient enforcement is the right analog.

### Proposed unified model sketch

A directive-branch lifecycle would tighten the existing mechanisms into a single coherent flow:

**Phase 1: Initiation** (`implement`)
- Creates `feature/{node-id}` branch (already does this)
- Auto-checkouts the branch (new)
- Sets focus to the design node (new — currently separate)
- Records `active_directive: {node-id, branch}` in shared state (new)

**Phase 2: Work** (normal session)
- On session start, detect if an active directive exists and the current branch matches
- If on wrong branch, surface in dashboard: "Active directive: X (branch: feature/X) — you are on main"
- Cleave reads the active directive and uses the directive branch as `baseBranch` instead of `getCurrentBranch()`
- OpenSpec artifacts are committed to the directive branch naturally (because the operator is on it)

**Phase 3: Assessment** (`/assess spec`, `/assess cleave`)
- Assessment could optionally verify that the assessed branch matches the directive branch
- Assessment results are committed on the directive branch

**Phase 4: Completion** (`/opsx:archive`)
- Archive merges the directive branch to main (fast-forward if possible, merge commit if not)
- Switches back to main
- Deletes the directive branch
- Clears the active directive
- Transitions design node to `implemented`

**Lightweight escape hatch**:
- Single-commit fixes that don't go through `implement` skip all of this — direct commits to main remain valid for bug/chore/task nodes
- `/opsx:propose` (untracked changes) also skip the branch model
- An explicit "I want to work on main" override should exist for operators who prefer trunk-based development for a particular change

### Risk analysis

**Risks of tighter coupling:**

1. **Merge conflicts on archive**: If main advances significantly while work happens on a directive branch, the merge at archive time could be painful. Mitigation: periodic rebase/merge from main, or a "sync" command.

2. **Operator friction**: Auto-checkout may surprise operators who are used to staying on main. Mitigation: make it opt-out, not opt-in. The system prompt already describes the "proper" path; this just enforces it.

3. **Complexity in shared state**: Adding `active_directive` to shared state creates another coordination point. Mitigation: derive it from git state (current branch + design-tree frontmatter) rather than storing separately.

4. **Multi-directive parallelism**: If the operator wants to work on two directives simultaneously, a single-branch model blocks this. Mitigation: this is already the case — you can only be on one branch at a time. Explicit switching between directives is fine.

**Risk of NOT doing this:**

The current state produces stale branches, assessment gaps, and work-on-main drift that undermines the entire lifecycle system. The design-tree and OpenSpec investments lose value when the actual work bypasses them.

## Open Questions

- Should `implement` auto-checkout the created branch, or is that too disruptive for operators who prefer to stay on main?
- How should OpenSpec change artifacts (proposal.md, specs, tasks.md, assessment.json) travel with the branch — committed on the branch, or kept on main as shared state?
- What is the right relationship between cleave worktree branches (ephemeral, per-child) and the directive branch (durable, per-task)? Should cleave children branch off the directive branch rather than main?
- Should archive automatically merge the directive branch to main (or open a PR), or should that remain a separate operator decision?
- How do small fixes (single-commit, no spec needed) fit into this model? Is there a lightweight path that skips the branch ceremony, or does every directive get a branch?
- What ambient enforcement should the system provide when the operator is on the wrong branch for their active directive — warning, auto-switch, or just dashboard visibility?

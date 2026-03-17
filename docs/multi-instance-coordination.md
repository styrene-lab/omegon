---
id: multi-instance-coordination
title: Multi-instance Omegon coordination — parallel work streams on the same repo
status: seed
parent: directive-branch-lifecycle
open_questions:
  - "Should `implement` auto-checkout the created branch, or is that too disruptive for operators who prefer to stay on main?"
  - For Mode B (worktree delegates), should the delegate instance share the same SQLite memory DB (WAL concurrency) or get its own isolated copy that merges facts back on completion?
  - "Is there a lightweight instance-presence signal (PID file, Unix socket, or similar) that would let a second instance detect \"another Omegon is already active on this repo\" and offer worktree isolation instead of shared-checkout collision?"
---

# Multi-instance Omegon coordination — parallel work streams on the same repo

## Overview

> Parent: [Directive-Branch Lifecycle — git branch as the unified task boundary](directive-branch-lifecycle.md)
> Spawned from: "Should `implement` auto-checkout the created branch, or is that too disruptive for operators who prefer to stay on main?"

*To be explored.*

## Research

### Current multi-instance state — what breaks today

Running two Omegon instances against the same repo today produces several collision domains:

**1. Git working tree (critical)**
Two instances sharing a single checkout cannot both make file edits. `git checkout`, `git add`, `git commit` are inherently serial on a single working tree. This is the hardest constraint — it's not an Omegon problem, it's a git fundamental.

**2. SQLite memory database** (.pi/memory/facts.db)
WAL mode + `busy_timeout = 10000` provides basic concurrent read access, but two instances writing facts, reinforcements, and embeddings will produce contention. SQLite WAL allows concurrent readers with one writer — but two writers will block each other with 10s timeouts.

**3. Shared state** (extensions/lib/shared-state.ts)
`globalThis[Symbol.for("pi-kit-shared-state")]` is per-process. Two Omegon processes have completely independent shared state. This is actually fine — they don't coordinate in-memory. But it means dashboard state, cleave state, and effort state are per-instance.

**4. OpenSpec artifacts** (openspec/changes/*)
Two instances both running `/opsx:propose` or `/opsx:ff` on the same change directory will clobber each other's files. No locking.

**5. Design tree documents** (docs/*.md)
Two instances both calling `design_tree_update` on the same node will race on file writes. Last writer wins.

**6. Session state** (~/.pi/agent/sessions/)
Per-session files — not a collision risk since sessions are UUID-namespaced.

**7. Cleave subprocess management** (subprocess-tracker.ts)
The PID tracking file and orphan cleanup are per-process. Two instances running cleave would create independent subprocess trees. No cross-instance visibility.

### Git worktrees as the natural isolation boundary

Git worktrees solve the hardest problem — working tree contention — and they're already proven in cleave's child dispatch:

**Each instance gets its own worktree and branch.** The primary instance stays on `main` (or the operator's chosen branch). When a directive starts, a second instance can operate in a separate worktree checked out to the directive branch. Each worktree has its own:
- Working tree (independent file edits)
- HEAD ref (independent branch)
- Index (independent staging area)

**What git worktrees share:**
- Object database (`.git/objects/`) — efficient, no duplication
- Refs (branches, tags) — both instances see all branches
- Config (`.git/config`) — shared settings

**What worktrees DON'T solve:**
- SQLite memory DB — still shared via `.pi/memory/facts.db` in the repo root. A second worktree's `.pi/` is a symlink or separate copy depending on setup. This needs explicit handling.
- OpenSpec artifacts — they live in the repo tree, so each worktree has its own copy. This is actually GOOD — each directive's artifacts live in its own worktree's `openspec/changes/`.
- Design tree docs — also in the repo tree, so each worktree has independent copies. Changes merge when branches merge.

**The model:**
```
repo/                         ← main worktree (operator's primary instance)
  .git/
  .pi/memory/facts.db         ← shared SQLite (WAL handles concurrency)
  docs/
  extensions/
  openspec/

/tmp/omegon-worktrees/
  feature-foo/                 ← worktree for directive "foo" (second instance)
    docs/
    extensions/
    openspec/changes/foo/      ← directive's artifacts isolated here
```

This is exactly how cleave already works, but at the directive level instead of the child level.

### Three operational modes for multi-instance

**Mode A: Single instance, serial directives (current default)**
One Omegon instance. `implement` creates a branch and checks it out. Work happens serially. When the directive completes, archive merges to main. Simple, no coordination needed. This is what the parent node's directive-branch lifecycle describes.

**Mode B: Primary + delegate instances via worktrees**
The primary instance stays on main. When `implement` fires, it creates a worktree + branch and spawns a delegate Omegon instance in that worktree. The delegate does the spec work, cleave, assessment. When done, the primary merges the worktree branch back to main. This is cleave's model lifted to the directive level.

Advantages: operator keeps their primary instance for ad-hoc work. Directives run in isolation. Multiple directives can run in parallel.

Disadvantages: the delegate instance needs its own terminal/session. The operator needs to monitor multiple instances. This is the Omega coordinator vision.

**Mode C: Single instance, branch-aware checkout**
One instance, but it actively manages which branch it's on based on the active directive. `implement` checks out the directive branch. Session start detects the active directive and ensures the right branch. Archive merges and switches back to main. Between directives, the instance is on main.

This is the simplest version of the parent node's proposal. It answers the parent question directly: yes, `implement` should auto-checkout, because the branch IS the directive boundary.

**Recommendation:** Mode C first (it's the foundation), Mode B later (it's the Omega scaling story). Mode A is what we have now and it doesn't work well.

## Decisions

### Decision: implement should auto-checkout the directive branch (Mode C foundation)

**Status:** exploring
**Rationale:** The branch IS the directive boundary. If `implement` creates a branch but doesn't check it out, every subsequent operation (cleave, assess, archive) must independently figure out which branch to use, and the operator drifts to main by default. Auto-checkout makes the branch the natural working context. 

For multi-instance (Mode B), the directive branch lives in a worktree — checkout is per-worktree and doesn't affect the primary instance. So auto-checkout is correct for both modes.

The escape hatch for operators who want to stay on main: they simply don't use `implement`. Direct commits to main remain valid for lightweight bug/chore/task work. The lifecycle ceremony is opt-in at the `implement` gate.

### Decision: Parallel directives use git worktrees, not shared checkout

**Status:** exploring
**Rationale:** Git's working tree is a serial resource. Two directives modifying the same checkout will produce corrupt state. Worktrees are the proven isolation mechanism — cleave already demonstrates this. The scaling path from Mode C (single instance, serial) to Mode B (multi-instance, parallel) is worktrees, not shared checkout tricks. This aligns with the Omega coordinator vision where each directive is a supervised subprocess with its own worktree.

## Open Questions

- Should `implement` auto-checkout the created branch, or is that too disruptive for operators who prefer to stay on main?
- For Mode B (worktree delegates), should the delegate instance share the same SQLite memory DB (WAL concurrency) or get its own isolated copy that merges facts back on completion?
- Is there a lightweight instance-presence signal (PID file, Unix socket, or similar) that would let a second instance detect "another Omegon is already active on this repo" and offer worktree isolation instead of shared-checkout collision?

+++
id = "f13d6a1b-e93c-4a09-a099-b3718a40783d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Git as first-class harness citizen — commit hygiene and repo-aware lifecycle

## Overview

Recurring pattern: sessions accumulate many ceremony commits (checkpoints, cleave merges, task-complete markers, OpenSpec archive commits) that add noise without value. The question is whether the fix is better directives or whether git itself needs to be a first-class internal concept in the harness rather than an external tool shelled out to.

## Research

### Symptom inventory — where the mess comes from

Examining the last 40 commits on main:

**Ceremony commits (noise):** 12 out of ~77 (15%)
- `chore(cleave): checkpoint before cleave` × 3 — dirty-tree preflight forcing a commit
- `cleave: merge cleave/N-label` × 4 — worktree branch merges
- `chore(vault-fail-closed): checkpoint ...` × 1 — same preflight
- `docs(...): mark all tasks complete` × 2 — OpenSpec task file updates
- `chore: archive ...` × 2 — OpenSpec lifecycle

**Redundant commits (work that was immediately superseded):**
- Feature branches had 20-30 commits each, many intermediate (edit, fix test, fix compilation, re-edit)
- The merge to main preserves all of these as individual commits (no squash)
- Post-review fixes created additional commits on top of already-committed work

**The merge topology is ugly:**
```
*   ff51d08 merge
|\  
| * 783f4ef fix (review findings)
| * b1fbfc6 chore (archive)
| * 5eb6c46 docs (mark tasks)
| * 8ab9d43 fix (implementation)
* | 6b94c2e merge
|\| 
| * e79c8fc docs (design)
| * 1eb24b5 docs (assessment)
```
Every feature branch brought its full edit history, including checkpoint commits from preflight.

### Root causes — why the harness produces dirty git

**1. Git is not a harness concept — it's an external tool the agent shells out to.**

The agent uses `bash` to run `git add`, `git commit`, `git stash`. The harness extensions (cleave, openspec, design-tree) shell out to git via `pi.exec("git", ...)` or `Command::new("git")`. No component has a coherent model of "what is the current repo state" — they all ask git ad-hoc and react.

This means:
- The agent and extensions make git mutations independently with no coordination
- The cleave preflight discovers dirty state reactively instead of preventing it
- OpenSpec lifecycle changes (archive, mark tasks complete) happen as commits because there's no staging concept — the only way to persist a file change is to commit it

**2. The dirty-tree preflight is a symptom, not a solution.**

The preflight exists because the agent often leaves uncommitted work when `/cleave` is invoked. It offers "checkpoint" (commit), "stash" (defer), or "cancel" as reactive options. This forces a commit that has no business existing — it's a commit that says "I wasn't done but the cleave system forced me to save."

The deeper question: why is there uncommitted work at all? Because the agent edits files (via `edit` tool) without any expectation of committing. The harness has no "working set" concept. File edits are fire-and-forget until something external (cleave, user request, session end) triggers a commit.

**3. Feature branches accumulate intermediate commits because there's no rebase/squash discipline.**

The agent commits whenever it feels like committing — after each edit, after each test fix, after each compilation failure. These are development-diary commits, not publishable units. When the branch merges to main, the full diary comes along.

The git skill defines conventional commit format but says nothing about commit granularity, when to amend vs. create new, or when to squash.

**4. Submodules are invisible to the harness model.**

The harness sees files. It doesn't know or care that `core/` is a submodule with its own commit graph. This means:
- Edits inside `core/` create a two-level dirty state that the preflight doesn't understand
- Cleave worktrees need special init that the TS layer completely lacks
- Commits require a two-step dance (commit inside submodule, then commit pointer in parent) that no harness component coordinates
- The agent can `git commit` in the parent without realizing the submodule pointer is stale

**5. OpenSpec lifecycle creates ceremony commits.**

Every lifecycle transition (mark tasks complete, archive, reconcile) writes files and expects them committed. These are bookkeeping operations that generate noise commits. The lifecycle is file-based because that's the only persistence mechanism — there's no structured state store that could track lifecycle without git.

### Option space — directive vs. structural fixes

**Option A: Better directives (behavioral)**

Tell the agent to commit less, amend more, squash before merge. Add git skill rules:
- "Amend the last commit instead of creating a new one when fixing the same logical unit"
- "Squash feature branches before merging to main"
- "Don't commit OpenSpec lifecycle files separately"

Problems: Directives are advisory. The agent forgets, gets interrupted, or has competing priorities. The ceremony commits from cleave preflight are structural (code forces them), not behavioral.

**Option B: Git-aware session state (structural, incremental)**

Add a `RepoState` concept to the harness that tracks:
- Current branch
- Dirty files (continuously, not on-demand)
- Submodule state (which submodules exist, which have dirty content)
- "Working set" — files the agent has touched since last commit

This enables:
- Automatic amend-or-new logic: if the agent edits a file it already committed, amend instead of creating a new commit
- Dirty-tree prevention: the harness knows the tree is dirty before cleave is invoked
- Submodule-aware commits: the harness coordinates the two-level commit dance
- Lifecycle ops can be folded into the next real commit instead of creating their own

**Option C: Shadow staging area (structural, deeper)**

The harness maintains its own staging concept separate from git. File edits go into a "working set" that isn't committed until a logical unit is complete. The harness decides when to commit based on:
- A feature is complete (all tasks done)
- A cleave is about to run (must checkpoint)
- The session is ending
- The operator explicitly requests a commit

This would eliminate most ceremony commits and reduce feature-branch diary commits to one per logical unit.

**Option D: Rebase-on-merge policy (structural, simple)**

Keep the current commit behavior but always squash-merge feature branches. The messy intermediate history stays on the branch (which is deleted after merge). Main gets one clean commit per feature.

This doesn't fix the ceremony commits on main (from direct commits like checkpoints) but dramatically cleans the merge topology.

**Option E: Hybrid — repo state tracking + squash merge + directive refinement**

Combine B and D:
1. `RepoState` tracks dirty state and submodules continuously
2. Cleave preflight becomes a check, not an interactive ceremony — if tree is dirty, the harness auto-stashes or auto-commits with a fixup commit
3. Feature branches squash-merge to main
4. OpenSpec lifecycle ops batch into the next real commit
5. Git skill adds amend/squash guidance
6. Submodule commits are always handled by the harness, never by the agent

### Assessment — directives can't fix this, structure can

**The directive approach is backwards.** Telling the agent "commit less" is fighting the current. The agent produces many small edits because that's how iterative coding works. The problem isn't that the agent commits too often — it's that the harness has no concept of what constitutes a publishable commit vs. working-state persistence.

The deeper issue: **git is used for two conflicting purposes simultaneously:**
1. **Working-state persistence** — saving progress so it's not lost (checkpoint commits, intermediate commits)
2. **Published history** — communicating meaningful changes to humans and other systems

These need to be separated. The harness should own working-state persistence (via git internals or its own staging). Published history should be curated — one commit per logical unit, with clean messages.

**The submodule problem is a special case of the same gap.** The harness doesn't model repo structure. It treats the file system as flat. Submodules, worktrees, branches — these are all repo-structural concepts that the harness encounters reactively rather than understanding proactively.

**What "git as first-class citizen" means concretely:**

1. **RepoModel** — a persistent object in the harness that knows: current branch, dirty files, submodule map, recent commits, worktree locations. Updated on every file mutation (the `edit`/`write` tools already go through the harness).

2. **CommitPolicy** — configurable rules for when the harness creates commits. Options: "per-logical-unit" (default), "per-edit" (current behavior), "manual-only". The agent stops running `git commit` via bash; the harness owns all commits.

3. **MergePolicy** — squash-merge by default for feature branches. The harness offers `git merge --squash` or interactive rebase at branch completion.

4. **SubmoduleModel** — part of RepoModel. The harness knows which paths are submodules, handles the two-level commit dance, and presents a unified dirty-state view.

5. **LifecycleCommitBatching** — OpenSpec and design-tree file changes are batched into the next real commit instead of creating their own. A "pending lifecycle changes" queue accumulates file writes that get folded in when the agent next commits real work.

## Decisions

### Decision: The Rust harness takes full ownership of git commits — the agent never runs git commit directly

**Status:** decided
**Rationale:** The Rust agent already intercepts every file mutation through its tool layer (edit, write, change, bash). It already has a speculate tool that uses git stash internally. The cleave orchestrator already has auto_commit_worktree and commit_dirty_submodules. The pattern is established — the agent makes file changes, the harness decides when and how to commit. Making this explicit: the agent's bash tool should not be used for git commit/add/stash (those can be intercepted or replaced with structured tools). A new `commit` tool replaces bash-based git commits with harness-controlled commits that apply the commit policy. The cleave child contract already says "commit your work" — the harness can do this automatically on child completion instead of relying on the child.

### Decision: RepoModel lives in the Rust core as a shared struct initialized at agent startup

**Status:** decided
**Rationale:** The Rust core is where all file mutations happen (tools layer) and where all git operations already live (cleave orchestrator, speculate). RepoModel should be initialized at agent startup by scanning .git, .gitmodules, and current branch. It gets updated by the edit/write/change tools (track which files were touched) and by the commit tool (reset the dirty set). The TS extensions can query it via the bridge or shared state, but Rust is the source of truth. This is also where the submodule model naturally fits — the Rust worktree.rs already has detect_submodules.

### Decision: Squash-merge is sufficient for feature branches — intermediate commit frequency is not the primary fix target

**Status:** decided
**Rationale:** The intermediate commits serve a real purpose during development: they're recovery points if the agent gets stuck or the session crashes. Trying to reduce their frequency (amend logic, commit batching) adds complexity with marginal benefit — the real problem is that they leak into main. Squash-merge on feature-branch close gives main one clean commit per feature while preserving the full diary on the branch for debugging. The cleave orchestrator already does this implicitly (it creates one merge commit per child). For interactive sessions, the harness should offer squash-merge when a feature branch is completed. Intermediate commit count on the branch is not a problem to solve.

### Decision: Lifecycle file changes queue in RepoModel and flush with the next real commit

**Status:** decided
**Rationale:** OpenSpec task-complete markers, archive operations, and design-tree status updates currently create their own commits. Instead, these file writes should be tracked in RepoModel as "pending lifecycle changes" and included in the next commit the agent makes for real work. If the session ends with unflushed lifecycle changes, the harness auto-commits them as a single "chore: lifecycle sync" commit on session close — this is the only ceremony commit that should survive. This eliminates the "mark tasks complete" and "archive" noise commits during normal flow while ensuring nothing is lost on session end.

## Open Questions

*No open questions.*

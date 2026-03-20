---
id: cleave-submodule-worktree
title: Cleave worktree submodule failures — root cause and fix
status: decided
tags: [cleave, git, submodule, reliability]
open_questions: []
---

# Cleave worktree submodule failures — root cause and fix

## Overview

Security assessment runs showed 2/4 child failures in both cleave runs. All failures were on children whose scope targeted files inside the `core` git submodule. Root cause analysis below.

## Research

### Root cause analysis — two distinct failure paths

**Failure pattern from both runs:**

| Run | Child | Status | Error |
|-----|-------|--------|-------|
| Security assessment | path-traversal-and-injection | failed | Branch has no new commits |
| Security assessment | network-and-ssrf | failed | Child exited with code 1 |
| Fail-closed | path-policy-enum | failed | Branch has no new commits |
| Fail-closed | fail-closed-auth | failed | Child exited with code -1 |

All 4 failures targeted files inside `core/` (a git submodule). Meanwhile, the 4 successes either worked on files in the outer repo (SECURITY_ASSESSMENT.md) or happened to succeed despite the submodule issue (vault-addr-and-errors, recipe-path-validation).

**The Rust orchestrator has submodule support but there are two compounding issues:**

### Issue 1: Submodule init succeeds but children still can't commit

`worktree.rs::submodule_init()` correctly runs `git submodule update --init --recursive` in the worktree. This populates `core/` with content. Children CAN read files. But when a child modifies a file inside the submodule and tries to commit, the commit happens inside the submodule's detached HEAD. The parent worktree's `core` pointer doesn't update unless `commit_dirty_submodules()` runs afterward.

`commit_dirty_submodules()` exists and IS called in the orchestrator — but only after the child process finishes (line ~237 in orchestrator.rs). If the child fails to commit (because it doesn't know it needs to `cd core && git add && git commit` first), the submodule changes are uncommitted when the orchestrator checks.

### Issue 2: "Branch has no new commits" on merge

The orchestrator runs `git merge --no-ff` which checks for divergence. If the child's only changes were inside the submodule but the parent worktree has no commits (child didn't commit the submodule pointer), the merge finds "Already up to date" and reports "Branch has no new commits — child did not produce any work".

This is a false negative — the child DID produce work (modified files inside core/), but those changes were never surfaced as parent-level commits.

### Issue 3: Code -1 = timeout/abort

`fail-closed-auth` exited with code -1, meaning it was killed by the idle timeout. This could be caused by the child getting stuck trying to read/write files in an improperly initialized submodule, or by the child spending too long navigating the submodule structure.

### The dirty-tree interaction

The dirty-tree preflight and checkpoint flow treat a dirty submodule (`m core` in porcelain output) as a single modified file. The checkpoint action commits the outer pointer, which is correct. But this creates a subtle coupling: if the submodule's HEAD doesn't match what the checkpoint committed, the worktree will be created from a parent commit that pins the submodule to a specific SHA — and the worktree's submodule init will check out that SHA, not the working copy's state.

### Code trace — where submodule support exists vs is missing

**Rust orchestrator (core/crates/omegon/src/cleave/):**
- `worktree.rs::submodule_init()` ✅ — runs `git submodule update --init --recursive`
- `worktree.rs::detect_submodules()` ✅ — parses `git submodule status`
- `worktree.rs::commit_dirty_submodules()` ✅ — stages+commits inside submodule, then commits pointer in parent
- `orchestrator.rs:133` ✅ — calls `submodule_init()` after `create_worktree()`
- `orchestrator.rs:237` ✅ — calls `commit_dirty_submodules()` after child finishes

**TS worktree layer (extensions/cleave/worktree.ts):**
- `createWorktree()` ❌ — NO submodule init. `git worktree add` creates empty submodule dirs.
- `mergeBranch()` ❌ — No submodule-aware merge handling
- `cleanupWorktrees()` ❌ — No submodule cleanup

**TS dirty-tree preflight (extensions/cleave/index.ts + lib/git-state.ts):**
- `inspectGitState()` ❌ — Treats `m core` as a regular modified file, no special submodule handling
- `classifyPreflightDirtyPaths()` ❌ — No submodule classification
- `checkpointRelatedChanges()` ❌ — Commits outer pointer but doesn't ensure submodule HEAD consistency

**TS dispatcher (extensions/cleave/dispatcher.ts):**
- `dispatchChildren()` ❌ — Uses the TS worktree path (no submodule init)
- But this code is labeled "RESUME-ONLY" — primary path goes through native dispatch

**Native dispatch (extensions/cleave/native-dispatch.ts):**
- Calls the Rust binary which DOES handle submodules ✅
- But TS index.ts creates branches before handing off — those branches may not have submodule state ❌

### Native dispatch flow — where the gap actually bites

The current `cleave_run` tool flow is:

1. **TS index.ts** creates CleaveState with branch names (`cleave/0-label`) — no worktree creation
2. **TS index.ts** writes task files to workspace
3. **TS index.ts** calls `dispatchViaNative()` which spawns the **Rust binary** with `--plan`, `--workspace`, `--cwd`
4. **Rust orchestrator** checks for existing worktrees (from TS) — if none, creates them
5. **Rust orchestrator** calls `submodule_init()` ✅
6. **Rust orchestrator** spawns child agents in worktree cwd
7. Child agent reads/edits files including submodule files
8. **Child agent commits** via `git add -A && git commit` — but this only stages files in the PARENT worktree, not inside the submodule
9. **Rust orchestrator** calls `commit_dirty_submodules()` — this should catch submodule changes the child didn't commit

The actual failure point: **step 8**. The native child agent (omegon-agent) uses `bash` tool to run `git` commands. When it does `git add -A` in the worktree root, submodule-internal changes are NOT staged — git requires explicitly entering the submodule directory first. The child's changes inside `core/` are invisible to `git add -A` at the parent level.

Then **step 9** (commit_dirty_submodules) should catch this — but if the child's exit code was already non-zero (e.g., the child tried to commit, got "nothing to commit", and errored), the orchestrator may skip the submodule commit step or the merge may have already been attempted.

**Key insight:** The child agent doesn't know it's working inside a submodule. Its task file says "edit core/crates/omegon-secrets/src/vault.rs" — the child opens the file, edits it, tries to commit, but the commit captures only the parent-level changes (none) while the submodule-level changes are orphaned.

### The compounding failure chain

Tracing the exact sequence for `path-policy-enum` (failed, "Branch has no new commits"):

1. TS creates CleaveState with `branch: "cleave/0-path-policy-enum"`, scope: `["core/crates/omegon-secrets/src/vault.rs"]`
2. Rust orchestrator creates worktree, calls `submodule_init()` — core/ is populated at the committed SHA
3. Child agent spawns in the worktree directory
4. **Child tries to read/edit `core/crates/omegon-secrets/src/vault.rs`** — file exists, edits succeed on disk
5. Child finishes (exit 0 or whatever)
6. Orchestrator calls `commit_dirty_submodules()`:
   - Checks `git status --porcelain` INSIDE `core/` — finds modified files
   - Runs `git add -A && git commit` inside `core/` — commits to submodule's detached HEAD
   - Runs `git add core` in parent — stages pointer update
   - Runs `git commit` in parent — **creates a commit on the branch** ← this should work
7. Orchestrator calls `auto_commit_worktree()` — finds remaining changes, commits
8. Merge — should find commits

**But**: "Branch has no new commits" means step 6-7 produced ZERO commits. This can only happen if the child didn't actually modify any files. The child agent may have:
- Gotten confused by the scope and done analysis without making edits
- Encountered an error (compilation failure in isolation, missing Cargo.lock) and bailed
- The native agent's path traversal protection rejected the edit somehow

For `fail-closed-auth` (code -1 = killed by idle timeout): The child probably stalled. In a worktree with a freshly-init'd submodule, running `cargo test` or `cargo check` would trigger a full rebuild from scratch (no target/ cache). A 3-minute idle timeout would kill a long compilation.

**The core issues are:**
1. **Children have no submodule context** — task files don't mention submodule structure
2. **No worktree health verification** — orchestrator doesn't confirm files are accessible after init
3. **Failed children skip submodule commit** — the `Err(e)` path in orchestrator never calls `commit_dirty_submodules()`
4. **Cargo cache not shared** — worktree submodules rebuild from scratch, hitting idle timeouts

## Decisions

### Decision: Orchestrator must own all submodule commits — children should not need submodule awareness

**Status:** decided
**Rationale:** Making children submodule-aware would mean injecting git plumbing knowledge into every child agent. The orchestrator already has commit_dirty_submodules() — it just needs to run it unconditionally (success AND failure paths) and verify after submodule_init that the files in scope are actually accessible. The child edits files normally; the orchestrator handles the two-level commit dance.

### Decision: commit_dirty_submodules must run on both success and failure paths

**Status:** decided
**Rationale:** Currently only the Ok(output) arm of the child result match calls commit_dirty_submodules. The Err(e) arm skips it entirely. A child that times out (code -1) or errors (code 1) may have made real edits inside the submodule before dying. Those edits should be preserved by the orchestrator's auto-commit, not silently lost. Even on failure, we should capture whatever work the child produced.

### Decision: Worktree health check after submodule init — verify scope files are accessible

**Status:** decided
**Rationale:** After create_worktree + submodule_init, the orchestrator should stat at least one file from the child's scope to confirm the worktree is functional. If scope files are inside a submodule and the submodule init failed silently (e.g., network error fetching submodule, .gitmodules missing), the child would spin uselessly. A health check catches this early and marks the child as failed with an actionable error message.

### Decision: Task files should declare submodule context when scope crosses a submodule boundary

**Status:** decided
**Rationale:** The task file should include a note like "Note: files in core/ are inside a git submodule. The orchestrator handles submodule commits — edit files normally." This doesn't require the child to understand git submodules, but it provides context that prevents confusion if the child runs git status and sees unexpected output. The orchestrator can detect submodules at worktree creation time and inject this context into the task file.

### Decision: Dirty-tree preflight should classify submodule paths and ensure consistency before checkpoint

**Status:** decided
**Rationale:** When git status shows ` m core` (modified submodule content), the preflight should recognize this as a submodule path and ensure the submodule's HEAD matches what will be committed in the checkpoint. Currently the preflight treats it as a regular file. The checkpoint commits the outer pointer but if the submodule has uncommitted changes inside it, the worktree will be created from a parent that pins the submodule to a SHA that doesn't include those changes. The preflight should either commit inside the submodule first or warn the operator.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — Move commit_dirty_submodules to run on BOTH success and failure paths. Add worktree health check after submodule init. Inject submodule context note into task files when scope crosses submodule boundary.
- `core/crates/omegon/src/cleave/worktree.rs` (modified) — Add verify_scope_accessible() function that stats files from scope to confirm worktree is functional after submodule init.
- `extensions/lib/git-state.ts` (modified) — Add submodule detection to inspectGitState — classify paths that are submodule roots based on .gitmodules parsing.
- `extensions/cleave/index.ts` (modified) — Dirty-tree preflight: detect submodule paths, ensure submodule HEAD consistency before checkpoint. TS worktree.ts is dead code (native dispatch owns worktrees) but add a deprecation comment.

### Constraints

- commit_dirty_submodules must run on both Ok and Err paths
- Worktree health check must verify at least one scope file is readable
- Task files must include submodule context when scope crosses submodule boundary
- TS worktree.ts is legacy — do not add submodule support there, only in Rust orchestrator

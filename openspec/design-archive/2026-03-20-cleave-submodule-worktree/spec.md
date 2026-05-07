+++
id = "7a912571-c8c0-484b-91da-d807067eeb7b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave worktree submodule failures — root cause and fix — Design Spec (extracted)

> Auto-extracted from docs/cleave-submodule-worktree.md at decide-time.

## Decisions

### Orchestrator must own all submodule commits — children should not need submodule awareness (decided)

Making children submodule-aware would mean injecting git plumbing knowledge into every child agent. The orchestrator already has commit_dirty_submodules() — it just needs to run it unconditionally (success AND failure paths) and verify after submodule_init that the files in scope are actually accessible. The child edits files normally; the orchestrator handles the two-level commit dance.

### commit_dirty_submodules must run on both success and failure paths (decided)

Currently only the Ok(output) arm of the child result match calls commit_dirty_submodules. The Err(e) arm skips it entirely. A child that times out (code -1) or errors (code 1) may have made real edits inside the submodule before dying. Those edits should be preserved by the orchestrator's auto-commit, not silently lost. Even on failure, we should capture whatever work the child produced.

### Worktree health check after submodule init — verify scope files are accessible (decided)

After create_worktree + submodule_init, the orchestrator should stat at least one file from the child's scope to confirm the worktree is functional. If scope files are inside a submodule and the submodule init failed silently (e.g., network error fetching submodule, .gitmodules missing), the child would spin uselessly. A health check catches this early and marks the child as failed with an actionable error message.

### Task files should declare submodule context when scope crosses a submodule boundary (decided)

The task file should include a note like "Note: files in core/ are inside a git submodule. The orchestrator handles submodule commits — edit files normally." This doesn't require the child to understand git submodules, but it provides context that prevents confusion if the child runs git status and sees unexpected output. The orchestrator can detect submodules at worktree creation time and inject this context into the task file.

### Dirty-tree preflight should classify submodule paths and ensure consistency before checkpoint (decided)

When git status shows ` m core` (modified submodule content), the preflight should recognize this as a submodule path and ensure the submodule's HEAD matches what will be committed in the checkpoint. Currently the preflight treats it as a regular file. The checkpoint commits the outer pointer but if the submodule has uncommitted changes inside it, the worktree will be created from a parent that pins the submodule to a SHA that doesn't include those changes. The preflight should either commit inside the submodule first or warn the operator.

## Research Summary

### Root cause analysis — two distinct failure paths

**Failure pattern from both runs:**

| Run | Child | Status | Error |
|-----|-------|--------|-------|
| Security assessment | path-traversal-and-injection | failed | Branch has no new commits |
| Security assessment | network-and-ssrf | failed | Child exited with code 1 |
| Fail-closed | path-policy-enum | failed | Branch has no new commits |
| Fail-closed | fail-closed-auth | failed | Child exited with code -1 |

All 4 failures targeted files inside `core/` (a git submodule). Meanwhile, the 4 …

### Issue 1: Submodule init succeeds but children still can't commit

`worktree.rs::submodule_init()` correctly runs `git submodule update --init --recursive` in the worktree. This populates `core/` with content. Children CAN read files. But when a child modifies a file inside the submodule and tries to commit, the commit happens inside the submodule's detached HEAD. The parent worktree's `core` pointer doesn't update unless `commit_dirty_submodules()` runs afterward.

`commit_dirty_submodules()` exists and IS called in the orchestrator — but only after the child …

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

**TS worktree …

### Native dispatch flow — where the gap actually bites

The current `cleave_run` tool flow is:

1. **TS index.ts** creates CleaveState with branch names (`cleave/0-label`) — no worktree creation
2. **TS index.ts** writes task files to workspace
3. **TS index.ts** calls `dispatchViaNative()` which spawns the **Rust binary** with `--plan`, `--workspace`, `--cwd`
4. **Rust orchestrator** checks for existing worktrees (from TS) — if none, creates them
5. **Rust orchestrator** calls `submodule_init()` ✅
6. **Rust orchestrator** spawns child agents in work…

### The compounding failure chain

Tracing the exact sequence for `path-policy-enum` (failed, "Branch has no new commits"):

1. TS creates CleaveState with `branch: "cleave/0-path-policy-enum"`, scope: `["core/crates/omegon-secrets/src/vault.rs"]`
2. Rust orchestrator creates worktree, calls `submodule_init()` — core/ is populated at the committed SHA
3. Child agent spawns in the worktree directory
4. **Child tries to read/edit `core/crates/omegon-secrets/src/vault.rs`** — file exists, edits succeed on disk
5. Child finishes (exi…

+++
id = "1ad09d16-fc61-4d74-a962-cad77e2d1fe0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave worktree submodule failures — root cause and fix — Tasks

## 1. Rust orchestrator: submodule commits on both paths (orchestrator.rs)
<!-- specs: cleave/submodule -->

- [x] 1.1 Extract `salvage_worktree_changes()` helper — runs commit_dirty_submodules then auto_commit_worktree
- [x] 1.2 Call `salvage_worktree_changes` from BOTH Ok and Err match arms
- [x] 1.3 On failure path, log at warn level before salvage attempt

## 2. Rust worktree: scope health check (worktree.rs)
<!-- specs: cleave/submodule -->

- [x] 2.1 Add `verify_scope_accessible(worktree_path, scope) -> Vec<String>` — stats each scope file
- [x] 2.2 Empty scope returns empty vec (vacuous pass)
- [x] 2.3 Call from orchestrator after submodule_init — mark child failed if scope files missing
- [x] 2.4 Tests: existing file passes, missing file+parent fails, empty scope passes, existing parent OK

## 3. Rust orchestrator: submodule context in task files (orchestrator.rs)
<!-- specs: cleave/submodule -->

- [x] 3.1 After submodule_init, call `detect_submodules()` via `build_submodule_context()`
- [x] 3.2 Check if any scope file starts with a submodule path prefix
- [x] 3.3 Inject "## Submodule Context" section into task file when scope crosses boundary
- [x] 3.4 Includes warning not to run cargo build/test in uncached worktree

## 4. TS dirty-tree preflight: submodule classification (git-state.ts + index.ts)
<!-- specs: cleave/submodule -->

- [x] 4.1 Add `parseGitmodules(repoPath) -> Set<string>` to git-state.ts
- [x] 4.2 Extend `GitStatusEntry` with `submodule: boolean` field
- [x] 4.3 `inspectGitState()` cross-references entries against submodule paths
- [x] 4.4 `checkpointRelatedChanges()` warns about submodule HEAD consistency
- [x] 4.5 Add deprecation comment to TS worktree.ts
- [x] 4.6 Tests: 6 tests covering submodule classification and parseGitmodules

## Cross-cutting constraints

- [x] commit_dirty_submodules runs on both Ok and Err paths
- [x] Worktree health check verifies at least one scope file is readable
- [x] Task files include submodule context when scope crosses submodule boundary
- [x] TS worktree.ts is legacy — no submodule support added there

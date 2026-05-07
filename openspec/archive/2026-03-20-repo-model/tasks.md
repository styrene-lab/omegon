+++
id = "50897274-d6c8-45d0-a35b-791136bc5850"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# RepoModel — git state tracking in Rust core — Tasks

## 1. omegon-git crate foundation
- [x] 1.1 New crate with git2 + jj-lib deps
- [x] 1.2 Crate root re-exports repo, status, commit, submodule, worktree, jj, merge modules

## 2. RepoModel (repo.rs)
- [x] 2.1 Discovery, branch, HEAD, submodule map, jj co-location detection
- [x] 2.2 Working set tracking (manual for git-only, jj diff for co-located)
- [x] 2.3 Lifecycle file classification and batching
- [x] 2.4 Poison-tolerant RwLock throughout

## 3. Operations
- [x] 3.1 Status queries via git2 with submodule detection
- [x] 3.2 Commit with submodule two-level dance + jj describe/new path
- [x] 3.3 Squash-merge + cleanup-merge + no-ff merge
- [x] 3.4 Worktree: git worktree + jj workspace smart dispatch
- [x] 3.5 Submodule: init, list, dirty-check

## 4. Agent integration
- [x] 4.1 RepoModel initialized in setup.rs, passed as Arc to CoreTools
- [x] 4.2 edit/write/change tools call record_edit on success
- [x] 4.3 Structured commit tool (jj path + git path)
- [x] 4.4 Bash tool warns on git mutation commands
- [x] 4.5 Cleave orchestrator uses squash-merge and jj workspaces

## 5. jj integration
- [x] 5.1 jj.rs module: detection, CLI wrappers, jj-lib load_repo
- [x] 5.2 Design tree nodes capture jj_change_id
- [x] 5.3 Memory facts capture jj_change_id
- [x] 5.4 Episodes capture jj_change_id
- [x] 5.5 Standardized 32-char change_id format across all sites

## Cross-cutting constraints
- [x] git2 primary, CLI for gaps, jj when co-located
- [x] RepoModel is Send + Sync (Arc<RwLock>)
- [x] Squash-merge default for cleave children

//! Git operations for Omegon.
//!
//! Provides a `RepoModel` that tracks repository state (branch, dirty files,
//! submodules, working set) and operations (commit, merge, worktree, stash)
//! backed by `git2` (libgit2 bindings).
//!
//! The harness owns all git mutations — the agent uses structured tools
//! instead of shelling out to `git` via bash.

pub mod commit;
pub mod merge;
pub mod repo;
pub mod status;
pub mod submodule;
pub mod worktree;

pub use repo::RepoModel;

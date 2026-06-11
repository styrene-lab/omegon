//! Worktree/workspace operations — create, remove, list.
//!
//! When jj is co-located, uses `jj workspace` (lock-free, no submodule
//! init needed, shared repo objects). Falls back to git worktrees when
//! jj is not available.

use anyhow::{Context, Result};
use git2::Repository;
use std::path::{Path, PathBuf};

/// Info about a created worktree/workspace.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    /// "jj" or "git" — which backend created it.
    pub backend: &'static str,
}

// ── jj workspace operations ─────────────────────────────────────────────

/// Create a jj workspace for a child task.
///
/// `jj workspace add` creates a new workspace sharing the same repo.
/// All files are immediately available — no submodule init, no clone.
/// The workspace gets its own working copy change on top of the
/// specified parent revision (defaults to current working copy parent).
pub fn create_jj_workspace(
    repo_path: &Path,
    workspace_path: &Path,
    name: &str,
) -> Result<WorktreeInfo> {
    // Remove stale workspace dir
    if workspace_path.exists() {
        let _ = std::fs::remove_dir_all(workspace_path);
    }

    // Forget stale workspace registration
    let _ = std::process::Command::new("jj")
        .args(["workspace", "forget", name])
        .current_dir(repo_path)
        .output();

    let output = std::process::Command::new("jj")
        .args([
            "workspace",
            "add",
            &workspace_path.to_string_lossy(),
            "--name",
            name,
            "-r",
            "@-", // Parent of current working copy — same base as other children
        ])
        .current_dir(repo_path)
        .output()
        .context("jj workspace add failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("jj workspace add failed: {}", stderr.trim());
    }

    tracing::info!(name = name, path = %workspace_path.display(), "jj workspace created");

    Ok(WorktreeInfo {
        path: workspace_path.to_path_buf(),
        branch: name.to_string(),
        backend: "jj",
    })
}

/// Remove a jj workspace.
pub fn remove_jj_workspace(repo_path: &Path, name: &str, workspace_path: &Path) -> Result<()> {
    let _ = std::process::Command::new("jj")
        .args(["workspace", "forget", name])
        .current_dir(repo_path)
        .output();

    // Remove the directory
    if workspace_path.exists() {
        let _ = std::fs::remove_dir_all(workspace_path);
    }

    Ok(())
}

/// List jj workspaces.
pub fn list_jj_workspaces(repo_path: &Path) -> Result<Vec<String>> {
    let output = std::process::Command::new("jj")
        .args(["workspace", "list"])
        .current_dir(repo_path)
        .output()
        .context("jj workspace list failed")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .filter_map(|line| {
                // Format: "name: change_id sha (description)"
                line.split(':').next().map(|s| s.trim().to_string())
            })
            .filter(|s| !s.is_empty())
            .collect())
    } else {
        Ok(vec![])
    }
}

// ── Smart dispatch ──────────────────────────────────────────────────────

/// Create a worktree/workspace — auto-selects jj or git based on availability.
pub fn create_smart(
    repo_path: &Path,
    workspace_path: &Path,
    name: &str,
    _branch: &str,
) -> Result<WorktreeInfo> {
    if crate::jj::is_jj_repo(repo_path) {
        create_jj_workspace(repo_path, workspace_path, name)
    } else {
        create(repo_path, workspace_path, _branch)
    }
}

/// Remove a worktree/workspace — auto-selects jj or git.
pub fn remove_smart(repo_path: &Path, name: &str, workspace_path: &Path) -> Result<()> {
    if crate::jj::is_jj_repo(repo_path) {
        remove_jj_workspace(repo_path, name, workspace_path)
    } else {
        remove(repo_path, workspace_path)
    }
}

// ── git worktree operations (fallback) ──────────────────────────────────
//
// All operations use libgit2 natively — no git CLI subprocess.

/// Create a git worktree with a new branch from HEAD.
pub fn create(repo_path: &Path, worktree_path: &Path, branch: &str) -> Result<WorktreeInfo> {
    let repo = Repository::open(repo_path)?;

    // Delete stale branch if it exists (equivalent to `git branch -D branch`)
    if let Ok(mut branch_ref) = repo.find_branch(branch, git2::BranchType::Local) {
        let _ = branch_ref.delete();
    }

    // Clean up stale worktree directory
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }

    // Create the branch from HEAD before adding the worktree. libgit2's
    // worktree add expects `reference` to name an existing ref; unlike
    // `git worktree add -b`, it does not create the branch for us.
    let head = repo.head().context("failed to read HEAD")?;
    let head_commit = head
        .peel_to_commit()
        .context("failed to resolve HEAD commit")?;
    repo.branch(branch, &head_commit, true)
        .with_context(|| format!("failed to create branch {branch}"))?;
    let branch_ref = repo
        .find_branch(branch, git2::BranchType::Local)
        .with_context(|| format!("failed to find created branch {branch}"))?
        .into_reference();
    let mut opts = git2::WorktreeAddOptions::new();
    opts.reference(Some(&branch_ref));

    let worktree_name = branch.replace('/', "-");
    repo.worktree(&worktree_name, worktree_path, Some(&opts))
        .with_context(|| format!("failed to create worktree at {}", worktree_path.display()))?;

    Ok(WorktreeInfo {
        path: worktree_path.to_path_buf(),
        branch: branch.to_string(),
        backend: "git",
    })
}

/// Remove a git worktree.
pub fn remove(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path)?;

    // Find the worktree by matching its path against registered worktrees
    let worktree_names = repo.worktrees().context("failed to list worktrees")?;
    for name in worktree_names.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name)
            && (wt.path() == worktree_path || worktree_path.starts_with(wt.path()))
        {
            // Prune the worktree reference (with force flags)
            let mut opts = git2::WorktreePruneOptions::new();
            opts.valid(true);
            opts.working_tree(true);
            let _ = wt.prune(Some(&mut opts));
            break;
        }
    }

    // Remove the working directory
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }
    Ok(())
}

/// List git worktrees.
pub fn list(repo_path: &Path) -> Result<Vec<String>> {
    let repo = Repository::open(repo_path)?;
    let worktrees = repo.worktrees().context("failed to list worktrees")?;
    Ok(worktrees
        .iter()
        .filter_map(|w| w.map(String::from))
        .collect())
}

/// Delete a git branch.
pub fn delete_branch(repo_path: &Path, branch: &str) -> Result<()> {
    let repo = Repository::open(repo_path)?;
    if let Ok(mut branch_ref) = repo.find_branch(branch, git2::BranchType::Local) {
        branch_ref.delete().context("failed to delete branch")?;
    }
    Ok(())
}

/// Prune stale git worktree references.
pub fn prune(repo_path: &Path) -> Result<()> {
    let repo = Repository::open(repo_path)?;
    let Ok(worktree_names) = repo.worktrees() else {
        return Ok(());
    };
    for name in worktree_names.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name) {
            let mut opts = git2::WorktreePruneOptions::new();
            // Only prune if the worktree is no longer valid (directory gone, etc.)
            if wt.is_prunable(Some(&mut opts)).unwrap_or(false) {
                let _ = wt.prune(Some(&mut opts));
            }
        }
    }
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_worktrees_in_repo() {
        let cwd = std::env::current_dir().unwrap();
        if let Ok(repo) = Repository::discover(&cwd) {
            let repo_root = repo.workdir().unwrap_or_else(|| repo.path());
            let wts = list(repo_root);
            assert!(wts.is_ok());
        }
    }

    #[test]
    fn jj_workspace_lifecycle() {
        let cwd = std::env::current_dir().unwrap();
        // Find the repo root
        let mut repo_path = cwd.as_path();
        loop {
            if repo_path.join(".jj").exists() {
                break;
            }
            match repo_path.parent() {
                Some(p) => repo_path = p,
                None => return, // Not in a jj repo
            }
        }

        let ws_dir = tempfile::tempdir().unwrap();
        let ws_path = ws_dir.path().join("test-child");
        let name = format!("test-ws-{}", std::process::id());

        // Create
        let result = create_jj_workspace(repo_path, &ws_path, &name);
        assert!(result.is_ok(), "create failed: {:?}", result.err());
        let info = result.unwrap();
        assert!(info.path.exists());
        assert_eq!(info.backend, "jj");

        // Verify files are accessible (no submodule init needed!)
        assert!(ws_path.join("core").exists(), "core/ should exist");
        assert!(
            ws_path.join("core/crates/omegon-git/src/lib.rs").exists(),
            "Rust source should be accessible"
        );

        // List
        let workspaces = list_jj_workspaces(repo_path).unwrap();
        assert!(workspaces.contains(&name), "workspace should be listed");

        // Remove
        remove_jj_workspace(repo_path, &name, &ws_path).unwrap();
        assert!(!ws_path.exists(), "workspace dir should be removed");
    }

    #[test]
    fn smart_dispatch_picks_jj() {
        let cwd = std::env::current_dir().unwrap();
        let mut repo_path = cwd.as_path();
        loop {
            if repo_path.join(".jj").exists() {
                break;
            }
            match repo_path.parent() {
                Some(p) => repo_path = p,
                None => return,
            }
        }

        let ws_dir = tempfile::tempdir().unwrap();
        let ws_path = ws_dir.path().join("smart-child");
        let name = format!("smart-{}", std::process::id());

        let result = create_smart(repo_path, &ws_path, &name, "unused-branch");
        assert!(result.is_ok(), "smart create failed: {:?}", result.err());
        let info = result.unwrap();
        assert_eq!(info.backend, "jj", "should use jj when co-located");

        // Cleanup
        remove_smart(repo_path, &name, &ws_path).unwrap();
    }
}

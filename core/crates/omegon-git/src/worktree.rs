//! Worktree operations — create, remove, list.
//!
//! Uses CLI for create (git2 worktree API is limited for branch creation)
//! and git2 for list/query.

use anyhow::{Context, Result};
use git2::Repository;
use std::path::{Path, PathBuf};

/// Info about a created worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
}

/// Create a worktree with a new branch from the current HEAD.
///
/// Uses CLI because git2's worktree API doesn't support `-b` (create branch).
pub fn create(
    repo_path: &Path,
    worktree_path: &Path,
    branch: &str,
) -> Result<WorktreeInfo> {
    // Clean up stale branch if it exists
    let _ = std::process::Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(repo_path)
        .output();

    // Remove stale worktree dir
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }

    let output = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            branch,
            &worktree_path.to_string_lossy(),
            "HEAD",
        ])
        .current_dir(repo_path)
        .output()
        .context("git worktree add failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("already exists") {
            anyhow::bail!("git worktree add failed: {}", stderr.trim());
        }
    }

    Ok(WorktreeInfo {
        path: worktree_path.to_path_buf(),
        branch: branch.to_string(),
    })
}

/// Remove a worktree.
pub fn remove(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_path.to_string_lossy(),
        ])
        .current_dir(repo_path)
        .output()
        .context("git worktree remove failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("worktree remove: {}", stderr.trim());
    }

    Ok(())
}

/// List active worktrees via git2.
pub fn list(repo_path: &Path) -> Result<Vec<String>> {
    let repo = Repository::open(repo_path)?;
    let worktrees = repo.worktrees().context("failed to list worktrees")?;
    Ok(worktrees.iter().filter_map(|w| w.map(String::from)).collect())
}

/// Delete a branch (after worktree removal and merge).
pub fn delete_branch(repo_path: &Path, branch: &str) -> Result<()> {
    let repo = Repository::open(repo_path)?;
    if let Ok(mut branch_ref) = repo.find_branch(branch, git2::BranchType::Local) {
        branch_ref.delete().context("failed to delete branch")?;
    }
    Ok(())
}

/// Prune stale worktree references.
pub fn prune(repo_path: &Path) -> Result<()> {
    let _ = std::process::Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_path)
        .output();
    Ok(())
}

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
}

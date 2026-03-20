//! Submodule operations via git2.

use anyhow::{Context, Result};
use git2::Repository;
use std::path::Path;

/// Initialize and update submodules in a worktree or repo.
///
/// Equivalent to `git submodule update --init --recursive`.
/// Uses git2 for init and CLI fallback for recursive update
/// (git2's submodule update is limited).
pub fn init_submodules(repo_path: &Path) -> Result<usize> {
    let repo = Repository::open(repo_path).context("failed to open repo")?;
    let submodules = repo.submodules().context("failed to list submodules")?;

    if submodules.is_empty() {
        return Ok(0);
    }

    // git2's submodule update is incomplete for recursive init.
    // Fall back to CLI which handles nested submodules reliably.
    let output = std::process::Command::new("git")
        .args(["submodule", "update", "--init", "--recursive"])
        .current_dir(repo_path)
        .output()
        .context("git submodule update --init --recursive failed")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("submodule init warning: {}", stderr.trim());
    }

    Ok(submodules.len())
}

/// List submodule paths in a repository.
pub fn list_submodule_paths(repo_path: &Path) -> Result<Vec<String>> {
    let repo = Repository::open(repo_path).context("failed to open repo")?;
    let submodules = repo.submodules().unwrap_or_default();
    Ok(submodules
        .iter()
        .map(|s| s.path().to_string_lossy().to_string())
        .collect())
}

/// Check if a submodule has dirty content (uncommitted changes inside it).
pub fn is_submodule_dirty(repo_path: &Path, submodule_path: &str) -> Result<bool> {
    let sub_full = repo_path.join(submodule_path);
    if !sub_full.exists() {
        return Ok(false);
    }

    let sub_repo = match Repository::open(&sub_full) {
        Ok(r) => r,
        Err(_) => return Ok(false), // Not initialized
    };

    let statuses = match sub_repo.statuses(None) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    Ok(!statuses.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_submodules_in_current_repo() {
        let cwd = std::env::current_dir().unwrap();
        if let Ok(repo) = Repository::discover(&cwd) {
            let repo_root = repo.workdir().unwrap_or_else(|| repo.path());
            let paths = list_submodule_paths(repo_root);
            assert!(paths.is_ok());
        }
    }
}

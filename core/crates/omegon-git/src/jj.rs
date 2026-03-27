//! Jujutsu integration — jj-lib backed operations.
//!
//! In co-located mode, jj and git share the same .git directory.
//! jj adds .jj/ for its own state (operation log, change tracking).
//!
//! Strategy: use jj CLI for user-facing operations (reliable, version-matched
//! with the installed jj), use jj-lib for read-only queries where the library
//! API is more efficient than spawning a process.
//!
//! jj-lib's API is pre-1.0 and changes frequently. The CLI is the stable
//! contract — we use the library only where it provides clear value
//! (workspace loading, commit graph queries) and fall back to CLI for
//! mutations (new, describe, squash, bookmark).

use anyhow::{Context, Result};
use std::path::Path;

// ── Detection ───────────────────────────────────────────────────────────

/// Check if a directory has jj initialized (co-located mode).
pub fn is_jj_repo(repo_path: &Path) -> bool {
    repo_path.join(".jj").exists()
}

// ── Read-only queries via jj-lib (optional feature) ─────────────────────

#[cfg(feature = "jj-lib")]
mod jj_lib_queries {
    use super::*;
    use jj_lib::object_id::ObjectId as _;
    use jj_lib::repo::Repo as _;

    /// Load the jj workspace and repo at the current operation head.
    pub async fn load_repo(
        repo_path: &Path,
    ) -> Result<(
        jj_lib::workspace::Workspace,
        std::sync::Arc<jj_lib::repo::ReadonlyRepo>,
    )> {
        let config = jj_lib::config::StackedConfig::with_defaults();
        let settings = jj_lib::settings::UserSettings::from_config(config)
            .context("failed to create jj settings")?;

        let workspace = jj_lib::workspace::Workspace::load(
            &settings,
            repo_path,
            &jj_lib::repo::StoreFactories::default(),
            &jj_lib::workspace::default_working_copy_factories(),
        )
        .context("failed to load jj workspace")?;

        let repo = workspace
            .repo_loader()
            .load_at_head()
            .await
            .context("failed to load jj repo at head")?;

        Ok((workspace, repo))
    }

    /// Get the change ID of the current working copy via jj-lib.
    pub async fn working_copy_change_id(repo_path: &Path) -> Result<Option<String>> {
        if !is_jj_repo(repo_path) {
            return Ok(None);
        }

        let (workspace, repo) = load_repo(repo_path).await?;
        let wc_id = repo.view().get_wc_commit_id(workspace.workspace_name());

        match wc_id {
            Some(commit_id) => {
                let commit = repo.store().get_commit(commit_id)?;
                Ok(Some(commit.change_id().reverse_hex()))
            }
            None => Ok(None),
        }
    }
}

#[cfg(feature = "jj-lib")]
pub use jj_lib_queries::*;

// ── Mutations via jj CLI ────────────────────────────────────────────────
//
// We use CLI for mutations because:
// 1. jj-lib's mutation API requires careful transaction handling
// 2. The CLI handles co-located git sync automatically
// 3. The CLI is the stable contract (library API breaks between versions)

/// Create a new jj change (like `jj new`).
///
/// "Commits" the current working copy and starts a new mutable change.
/// No dirty tree concept — the working copy becomes immutable.
pub fn new_change(repo_path: &Path, description: &str) -> Result<()> {
    run_jj(repo_path, &["new", "-m", description])
}

/// Describe the current working copy change.
pub fn describe(repo_path: &Path, description: &str) -> Result<()> {
    run_jj(repo_path, &["describe", "-m", description])
}

/// Squash the current change into its parent.
pub fn squash(repo_path: &Path) -> Result<()> {
    run_jj(repo_path, &["squash"])
}

/// Set a bookmark (branch) to a specific revision.
pub fn bookmark_set(repo_path: &Path, name: &str, revision: &str) -> Result<()> {
    run_jj(repo_path, &["bookmark", "set", name, "-r", revision])
}

/// Get changed files in the working copy.
pub fn diff_summary(repo_path: &Path) -> Result<Vec<String>> {
    if !is_jj_repo(repo_path) {
        return Ok(vec![]);
    }

    let output = std::process::Command::new("jj")
        .args(["diff", "--summary", "-r", "@"])
        .current_dir(repo_path)
        .output()
        .context("jj diff failed")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                // Format: "M path" or "A path" or "D path"
                trimmed.split_once(' ').map(|(_, path)| path.to_string())
            })
            .collect())
    } else {
        Ok(vec![])
    }
}

/// Squash the current jj workspace changes into the parent revision with a description.
/// Returns true if there were changes to integrate, false if the workspace was empty.
pub fn squash_working_copy_into_parent(repo_path: &Path, message: &str) -> Result<bool> {
    if !is_jj_repo(repo_path) {
        anyhow::bail!("not a jj repo")
    }

    let changed = diff_summary(repo_path)?;
    if changed.is_empty() {
        return Ok(false);
    }

    run_jj(repo_path, &["squash", "--into", "@-", "-m", message])?;
    Ok(true)
}

// ── Internal helper ─────────────────────────────────────────────────────

fn run_jj(repo_path: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("jj")
        .args(args)
        .current_dir(repo_path)
        .output()
        .with_context(|| format!("jj {} failed to execute", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("jj {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_jj_repo() {
        let cwd = std::env::current_dir().unwrap();
        let mut path = cwd.as_path();
        loop {
            if path.join(".jj").exists() {
                assert!(is_jj_repo(path));
                return;
            }
            if path.join(".git").exists() {
                let _ = is_jj_repo(path);
                return;
            }
            match path.parent() {
                Some(p) => path = p,
                None => break,
            }
        }
    }

    #[test]
    fn not_jj_outside_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_jj_repo(dir.path()));
    }

    #[cfg(feature = "jj-lib")]
    #[tokio::test]
    async fn load_repo_in_jj_workspace() {
        let cwd = std::env::current_dir().unwrap();
        let mut path = cwd.as_path();
        loop {
            if path.join(".jj").exists() {
                let result = load_repo(path).await;
                assert!(result.is_ok(), "load_repo failed: {:?}", result.err());
                let (_ws, repo) = result.unwrap();
                // Verify we can read the view
                let _ = repo.view();
                return;
            }
            match path.parent() {
                Some(p) => path = p,
                None => break,
            }
        }
        // Not in a jj repo — skip
    }
}

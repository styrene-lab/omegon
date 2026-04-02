//! Git worktree management for cleave children.
//!
//! Delegates to `omegon_git` for git operations. This module is a thin
//! adapter that maps cleave-specific conventions (branch naming, workspace
//! layout, child labels) onto the generic git API.

use anyhow::Result;
use std::path::{Path, PathBuf};

// ── Worktree lifecycle ──────────────────────────────────────────────────

/// Create a worktree/workspace for a child.
///
/// Native cleave currently relies on git-branch-based merge semantics: each
/// child is created on a named branch and later squash-merged by that branch
/// name. jj workspaces do not satisfy that contract yet because they create
/// workspace-local changes without a corresponding git branch for merge.
///
/// Until cleave grows a jj-aware harvest/merge path, always use a git worktree
/// here, even in co-located jj repos.
pub fn create_worktree(
    repo_path: &Path,
    workspace_path: &Path,
    child_id: usize,
    label: &str,
    branch: &str,
) -> Result<PathBuf> {
    let worktree_dir = workspace_path.join(format!("{}-wt-{}", child_id, label));
    omegon_git::worktree::create(repo_path, &worktree_dir, branch)?;
    Ok(worktree_dir)
}

/// Remove a child worktree.
///
/// This matches `create_worktree`: cleave children always use git worktrees,
/// so cleanup should remove the git worktree directly.
pub fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    omegon_git::worktree::remove(repo_path, worktree_path)
}

/// Delete a child branch after merge.
///
/// Cleave children are always created on git branches, even in co-located jj
/// repos, so the branch should always be removed through git.
pub fn delete_branch(repo_path: &Path, branch: &str) -> Result<()> {
    omegon_git::worktree::delete_branch(repo_path, branch)
}

// ── Merge ───────────────────────────────────────────────────────────────

/// Merge result — kept as a separate enum from omegon_git's to avoid
/// changing all orchestrator call sites at once.
#[derive(Debug)]
pub enum MergeResult {
    Success,
    NoChanges,
    Conflict(String),
    Failed(String),
}

/// Squash-merge a child's branch into the current HEAD.
///
/// All diary commits on the child branch are compressed into one commit.
/// This is the default for cleave children — their intermediate commit
/// history has no bisect/revert value.
pub fn squash_merge_branch(repo_path: &Path, branch: &str, message: &str) -> Result<MergeResult> {
    match omegon_git::merge::squash_merge(repo_path, branch, message)? {
        omegon_git::merge::MergeResult::Success { .. } => Ok(MergeResult::Success),
        omegon_git::merge::MergeResult::NoChanges => Ok(MergeResult::NoChanges),
        omegon_git::merge::MergeResult::Conflict { files } => {
            Ok(MergeResult::Conflict(files.join(", ")))
        }
        omegon_git::merge::MergeResult::Failed(detail) => Ok(MergeResult::Failed(detail)),
    }
}

/// Legacy no-ff merge (kept for backward compatibility and fallback).
pub fn merge_branch(repo_path: &Path, branch: &str) -> Result<MergeResult> {
    let message = format!("cleave: merge {}", branch);
    match omegon_git::merge::merge_no_ff(repo_path, branch, &message)? {
        omegon_git::merge::MergeResult::Success { .. } => Ok(MergeResult::Success),
        omegon_git::merge::MergeResult::NoChanges => Ok(MergeResult::NoChanges),
        omegon_git::merge::MergeResult::Conflict { files } => {
            Ok(MergeResult::Conflict(files.join(", ")))
        }
        omegon_git::merge::MergeResult::Failed(detail) => Ok(MergeResult::Failed(detail)),
    }
}

// ── Submodule operations ────────────────────────────────────────────────

/// Initialize submodules in a worktree.
///
/// No-op when jj is active — jj workspaces share the full repo tree,
/// no submodule init needed. Also no-op in a monorepo with no submodules.
pub fn submodule_init(worktree_path: &Path) -> Result<()> {
    // Skip if jj co-located (workspaces have everything already)
    if omegon_git::jj::is_jj_repo(worktree_path) {
        return Ok(());
    }
    // Skip if no submodules detected
    let subs = omegon_git::submodule::list_submodule_paths(worktree_path).unwrap_or_default();
    if subs.is_empty() {
        return Ok(());
    }
    omegon_git::submodule::init_submodules(worktree_path)?;
    Ok(())
}

/// Detect active submodules in a repo/worktree.
pub fn detect_submodules(repo_path: &Path) -> Vec<(String, PathBuf)> {
    omegon_git::submodule::list_submodule_paths(repo_path)
        .unwrap_or_default()
        .into_iter()
        .map(|path| {
            let full = repo_path.join(&path);
            (path, full)
        })
        .collect()
}

/// Commit dirty submodules in a worktree after a child finishes.
///
/// Uses `omegon_git::commit::commit_in_submodule` for each dirty submodule,
/// then commits the pointer updates in the parent.
pub fn commit_dirty_submodules(worktree_path: &Path, child_label: &str) -> Result<usize> {
    let submodules = detect_submodules(worktree_path);
    if submodules.is_empty() {
        return Ok(0);
    }

    let mut committed = 0;
    for (name, _sub_path) in &submodules {
        let msg = format!("feat({child_label}): auto-commit from cleave child");
        match omegon_git::commit::commit_in_submodule(worktree_path, name, &msg) {
            Ok(n) if n > 0 => {
                committed += 1;
                tracing::info!(
                    child = %child_label,
                    submodule = %name,
                    files = n,
                    "auto-committed dirty submodule"
                );
            }
            Err(e) => {
                tracing::warn!(
                    child = %child_label,
                    submodule = %name,
                    "submodule commit failed: {e}"
                );
            }
            _ => {} // clean, nothing to do
        }
    }

    // Commit the pointer updates in the parent — stage only submodule paths,
    // not everything (avoids sweeping in unrelated dirty files).
    if committed > 0 {
        let sub_path_strings: Vec<String> =
            submodules.iter().map(|(name, _)| name.clone()).collect();
        let msg = format!("chore({child_label}): update submodule pointer(s)");
        if let Err(e) = omegon_git::commit::create_commit(
            worktree_path,
            &omegon_git::commit::CommitOptions {
                message: &msg,
                paths: &sub_path_strings,
                include_lifecycle: false,
                lifecycle_paths: &[],
            },
        ) {
            tracing::warn!(child = %child_label, "submodule pointer commit failed: {e}");
        } else {
            tracing::info!(
                child = %child_label,
                submodules_committed = committed,
                "submodule auto-commit complete"
            );
        }
    }

    Ok(committed)
}

// ── Scope verification ──────────────────────────────────────────────────

/// Verify that scope files are accessible in a worktree after submodule init.
pub fn verify_scope_accessible(worktree_path: &Path, scope: &[String]) -> Vec<String> {
    let mut missing = Vec::new();
    for path_str in scope {
        let full_path = worktree_path.join(path_str);
        if !full_path.exists() {
            let parent = full_path.parent();
            if parent.is_none() || !parent.unwrap().exists() {
                missing.push(path_str.clone());
            }
        }
    }
    missing
}

/// Build a submodule context note for a task file.
pub fn build_submodule_context(worktree_path: &Path, scope: &[String]) -> Option<String> {
    let submodules = detect_submodules(worktree_path);
    build_submodule_context_from_list(&submodules, scope)
}

/// Inner function that takes an explicit submodule list — testable without git.
fn build_submodule_context_from_list(
    submodules: &[(String, PathBuf)],
    scope: &[String],
) -> Option<String> {
    if submodules.is_empty() {
        return None;
    }

    let mut affected: Vec<&str> = Vec::new();
    for (name, _path) in submodules {
        let prefix = format!("{name}/");
        if scope.iter().any(|s| s.starts_with(&prefix) || s == name) {
            affected.push(name);
        }
    }

    if affected.is_empty() {
        return None;
    }

    let paths = affected
        .iter()
        .map(|p| format!("`{p}/`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "## Submodule Context\n\n\
         The following paths in your scope are inside git submodules: {paths}\n\n\
         **Edit files normally.** The orchestrator handles all git submodule commits \
         after your task completes — you do NOT need to run any special git commands \
         for submodule files. Just use the `edit` tool as usual.\n\n\
         **Do NOT run `cargo build` or `cargo test` inside the submodule** — \
         the worktree has no build cache and compilation will be slow. Focus on \
         making your edits and let the orchestrator handle verification.\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_path_format() {
        let workspace = Path::new("/tmp/ws");
        let expected = workspace.join("2-wt-my-task");
        let result_path = workspace.join(format!("{}-wt-{}", 2, "my-task"));
        assert_eq!(result_path, expected);
    }

    #[test]
    fn merge_result_variants() {
        let s = MergeResult::Success;
        assert!(format!("{s:?}").contains("Success"));
        let n = MergeResult::NoChanges;
        assert!(format!("{n:?}").contains("NoChanges"));
        let c = MergeResult::Conflict("file.rs".into());
        assert!(format!("{c:?}").contains("file.rs"));
        let f = MergeResult::Failed("error".into());
        assert!(format!("{f:?}").contains("error"));
    }

    #[test]
    fn verify_scope_empty_is_vacuous_pass() {
        let dir = tempfile::tempdir().unwrap();
        let missing = verify_scope_accessible(dir.path(), &[]);
        assert!(missing.is_empty());
    }

    #[test]
    fn verify_scope_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        let missing = verify_scope_accessible(dir.path(), &["src/main.rs".to_string()]);
        assert!(missing.is_empty());
    }

    #[test]
    fn verify_scope_missing_file_with_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let missing = verify_scope_accessible(dir.path(), &["src/new_file.rs".to_string()]);
        assert!(missing.is_empty());
    }

    #[test]
    fn verify_scope_missing_file_and_parent() {
        let dir = tempfile::tempdir().unwrap();
        let missing = verify_scope_accessible(dir.path(), &["core/crates/lib.rs".to_string()]);
        assert_eq!(missing, vec!["core/crates/lib.rs"]);
    }

    #[test]
    fn submodule_context_with_crossing_scope() {
        let submodules = vec![("core".to_string(), PathBuf::from("/repo/core"))];
        let scope = vec!["core/crates/omegon-secrets/src/vault.rs".to_string()];
        let result = build_submodule_context_from_list(&submodules, &scope);
        assert!(result.is_some());
        let note = result.unwrap();
        assert!(note.contains("`core/`"));
        assert!(note.contains("Edit files normally"));
    }

    #[test]
    fn submodule_context_without_crossing_scope() {
        let submodules = vec![("core".to_string(), PathBuf::from("/repo/core"))];
        let scope = vec!["extensions/cleave/index.ts".to_string()];
        let result = build_submodule_context_from_list(&submodules, &scope);
        assert!(result.is_none());
    }

    #[test]
    fn submodule_context_no_submodules() {
        let result = build_submodule_context_from_list(&[], &["anything.rs".to_string()]);
        assert!(result.is_none());
    }

    #[test]
    fn submodule_context_multiple_submodules() {
        let submodules = vec![
            ("core".to_string(), PathBuf::from("/repo/core")),
            ("vendor".to_string(), PathBuf::from("/repo/vendor")),
        ];
        let scope = vec![
            "core/crates/lib.rs".to_string(),
            "vendor/dep/src/lib.rs".to_string(),
        ];
        let result = build_submodule_context_from_list(&submodules, &scope);
        assert!(result.is_some());
        let note = result.unwrap();
        assert!(note.contains("`core/`"));
        assert!(note.contains("`vendor/`"));
    }

    #[test]
    fn create_worktree_in_git_repo() {
        let cwd = std::env::current_dir().unwrap();
        // Use omegon_git to discover the repo
        if let Ok(Some(model)) = omegon_git::RepoModel::discover(&cwd) {
            let workspace = tempfile::tempdir().unwrap();
            let branch_name = format!("test-wt-{}", std::process::id());
            let result =
                create_worktree(model.repo_path(), workspace.path(), 0, "test", &branch_name);

            if let Ok(wt_path) = result {
                assert!(wt_path.exists(), "worktree should exist");

                let branch_exists = std::process::Command::new("git")
                    .args(["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch_name}")])
                    .current_dir(model.repo_path())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                assert!(
                    branch_exists,
                    "cleave worktree must create a git branch so merge can address it"
                );

                let _ = remove_worktree(model.repo_path(), &wt_path);
                let _ = delete_branch(model.repo_path(), &branch_name);
            }
        }
    }
}

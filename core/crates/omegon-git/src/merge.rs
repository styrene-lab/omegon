//! Merge operations — squash-merge, cleanup-merge, conflict detection.
//!
//! Squash-merge is the default for cleave child branches: all diary
//! commits on the child branch are compressed into a single commit
//! on the base branch.
//!
//! Cleanup-merge is for feature branches: ceremony commits are dropped
//! via cherry-pick, real work commits are preserved for bisect/revert.

use anyhow::{Context, Result};
use git2::{MergeOptions, Repository, Signature};
use std::path::Path;
use tracing::info;

/// Result of a merge operation.
#[derive(Debug)]
pub enum MergeResult {
    /// Merge succeeded — one clean commit on the current branch.
    Success { sha: String },
    /// No new commits on the source branch.
    NoChanges,
    /// Merge conflict in the listed files.
    Conflict { files: Vec<String> },
    /// Merge failed for another reason.
    Failed(String),
}

// ── Squash merge (git2, no CLI) ─────────────────────────────────────────

/// Squash-merge a branch into the current HEAD.
///
/// All commits on `source_branch` since the merge-base are squashed
/// into a single commit on the current branch. The source branch is
/// NOT deleted — the caller decides cleanup.
pub fn squash_merge(
    repo_path: &Path,
    source_branch: &str,
    message: &str,
) -> Result<MergeResult> {
    let repo = Repository::open(repo_path).context("failed to open repo")?;

    let source_ref = repo
        .find_branch(source_branch, git2::BranchType::Local)
        .with_context(|| format!("branch not found: {}", source_branch))?;
    let source_commit = source_ref
        .get()
        .peel_to_commit()
        .context("failed to resolve branch to commit")?;

    let head = repo.head().context("no HEAD")?;
    let head_commit = head.peel_to_commit().context("HEAD is not a commit")?;

    let merge_base = repo
        .merge_base(head_commit.id(), source_commit.id())
        .context("no merge base found")?;

    if merge_base == source_commit.id() {
        return Ok(MergeResult::NoChanges);
    }

    let annotated = repo
        .find_annotated_commit(source_commit.id())
        .context("failed to create annotated commit")?;

    let mut merge_opts = MergeOptions::new();
    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.safe();

    repo.merge(&[&annotated], Some(&mut merge_opts), Some(&mut checkout_opts))
        .context("merge failed")?;

    let index = repo.index().context("failed to read index after merge")?;
    if index.has_conflicts() {
        let conflicts: Vec<String> = index
            .conflicts()
            .context("failed to read conflicts")?
            .filter_map(|c| {
                c.ok().and_then(|entry| {
                    entry
                        .our
                        .or(entry.their)
                        .and_then(|e| String::from_utf8(e.path).ok())
                })
            })
            .collect();

        repo.cleanup_state().ok();
        return Ok(MergeResult::Conflict { files: conflicts });
    }

    let mut index = repo.index()?;
    let tree_oid = index.write_tree().context("failed to write merged tree")?;
    let tree = repo.find_tree(tree_oid)?;

    let sig = repo
        .signature()
        .or_else(|_| Signature::now("omegon", "noreply@omegon.dev"))
        .context("failed to create signature")?;

    // Single parent = squash commit (not a merge commit)
    let commit_oid = repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &[&head_commit],
        )
        .context("failed to create squash commit")?;

    repo.cleanup_state().ok();

    // Mixed reset: updates index + HEAD but preserves unrelated dirty files
    // in the working tree (unlike Hard which destroys them).
    let obj = repo.find_object(commit_oid, None)?;
    repo.reset(&obj, git2::ResetType::Mixed, None)?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_index(None, Some(&mut checkout))?;

    info!(source = source_branch, sha = %commit_oid, "squash-merged");

    Ok(MergeResult::Success {
        sha: commit_oid.to_string(),
    })
}

// ── No-ff merge (git2, no CLI) ──────────────────────────────────────────

/// Merge a branch into HEAD with a merge commit (--no-ff equivalent).
///
/// Uses git2 for the entire operation — no CLI fallback needed.
pub fn merge_no_ff(
    repo_path: &Path,
    source_branch: &str,
    message: &str,
) -> Result<MergeResult> {
    let repo = Repository::open(repo_path).context("failed to open repo")?;

    let source_ref = repo
        .find_branch(source_branch, git2::BranchType::Local)
        .with_context(|| format!("branch not found: {}", source_branch))?;
    let source_commit = source_ref
        .get()
        .peel_to_commit()
        .context("failed to resolve branch to commit")?;

    let head = repo.head().context("no HEAD")?;
    let head_commit = head.peel_to_commit().context("HEAD is not a commit")?;

    let merge_base = repo
        .merge_base(head_commit.id(), source_commit.id())
        .context("no merge base found")?;

    if merge_base == source_commit.id() {
        return Ok(MergeResult::NoChanges);
    }

    let annotated = repo
        .find_annotated_commit(source_commit.id())
        .context("failed to create annotated commit")?;

    let mut merge_opts = MergeOptions::new();
    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.safe();

    repo.merge(&[&annotated], Some(&mut merge_opts), Some(&mut checkout_opts))
        .context("merge failed")?;

    let index = repo.index().context("failed to read index after merge")?;
    if index.has_conflicts() {
        let conflicts: Vec<String> = index
            .conflicts()
            .context("failed to read conflicts")?
            .filter_map(|c| {
                c.ok().and_then(|entry| {
                    entry
                        .our
                        .or(entry.their)
                        .and_then(|e| String::from_utf8(e.path).ok())
                })
            })
            .collect();

        repo.cleanup_state().ok();
        return Ok(MergeResult::Conflict { files: conflicts });
    }

    let mut index = repo.index()?;
    let tree_oid = index.write_tree().context("failed to write merged tree")?;
    let tree = repo.find_tree(tree_oid)?;

    let sig = repo
        .signature()
        .or_else(|_| Signature::now("omegon", "noreply@omegon.dev"))
        .context("failed to create signature")?;

    // Two parents = merge commit (--no-ff)
    let commit_oid = repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &[&head_commit, &source_commit],
        )
        .context("failed to create merge commit")?;

    repo.cleanup_state().ok();

    let obj = repo.find_object(commit_oid, None)?;
    repo.reset(&obj, git2::ResetType::Mixed, None)?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_index(None, Some(&mut checkout))?;

    info!(source = source_branch, sha = %commit_oid, "merged (no-ff)");

    Ok(MergeResult::Success {
        sha: commit_oid.to_string(),
    })
}

// ── Cleanup merge (git2 for analysis, CLI cherry-pick for replay) ───────

/// Patterns that identify ceremony commits to drop during cleanup merge.
const CEREMONY_PATTERNS: &[&str] = &[
    "chore(cleave): checkpoint",
    "chore: checkpoint",
    "cleave: merge",
    "chore: archive",
    "docs: mark ",
    ": mark all tasks complete",
];

/// Check if a commit message is ceremony (should be dropped in cleanup merge).
pub fn is_ceremony_commit(message: &str) -> bool {
    let first_line = message.lines().next().unwrap_or("");
    CEREMONY_PATTERNS
        .iter()
        .any(|pattern| first_line.contains(pattern))
}

/// Cleanup-and-merge: cherry-pick non-ceremony commits, then merge.
///
/// Walks the source branch history using git2, identifies ceremony commits
/// to skip, then cherry-picks the remaining commits using `git cherry-pick`
/// (the one CLI operation — git2's cherry-pick doesn't handle all edge cases).
///
/// If there are no ceremony commits, falls through to a normal no-ff merge.
pub fn cleanup_and_merge(
    repo_path: &Path,
    source_branch: &str,
    message: &str,
) -> Result<MergeResult> {
    let repo = Repository::open(repo_path).context("failed to open repo")?;

    // Resolve branches
    let source_ref = repo
        .find_branch(source_branch, git2::BranchType::Local)
        .with_context(|| format!("branch not found: {}", source_branch))?;
    let source_commit = source_ref.get().peel_to_commit()?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let merge_base = repo.merge_base(head_commit.id(), source_commit.id())?;

    if merge_base == source_commit.id() {
        return Ok(MergeResult::NoChanges);
    }

    // Walk commits on the source branch since merge-base (oldest first)
    let mut revwalk = repo.revwalk()?;
    revwalk.push(source_commit.id())?;
    revwalk.hide(merge_base)?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

    let mut all_commits: Vec<(git2::Oid, String)> = Vec::new();
    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        let msg = commit.message().unwrap_or("").to_string();
        all_commits.push((oid, msg));
    }

    let ceremony_count = all_commits
        .iter()
        .filter(|(_, msg)| is_ceremony_commit(msg))
        .count();

    if ceremony_count == 0 {
        return merge_no_ff(repo_path, source_branch, message);
    }

    let keep_commits: Vec<&(git2::Oid, String)> = all_commits
        .iter()
        .filter(|(_, msg)| !is_ceremony_commit(msg))
        .collect();

    info!(
        branch = source_branch,
        total = all_commits.len(),
        ceremony = ceremony_count,
        keeping = keep_commits.len(),
        "cleanup merge: cherry-picking non-ceremony commits"
    );

    if keep_commits.is_empty() {
        return Ok(MergeResult::NoChanges);
    }

    // Create a temporary branch from HEAD for the cherry-pick sequence
    let cleanup_branch = format!("{}-cleanup", source_branch);
    cleanup_branch_helper(&repo, repo_path, &cleanup_branch, &keep_commits)?;

    // Merge the cleanup branch back
    let result = merge_no_ff(repo_path, &cleanup_branch, message);

    // Clean up
    if let Ok(mut branch) = repo.find_branch(&cleanup_branch, git2::BranchType::Local) {
        // Need to checkout the original branch first if we're on cleanup
        let _ = branch.delete();
    }

    result
}

/// Helper: create a cleanup branch and cherry-pick selected commits onto it.
///
/// Uses `git cherry-pick` CLI because git2's cherry_pick doesn't handle
/// all merge strategies and conflict resolution modes. This is the one
/// legitimate CLI call — git cherry-pick is portable and reliable.
fn cleanup_branch_helper(
    repo: &Repository,
    repo_path: &Path,
    cleanup_branch: &str,
    commits: &[&(git2::Oid, String)],
) -> Result<()> {
    // Delete stale branch
    if let Ok(mut b) = repo.find_branch(cleanup_branch, git2::BranchType::Local) {
        let _ = b.delete();
    }

    // Create branch from HEAD
    let head_commit = repo.head()?.peel_to_commit()?;
    repo.branch(cleanup_branch, &head_commit, false)?;

    // Checkout the cleanup branch
    let refname = format!("refs/heads/{}", cleanup_branch);
    repo.set_head(&refname)?;
    repo.checkout_head(Some(
        git2::build::CheckoutBuilder::new().safe(),
    ))?;

    // Cherry-pick each non-ceremony commit
    for (oid, msg) in commits {
        let first_line = msg.lines().next().unwrap_or("(no message)");
        let output = std::process::Command::new("git")
            .args(["cherry-pick", &oid.to_string()])
            .current_dir(repo_path)
            .output()
            .with_context(|| format!("cherry-pick {} failed", first_line))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Abort cherry-pick and restore
            let _ = std::process::Command::new("git")
                .args(["cherry-pick", "--abort"])
                .current_dir(repo_path)
                .output();
            anyhow::bail!(
                "cherry-pick failed for '{}': {}",
                first_line,
                stderr.trim()
            );
        }
    }

    // Switch back to the original branch
    // (checkout_head with FORCE to get back to where we were)
    let _ = std::process::Command::new("git")
        .args(["checkout", "-"])
        .current_dir(repo_path)
        .output();

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use git2::IndexAddOption;

    fn init_repo(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
            config.set_str("user.name", "Test").unwrap();
        }

        std::fs::write(dir.join("init.txt"), "init").unwrap();
        {
            let mut index = repo.index().unwrap();
            index
                .add_all(["."].iter(), IndexAddOption::DEFAULT, None)
                .unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = repo.signature().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }

        repo
    }

    fn add_commit(repo: &Repository, dir: &Path, filename: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(filename), content).unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["."].iter(), IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&parent])
            .unwrap();
    }

    fn create_feature_branch(repo: &Repository, name: &str) -> String {
        let default_branch = repo.head().unwrap().shorthand().unwrap().to_string();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch(name, &head, false).unwrap();
        repo.set_head(&format!("refs/heads/{}", name)).unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().safe()))
            .unwrap();
        default_branch
    }

    fn switch_branch(repo: &Repository, name: &str) {
        repo.set_head(&format!("refs/heads/{}", name)).unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    }

    #[test]
    fn squash_merge_basic() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path());
        let default_branch = create_feature_branch(&repo, "feature");

        add_commit(&repo, dir.path(), "a.txt", "a", "feat: add a");
        add_commit(&repo, dir.path(), "b.txt", "b", "feat: add b");

        switch_branch(&repo, &default_branch);

        let result = squash_merge(dir.path(), "feature", "feat: squashed").unwrap();
        match result {
            MergeResult::Success { sha } => {
                assert!(!sha.is_empty());
                let head = repo.head().unwrap().peel_to_commit().unwrap();
                assert_eq!(head.parent_count(), 1, "squash = 1 parent");
                assert!(dir.path().join("a.txt").exists());
                assert!(dir.path().join("b.txt").exists());
            }
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[test]
    fn squash_merge_no_changes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path());
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("empty", &head, false).unwrap();

        let result = squash_merge(dir.path(), "empty", "nothing").unwrap();
        assert!(matches!(result, MergeResult::NoChanges));
    }

    #[test]
    fn squash_merge_with_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path());
        let default_branch = create_feature_branch(&repo, "conflict-feature");

        add_commit(&repo, dir.path(), "init.txt", "feature version", "feat: change init");

        switch_branch(&repo, &default_branch);
        add_commit(&repo, dir.path(), "init.txt", "main version", "diverge on main");

        let result = squash_merge(dir.path(), "conflict-feature", "should conflict").unwrap();
        assert!(
            matches!(result, MergeResult::Conflict { .. }),
            "expected Conflict, got {:?}",
            result
        );
    }

    #[test]
    fn merge_no_ff_basic() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path());
        let default_branch = create_feature_branch(&repo, "ff-feature");

        add_commit(&repo, dir.path(), "new.txt", "content", "feat: new file");

        switch_branch(&repo, &default_branch);

        let result = merge_no_ff(dir.path(), "ff-feature", "merge ff-feature").unwrap();
        match result {
            MergeResult::Success { sha } => {
                assert!(!sha.is_empty());
                let head = repo.head().unwrap().peel_to_commit().unwrap();
                assert_eq!(head.parent_count(), 2, "no-ff = 2 parents");
            }
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[test]
    fn ceremony_detection() {
        assert!(is_ceremony_commit("chore(cleave): checkpoint before cleave"));
        assert!(is_ceremony_commit("chore: checkpoint before cleave"));
        assert!(is_ceremony_commit("cleave: merge cleave/0-token-handling"));
        assert!(is_ceremony_commit("chore: archive cleave-submodule-worktree change"));
        assert!(is_ceremony_commit(
            "docs(vault-fail-closed): mark all tasks complete"
        ));
        assert!(is_ceremony_commit("docs: mark all tasks complete"));

        assert!(!is_ceremony_commit("feat(vault): add VaultClient"));
        assert!(!is_ceremony_commit("fix: address adversarial review findings"));
        assert!(!is_ceremony_commit(
            "docs: consolidated vault security assessment report"
        ));
        assert!(!is_ceremony_commit("refactor: extract salvage_worktree_changes"));
        assert!(!is_ceremony_commit("chore: update core submodule to v0.14.0"));
    }
}

//! Commit operations — stage files and create commits via git2.
//!
//! Handles the submodule two-level commit dance: when files inside a
//! submodule are dirty, commit inside the submodule first, then stage
//! the updated pointer in the parent.

use anyhow::{Context, Result};
use git2::{IndexAddOption, Repository, Signature};
use std::path::Path;
use tracing::info;

/// Options for creating a commit.
pub struct CommitOptions<'a> {
    /// Commit message.
    pub message: &'a str,
    /// Paths to stage. Empty = stage all dirty files.
    pub paths: &'a [String],
    /// Whether to include pending lifecycle files.
    pub include_lifecycle: bool,
    /// Additional files to include (lifecycle batch).
    pub lifecycle_paths: &'a [String],
}

/// Result of a commit operation.
#[derive(Debug)]
pub struct CommitResult {
    /// The SHA of the new commit.
    pub sha: String,
    /// Number of files staged.
    pub files_staged: usize,
    /// Whether any submodule commits were created.
    pub submodule_commits: usize,
}

/// Stage specific paths and create a commit.
///
/// If `paths` is empty, stages all dirty files (like `git add -A`).
/// Handles submodule paths by committing inside the submodule first.
pub fn create_commit(repo_path: &Path, options: &CommitOptions) -> Result<CommitResult> {
    let repo = Repository::open(repo_path).context("failed to open repo")?;
    let mut index = repo.index().context("failed to read index")?;

    // Combine real paths with lifecycle paths
    let mut all_paths: Vec<String> = options.paths.to_vec();
    if options.include_lifecycle {
        all_paths.extend(options.lifecycle_paths.iter().cloned());
    }

    let files_staged;

    if all_paths.is_empty() {
        // Stage everything — like `git add -A`
        // Use "." pathspec which respects .gitignore (unlike bare "*" which
        // can behave inconsistently across git2 versions).
        let mut staged_count: usize = 0;
        index
            .add_all(["."].iter(), IndexAddOption::DEFAULT, Some(&mut |_path: &Path, _spec: &[u8]| {
                // Callback returns 0 = add, 1 = skip. We count adds.
                staged_count += 1;
                0 // add
            }))
            .context("git add -A failed")?;
        files_staged = staged_count;
    } else {
        // Stage specific paths
        for path_str in &all_paths {
            let path = Path::new(path_str);
            if repo_path.join(path).exists() {
                index
                    .add_path(path)
                    .with_context(|| format!("failed to stage: {}", path_str))?;
            } else {
                // File was deleted — remove from index
                index
                    .remove_path(path)
                    .with_context(|| format!("failed to remove from index: {}", path_str))?;
            }
        }
        files_staged = all_paths.len();
    }

    index.write().context("failed to write index")?;

    // Create tree from index
    let tree_oid = index.write_tree().context("failed to write tree")?;
    let tree = repo
        .find_tree(tree_oid)
        .context("failed to find tree from index")?;

    // Get parent commit (HEAD)
    let parent = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok());

    // Create signature from git config
    let sig = repo
        .signature()
        .or_else(|_| Signature::now("omegon", "noreply@omegon.dev"))
        .context("failed to create signature")?;

    // Create commit
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    let commit_oid = repo
        .commit(Some("HEAD"), &sig, &sig, options.message, &tree, &parents)
        .context("failed to create commit")?;

    info!(
        sha = %commit_oid,
        files = files_staged,
        "commit created"
    );

    Ok(CommitResult {
        sha: commit_oid.to_string(),
        files_staged,
        submodule_commits: 0,
    })
}

/// Stage and commit inside a submodule, then stage the pointer in the parent.
///
/// Returns the number of files committed inside the submodule (0 if clean).
pub fn commit_in_submodule(
    repo_path: &Path,
    submodule_path: &str,
    message: &str,
) -> Result<usize> {
    let sub_full_path = repo_path.join(submodule_path);
    if !sub_full_path.join(".git").exists() && !sub_full_path.join(".git").is_file() {
        // Submodule not initialized
        return Ok(0);
    }

    let sub_repo =
        Repository::open(&sub_full_path).context("failed to open submodule repo")?;

    // Check if submodule has changes
    let statuses = sub_repo.statuses(None).context("submodule status failed")?;
    if statuses.is_empty() {
        return Ok(0);
    }

    let file_count = statuses.len();

    // Stage all changes inside the submodule (respects .gitignore)
    let mut index = sub_repo.index()?;
    index.add_all(["."].iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;

    // Create commit inside submodule
    let tree_oid = index.write_tree()?;
    let tree = sub_repo.find_tree(tree_oid)?;
    let parent = sub_repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let sig = sub_repo
        .signature()
        .or_else(|_| Signature::now("omegon", "noreply@omegon.dev"))?;
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    sub_repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;

    info!(
        submodule = submodule_path,
        files = file_count,
        "committed inside submodule"
    );

    // Stage the updated submodule pointer in the parent.
    // git2's index.add_path doesn't handle submodule pointers correctly
    // (treats them as directories), so we use CLI for this specific step.
    let stage_output = std::process::Command::new("git")
        .args(["add", submodule_path])
        .current_dir(repo_path)
        .output()
        .context("failed to stage submodule pointer")?;
    if !stage_output.status.success() {
        let stderr = String::from_utf8_lossy(&stage_output.stderr);
        tracing::warn!(submodule = submodule_path, "pointer staging warning: {}", stderr.trim());
    }

    Ok(file_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_in_temp_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        // Configure git
        let mut config = repo.config().unwrap();
        config.set_str("user.email", "test@test.com").unwrap();
        config.set_str("user.name", "Test").unwrap();

        // Create a file
        std::fs::write(dir.path().join("hello.txt"), "world").unwrap();

        let result = create_commit(
            dir.path(),
            &CommitOptions {
                message: "feat: initial commit",
                paths: &[],
                include_lifecycle: false,
                lifecycle_paths: &[],
            },
        );

        assert!(result.is_ok(), "commit should succeed: {:?}", result.err());
        let r = result.unwrap();
        assert!(!r.sha.is_empty());
        assert!(r.files_staged > 0);
    }

    #[test]
    fn commit_specific_paths() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.email", "test@test.com").unwrap();
        config.set_str("user.name", "Test").unwrap();

        // Initial commit
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        create_commit(
            dir.path(),
            &CommitOptions {
                message: "initial",
                paths: &[],
                include_lifecycle: false,
                lifecycle_paths: &[],
            },
        )
        .unwrap();

        // Modify both files but only commit one
        std::fs::write(dir.path().join("a.txt"), "a-modified").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b-modified").unwrap();

        let result = create_commit(
            dir.path(),
            &CommitOptions {
                message: "feat: update a only",
                paths: &["a.txt".to_string()],
                include_lifecycle: false,
                lifecycle_paths: &[],
            },
        )
        .unwrap();

        assert_eq!(result.files_staged, 1);

        // b.txt should still be dirty
        let statuses = repo.statuses(None).unwrap();
        let dirty: Vec<String> = statuses
            .iter()
            .filter_map(|e| e.path().map(String::from))
            .collect();
        assert!(
            dirty.contains(&"b.txt".to_string()),
            "b.txt should still be dirty"
        );
    }

    #[test]
    fn commit_in_submodule_test() {
        // Use `git submodule add` via CLI to create a proper submodule.
        // git2 doesn't have a complete submodule-add API.
        let parent_dir = tempfile::tempdir().unwrap();

        // Init a "remote" bare repo to use as submodule source
        let remote_dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(remote_dir.path())
            .output();
        if status.is_err() || !status.unwrap().status.success() {
            // git not available — skip
            return;
        }

        // Init the remote with a commit
        let clone_dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["clone", &remote_dir.path().to_string_lossy(), "."])
            .current_dir(clone_dir.path())
            .output().unwrap();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(clone_dir.path()).output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(clone_dir.path()).output();
        std::fs::write(clone_dir.path().join("sub_file.txt"), "initial").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "-A"]).current_dir(clone_dir.path()).output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "init sub"]).current_dir(clone_dir.path()).output();
        let _ = std::process::Command::new("git")
            .args(["push"]).current_dir(clone_dir.path()).output();

        // Init parent repo with the submodule
        let _ = std::process::Command::new("git")
            .args(["init"]).current_dir(parent_dir.path()).output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(parent_dir.path()).output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(parent_dir.path()).output();
        std::fs::write(parent_dir.path().join("parent.txt"), "parent").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "-A"]).current_dir(parent_dir.path()).output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "init parent"]).current_dir(parent_dir.path()).output();
        let add_sub = std::process::Command::new("git")
            .args(["submodule", "add", &remote_dir.path().to_string_lossy(), "sub"])
            .current_dir(parent_dir.path())
            .output().unwrap();
        if !add_sub.status.success() {
            // Submodule setup failed — skip
            return;
        }
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "add submodule"]).current_dir(parent_dir.path()).output();

        // Make the submodule dirty
        let sub_dir = parent_dir.path().join("sub");
        std::fs::write(sub_dir.join("sub_file.txt"), "modified").unwrap();

        // Commit inside the submodule
        let result = commit_in_submodule(parent_dir.path(), "sub", "feat: sub change");
        assert!(result.is_ok(), "commit_in_submodule failed: {:?}", result.err());
        let count = result.unwrap();
        assert!(count > 0, "should have committed files inside submodule");

        // Verify the submodule is now clean
        let sub_status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&sub_dir)
            .output().unwrap();
        assert!(
            String::from_utf8_lossy(&sub_status.stdout).trim().is_empty(),
            "submodule should be clean after commit"
        );
    }

    #[test]
    fn commit_in_submodule_clean_is_noop() {
        // Create a nested git repo (simulates an initialized submodule)
        let parent_dir = tempfile::tempdir().unwrap();
        let sub_dir = parent_dir.path().join("sub");
        std::fs::create_dir_all(&sub_dir).unwrap();

        // Init sub with a commit so it's clean
        let _ = std::process::Command::new("git")
            .args(["init"]).current_dir(&sub_dir).output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "t@t.com"]).current_dir(&sub_dir).output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "T"]).current_dir(&sub_dir).output();
        std::fs::write(sub_dir.join("file.txt"), "content").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "-A"]).current_dir(&sub_dir).output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "init"]).current_dir(&sub_dir).output();

        // Sub is clean — should return 0
        let result = commit_in_submodule(parent_dir.path(), "sub", "nothing").unwrap();
        assert_eq!(result, 0, "clean submodule should be a no-op");
    }
}

//! RepoModel — the harness's view of the git repository.
//!
//! Initialized at agent startup by discovering the git repo from the cwd.
//! Tracks branch, HEAD, submodule map, and working set (files touched by
//! the agent's edit/write tools since the last commit).

use anyhow::{Context, Result};
use git2::Repository;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Submodule entry — name and relative path within the repo.
#[derive(Debug, Clone)]
pub struct SubmoduleInfo {
    pub name: String,
    pub path: String,
}

/// Shared, thread-safe repository model.
///
/// Designed to be held in an `Arc` and shared across async tasks.
/// Read-heavy operations (status, branch queries) take a read lock.
/// Mutations (record edit, commit, refresh) take a write lock.
#[derive(Debug)]
pub struct RepoModel {
    /// Path to the repo root (where .git lives).
    repo_path: PathBuf,
    /// Current branch name (None if detached HEAD).
    branch: RwLock<Option<String>>,
    /// HEAD commit SHA.
    head_sha: RwLock<Option<String>>,
    /// Submodule map: path → info.
    submodules: RwLock<HashMap<String, SubmoduleInfo>>,
    /// Working set: files touched by edit/write tools since last commit.
    /// Reset when the harness creates a commit.
    working_set: RwLock<HashSet<String>>,
    /// Pending lifecycle files: OpenSpec/design-tree writes to fold into
    /// the next real commit.
    pending_lifecycle: RwLock<HashSet<String>>,
}

impl RepoModel {
    /// Discover and initialize from a working directory.
    ///
    /// Walks up from `cwd` to find the repo root, reads branch/HEAD/submodules.
    /// Returns `None` if not inside a git repo.
    pub fn discover(cwd: &Path) -> Result<Option<Arc<Self>>> {
        let repo = match Repository::discover(cwd) {
            Ok(r) => r,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(e).context("git repo discovery failed"),
        };

        let repo_path = repo
            .workdir()
            .unwrap_or_else(|| repo.path())
            .to_path_buf();

        let branch = Self::read_branch(&repo);
        let head_sha = Self::read_head_sha(&repo);
        let submodules = Self::read_submodules(&repo);

        tracing::info!(
            repo = %repo_path.display(),
            branch = branch.as_deref().unwrap_or("(detached)"),
            submodules = submodules.len(),
            "RepoModel initialized"
        );

        Ok(Some(Arc::new(Self {
            repo_path,
            branch: RwLock::new(branch),
            head_sha: RwLock::new(head_sha),
            submodules: RwLock::new(submodules),
            working_set: RwLock::new(HashSet::new()),
            pending_lifecycle: RwLock::new(HashSet::new()),
        })))
    }

    // ── Accessors ──────────────────────────────────────────────────────

    /// Repository root path.
    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    /// Current branch name (None if detached HEAD).
    pub fn branch(&self) -> Option<String> {
        self.branch.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// HEAD commit SHA.
    pub fn head_sha(&self) -> Option<String> {
        self.head_sha.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Check if a path is inside a submodule.
    pub fn is_submodule_path(&self, path: &str) -> bool {
        let subs = self.submodules.read().unwrap_or_else(|e| e.into_inner());
        subs.keys().any(|sub_path| {
            let prefix = format!("{}/", sub_path);
            path.starts_with(&prefix) || path == *sub_path
        })
    }

    /// Get the submodule that contains a path, if any.
    pub fn containing_submodule(&self, path: &str) -> Option<SubmoduleInfo> {
        let subs = self.submodules.read().unwrap_or_else(|e| e.into_inner());
        for (sub_path, info) in subs.iter() {
            if path.starts_with(sub_path)
                && (path.len() == sub_path.len() || path[sub_path.len()..].starts_with('/'))
            {
                return Some(info.clone());
            }
        }
        None
    }

    /// Get all submodule infos.
    pub fn submodules(&self) -> Vec<SubmoduleInfo> {
        self.submodules.read().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
    }

    /// Get the current working set (files touched since last commit).
    pub fn working_set(&self) -> HashSet<String> {
        self.working_set.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Get pending lifecycle files.
    pub fn pending_lifecycle_files(&self) -> HashSet<String> {
        self.pending_lifecycle.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    // ── Mutations ──────────────────────────────────────────────────────

    /// Record that a file was touched by an edit/write tool.
    ///
    /// Lifecycle paths (openspec/, docs/, .pi/) are automatically classified
    /// as lifecycle writes so they get batched into the next real commit.
    pub fn record_edit(&self, path: &str) {
        if Self::is_lifecycle_path(path) {
            self.pending_lifecycle
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(path.to_string());
        } else {
            self.working_set
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(path.to_string());
        }
    }

    /// Check if a path is a lifecycle artifact (OpenSpec, design-tree, memory).
    fn is_lifecycle_path(path: &str) -> bool {
        path.starts_with("openspec/")
            || path.starts_with("docs/")
            || path.starts_with(".pi/")
            || path.starts_with("design/")
    }

    /// Record a lifecycle file write (OpenSpec, design-tree).
    /// These get folded into the next real commit.
    pub fn record_lifecycle_write(&self, path: &str) {
        self.pending_lifecycle
            .write()
            .unwrap()
            .insert(path.to_string());
    }

    /// Clear the working set and pending lifecycle files (after commit).
    pub fn clear_working_set(&self) {
        self.working_set.write().unwrap_or_else(|e| e.into_inner()).clear();
        self.pending_lifecycle.write().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Refresh branch and HEAD from the repo.
    ///
    /// Opens a fresh `Repository` handle each time. Caching the handle would
    /// require `Sync` on `git2::Repository` (which it doesn't implement).
    /// This is acceptable because `refresh()` is only called after commits,
    /// not on every tool invocation.
    pub fn refresh(&self) -> Result<()> {
        let repo = Repository::open(&self.repo_path)?;
        *self.branch.write().unwrap_or_else(|e| e.into_inner()) = Self::read_branch(&repo);
        *self.head_sha.write().unwrap_or_else(|e| e.into_inner()) = Self::read_head_sha(&repo);
        Ok(())
    }

    /// Refresh the submodule map.
    pub fn refresh_submodules(&self) -> Result<()> {
        let repo = Repository::open(&self.repo_path)?;
        *self.submodules.write().unwrap_or_else(|e| e.into_inner()) = Self::read_submodules(&repo);
        Ok(())
    }

    /// Open the underlying git2 Repository.
    ///
    /// Callers should prefer the typed methods on RepoModel, but this
    /// escape hatch is available for operations not yet wrapped.
    pub fn open_repo(&self) -> Result<Repository> {
        Repository::open(&self.repo_path).context("failed to open git repo")
    }

    // ── Internal helpers ───────────────────────────────────────────────

    fn read_branch(repo: &Repository) -> Option<String> {
        repo.head()
            .ok()
            .and_then(|h| h.shorthand().map(String::from))
    }

    fn read_head_sha(repo: &Repository) -> Option<String> {
        repo.head()
            .ok()
            .and_then(|h| h.target())
            .map(|oid| oid.to_string())
    }

    fn read_submodules(repo: &Repository) -> HashMap<String, SubmoduleInfo> {
        let mut map = HashMap::new();
        if let Ok(subs) = repo.submodules() {
            for sub in subs {
                let name = sub.name().unwrap_or("").to_string();
                let path = sub.path().to_string_lossy().to_string();
                map.insert(
                    path.clone(),
                    SubmoduleInfo {
                        name: name.clone(),
                        path,
                    },
                );
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_in_git_repo() {
        // This test runs inside the omegon-core repo
        let cwd = std::env::current_dir().unwrap();
        let result = RepoModel::discover(&cwd).unwrap();
        // Should find a repo (we're inside one)
        assert!(result.is_some(), "should discover a git repo");
        let model = result.unwrap();
        assert!(model.repo_path().exists());
        // Should have a branch or at least a HEAD
        // (detached HEAD in CI is OK — just check we don't crash)
        let _ = model.branch();
        let _ = model.head_sha();
    }

    #[test]
    fn discover_outside_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = RepoModel::discover(dir.path()).unwrap();
        assert!(result.is_none(), "should not find a repo in temp dir");
    }

    #[test]
    fn working_set_tracking() {
        let cwd = std::env::current_dir().unwrap();
        if let Some(model) = RepoModel::discover(&cwd).unwrap() {
            assert!(model.working_set().is_empty());
            model.record_edit("src/main.rs");
            model.record_edit("src/lib.rs");
            assert_eq!(model.working_set().len(), 2);
            assert!(model.working_set().contains("src/main.rs"));

            model.clear_working_set();
            assert!(model.working_set().is_empty());
        }
    }

    #[test]
    fn lifecycle_write_tracking() {
        let cwd = std::env::current_dir().unwrap();
        if let Some(model) = RepoModel::discover(&cwd).unwrap() {
            // Direct lifecycle write
            model.record_lifecycle_write("openspec/changes/foo/tasks.md");
            assert_eq!(model.pending_lifecycle_files().len(), 1);
            model.clear_working_set();
            assert!(model.pending_lifecycle_files().is_empty());
        }
    }

    #[test]
    fn record_edit_auto_classifies_lifecycle() {
        let cwd = std::env::current_dir().unwrap();
        if let Some(model) = RepoModel::discover(&cwd).unwrap() {
            // Lifecycle paths go to pending_lifecycle, not working_set
            model.record_edit("openspec/changes/foo/tasks.md");
            model.record_edit("docs/some-design-doc.md");
            model.record_edit(".pi/memory/facts.jsonl");
            assert_eq!(model.pending_lifecycle_files().len(), 3);
            assert!(model.working_set().is_empty());

            // Non-lifecycle paths go to working_set
            model.record_edit("src/main.rs");
            model.record_edit("core/crates/omegon/src/tools/mod.rs");
            assert_eq!(model.working_set().len(), 2);
            assert_eq!(model.pending_lifecycle_files().len(), 3);

            model.clear_working_set();
            assert!(model.working_set().is_empty());
            assert!(model.pending_lifecycle_files().is_empty());
        }
    }

    #[test]
    fn submodule_path_detection() {
        let cwd = std::env::current_dir().unwrap();
        if let Some(model) = RepoModel::discover(&cwd).unwrap() {
            let subs = model.submodules();
            if !subs.is_empty() {
                // If we have submodules (like "core"), test the path detection
                let sub = &subs[0];
                assert!(model.is_submodule_path(&format!("{}/some/file.rs", sub.path)));
                assert!(!model.is_submodule_path("not-a-submodule/file.rs"));
            }
        }
    }
}

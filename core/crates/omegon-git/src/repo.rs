//! RepoModel — the harness's view of the git repository.
//!
//! Initialized at agent startup by discovering the git repo from the cwd.
//! In jj co-located repos, delegates working copy tracking to jj (the
//! working directory IS a mutable commit — no manual tracking needed).
//! In git-only repos, falls back to manual HashSet tracking.

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
    /// Whether jj is co-located (`.jj/` exists alongside `.git/`).
    /// When true, working copy tracking delegates to jj instead of
    /// the manual HashSet.
    jj_colocated: bool,
    /// Current branch name (None if detached HEAD).
    branch: RwLock<Option<String>>,
    /// HEAD commit SHA.
    head_sha: RwLock<Option<String>>,
    /// jj change ID for the current working copy (if jj active).
    jj_change_id: RwLock<Option<String>>,
    /// Submodule map: path → info.
    submodules: RwLock<HashMap<String, SubmoduleInfo>>,
    /// Working set: files touched by edit/write tools since last commit.
    /// Only used when jj is NOT active — jj tracks this automatically.
    working_set: RwLock<HashSet<String>>,
    /// Pending lifecycle files: OpenSpec/design-tree writes to fold into
    /// the next real commit. Only used when jj is NOT active.
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

        let repo_path = repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf();

        let branch = Self::read_branch(&repo);
        let head_sha = Self::read_head_sha(&repo);
        let submodules = Self::read_submodules(&repo);
        let jj_colocated = crate::jj::is_jj_repo(&repo_path);

        // Read jj change ID if co-located
        let jj_change_id = if jj_colocated {
            let id = std::process::Command::new("jj")
                .args(["log", "-r", "@", "--no-graph", "-T", "change_id"])
                .current_dir(&repo_path)
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if s.is_empty() { None } else { Some(s) }
                    } else {
                        None
                    }
                });
            id
        } else {
            None
        };

        tracing::info!(
            repo = %repo_path.display(),
            branch = branch.as_deref().unwrap_or("(detached)"),
            submodules = submodules.len(),
            jj = jj_colocated,
            change_id = jj_change_id.as_deref().unwrap_or("none"),
            "RepoModel initialized"
        );

        Ok(Some(Arc::new(Self {
            repo_path,
            jj_colocated,
            branch: RwLock::new(branch),
            head_sha: RwLock::new(head_sha),
            jj_change_id: RwLock::new(jj_change_id),
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

    /// Whether jj is co-located with git.
    pub fn is_jj(&self) -> bool {
        self.jj_colocated
    }

    /// Current jj change ID (if jj active).
    pub fn jj_change_id(&self) -> Option<String> {
        self.jj_change_id
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Current branch name (None if detached HEAD).
    pub fn branch(&self) -> Option<String> {
        self.branch
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// HEAD commit SHA.
    pub fn head_sha(&self) -> Option<String> {
        self.head_sha
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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
            let prefix = format!("{}/", sub_path);
            if path.starts_with(&prefix) || path == *sub_path {
                return Some(info.clone());
            }
        }
        None
    }

    /// Get all submodule infos.
    pub fn submodules(&self) -> Vec<SubmoduleInfo> {
        self.submodules
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// Get the current working set (files touched since last commit).
    ///
    /// When jj is co-located, queries jj's diff (the working copy IS a
    /// change, so jj knows exactly what's modified). When git-only,
    /// returns the manually-tracked HashSet.
    ///
    /// Note: the jj path spawns a subprocess. Callers should avoid
    /// calling this in tight loops — cache the result if needed.
    pub fn working_set(&self) -> HashSet<String> {
        if self.jj_colocated {
            crate::jj::diff_summary(&self.repo_path)
                .unwrap_or_default()
                .into_iter()
                .collect()
        } else {
            self.working_set
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
    }

    /// Get pending lifecycle files.
    pub fn pending_lifecycle_files(&self) -> HashSet<String> {
        self.pending_lifecycle
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    // ── Mutations ──────────────────────────────────────────────────────

    /// Record that a file was touched by an edit/write tool.
    ///
    /// When jj is co-located, this is a **no-op** — jj's working copy
    /// automatically tracks all file changes. No manual bookkeeping needed.
    ///
    /// When git-only, lifecycle paths (openspec/, docs/, ai/) are classified
    /// as lifecycle writes for batching into the next real commit.
    pub fn record_edit(&self, path: &str) {
        if self.jj_colocated {
            // jj tracks file changes automatically via the working copy.
            // No manual recording needed.
            return;
        }

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
    /// Keep in sync with `omegon::paths::is_agent_artifact()`.
    fn is_lifecycle_path(path: &str) -> bool {
        path.starts_with("ai/")           // canonical location
            || path.starts_with("openspec/")  // legacy
            || path.starts_with("docs/")      // legacy
            || path.starts_with(".omegon/") // tool config
    }

    /// Record a lifecycle file write (OpenSpec, design-tree).
    /// These get folded into the next real commit.
    pub fn record_lifecycle_write(&self, path: &str) {
        self.pending_lifecycle
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_string());
    }

    /// Clear the working set and pending lifecycle files (after commit).
    ///
    /// When jj is active, this updates the jj change ID (jj new creates
    /// a fresh change, so the ID changes).
    pub fn clear_working_set(&self) {
        self.working_set
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.pending_lifecycle
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Refresh branch, HEAD, and jj state from the repo.
    pub fn refresh(&self) -> Result<()> {
        let repo = Repository::open(&self.repo_path)?;
        *self.branch.write().unwrap_or_else(|e| e.into_inner()) = Self::read_branch(&repo);
        *self.head_sha.write().unwrap_or_else(|e| e.into_inner()) = Self::read_head_sha(&repo);

        // Refresh jj change ID if co-located
        if self.jj_colocated {
            let id = std::process::Command::new("jj")
                .args(["log", "-r", "@", "--no-graph", "-T", "change_id"])
                .current_dir(&self.repo_path)
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if s.is_empty() { None } else { Some(s) }
                    } else {
                        None
                    }
                });
            *self.jj_change_id.write().unwrap_or_else(|e| e.into_inner()) = id;
        }

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
            if model.is_jj() {
                // When jj is active, working_set queries jj diff — we can't
                // control what jj reports, so just verify it doesn't crash.
                let _ = model.working_set();
                // record_edit is a no-op in jj mode
                model.record_edit("src/main.rs");
                // working_set still works (returns jj's view)
                let _ = model.working_set();
            } else {
                // Git-only: manual tracking
                assert!(model.working_set().is_empty());
                model.record_edit("src/main.rs");
                model.record_edit("src/lib.rs");
                assert_eq!(model.working_set().len(), 2);
                assert!(model.working_set().contains("src/main.rs"));

                model.clear_working_set();
                assert!(model.working_set().is_empty());
            }
        }
    }

    #[test]
    fn lifecycle_write_tracking() {
        let cwd = std::env::current_dir().unwrap();
        if let Some(model) = RepoModel::discover(&cwd).unwrap() {
            if model.is_jj() {
                // In jj mode, lifecycle writes are no-ops (jj tracks everything)
                model.record_lifecycle_write("openspec/changes/foo/tasks.md");
                // Direct writes still go to pending (for git-path compat)
                assert_eq!(model.pending_lifecycle_files().len(), 1);
                model.clear_working_set();
            } else {
                model.record_lifecycle_write("openspec/changes/foo/tasks.md");
                assert_eq!(model.pending_lifecycle_files().len(), 1);
                model.clear_working_set();
                assert!(model.pending_lifecycle_files().is_empty());
            }
        }
    }

    #[test]
    fn record_edit_auto_classifies_lifecycle() {
        let cwd = std::env::current_dir().unwrap();
        if let Some(model) = RepoModel::discover(&cwd).unwrap() {
            if model.is_jj() {
                // In jj mode, record_edit is a no-op — jj tracks automatically
                model.record_edit("openspec/changes/foo/tasks.md");
                model.record_edit("src/main.rs");
                // Working set comes from jj diff, not our HashSet
                let _ = model.working_set();
                // Pending lifecycle stays empty (record_edit is no-op in jj)
                assert!(model.pending_lifecycle_files().is_empty());
            } else {
                // Git-only: lifecycle classification
                model.record_edit("openspec/changes/foo/tasks.md");
                model.record_edit("docs/some-design-doc.md");
                model.record_edit(".omegon/memory/facts.jsonl");
                assert_eq!(model.pending_lifecycle_files().len(), 3);
                assert!(model.working_set().is_empty());

                model.record_edit("src/main.rs");
                model.record_edit("core/crates/omegon/src/tools/mod.rs");
                assert_eq!(model.working_set().len(), 2);
                assert_eq!(model.pending_lifecycle_files().len(), 3);

                model.clear_working_set();
                assert!(model.working_set().is_empty());
                assert!(model.pending_lifecycle_files().is_empty());
            }
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

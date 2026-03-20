//! Git status queries via git2.
//!
//! Provides dirty-file detection, staged-file listing, and submodule
//! state classification without shelling out to `git status`.

use anyhow::{Context, Result};
use git2::{Repository, StatusOptions, StatusShow};
use std::path::Path;

/// A file's status in the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// Modified in working tree (not staged).
    Modified,
    /// Staged for commit.
    Staged,
    /// Both staged and has further modifications.
    StagedAndModified,
    /// New file (untracked).
    Untracked,
    /// Deleted.
    Deleted,
    /// Renamed.
    Renamed,
    /// Submodule with modified content.
    SubmoduleModified,
}

/// A single file entry from git status.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub path: String,
    pub status: FileStatus,
    /// True if this path is a submodule root.
    pub is_submodule: bool,
}

/// Full status snapshot of a repository.
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub entries: Vec<StatusEntry>,
    pub is_clean: bool,
}

impl RepoStatus {
    /// Paths that are dirty (modified, staged, untracked, deleted).
    pub fn dirty_paths(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.status != FileStatus::Untracked)
            .map(|e| e.path.as_str())
            .collect()
    }

    /// Paths that are staged for commit.
    pub fn staged_paths(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| {
                matches!(
                    e.status,
                    FileStatus::Staged | FileStatus::StagedAndModified
                )
            })
            .map(|e| e.path.as_str())
            .collect()
    }

    /// Untracked files.
    pub fn untracked_paths(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.status == FileStatus::Untracked)
            .map(|e| e.path.as_str())
            .collect()
    }
}

/// Query the status of a repository at the given path.
pub fn query_status(repo_path: &Path) -> Result<RepoStatus> {
    let repo = Repository::open(repo_path).context("failed to open repo for status")?;

    let mut opts = StatusOptions::new();
    opts.show(StatusShow::IndexAndWorkdir)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_unmodified(false)
        .exclude_submodules(false);

    let statuses = repo.statuses(Some(&mut opts)).context("git status failed")?;

    let mut entries = Vec::with_capacity(statuses.len());

    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("").to_string();
        let s = entry.status();

        // Submodule detection via status flags — git2 sets SUBMODULE flags
        // when the entry is a submodule with modified/dirty content.
        let is_submodule = s.intersects(
            git2::Status::WT_TYPECHANGE | git2::Status::INDEX_TYPECHANGE,
        ) || {
            // Check if old file mode indicates a submodule (commit mode = 0o160000)
            entry.index_to_workdir().map_or(false, |d| {
                matches!(d.old_file().mode(), git2::FileMode::Commit)
                    || matches!(d.new_file().mode(), git2::FileMode::Commit)
            }) || entry.head_to_index().map_or(false, |d| {
                matches!(d.old_file().mode(), git2::FileMode::Commit)
                    || matches!(d.new_file().mode(), git2::FileMode::Commit)
            })
        };

        let file_status = if is_submodule {
            FileStatus::SubmoduleModified
        } else if s.is_index_new() || s.is_index_modified() || s.is_index_renamed() {
            if s.is_wt_modified() {
                FileStatus::StagedAndModified
            } else {
                FileStatus::Staged
            }
        } else if s.is_wt_deleted() || s.is_index_deleted() {
            FileStatus::Deleted
        } else if s.is_wt_renamed() || s.is_index_renamed() {
            FileStatus::Renamed
        } else if s.is_wt_new() {
            FileStatus::Untracked
        } else if s.is_wt_modified() {
            FileStatus::Modified
        } else {
            // Other status flags — treat as modified
            FileStatus::Modified
        };

        entries.push(StatusEntry {
            path,
            status: file_status,
            is_submodule,
        });
    }

    let is_clean = entries.is_empty();

    Ok(RepoStatus { entries, is_clean })
}

/// Check if a working tree is clean (no dirty files).
pub fn is_clean(repo_path: &Path) -> Result<bool> {
    Ok(query_status(repo_path)?.is_clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_in_current_repo() {
        let cwd = std::env::current_dir().unwrap();
        if let Ok(repo) = Repository::discover(&cwd) {
            let repo_root = repo.workdir().unwrap_or_else(|| repo.path());
            let status = query_status(repo_root);
            assert!(status.is_ok(), "status should succeed: {:?}", status.err());
        }
    }

    #[test]
    fn status_entries_have_paths() {
        let cwd = std::env::current_dir().unwrap();
        if let Ok(repo) = Repository::discover(&cwd) {
            let repo_root = repo.workdir().unwrap_or_else(|| repo.path());
            let status = query_status(repo_root).unwrap();
            for entry in &status.entries {
                assert!(!entry.path.is_empty(), "entry path should not be empty");
            }
        }
    }
}

//! Read-only memory/federation status projection.
//!
//! The projection treats Git-tracked JSONL facts as the durable cross-checkout
//! substrate. Local databases and vector stores are rebuildable indexes over
//! those files, not coordination state.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationMode {
    OneOff,
    OrdinaryGit,
    LifecycleProject,
    Federation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryAuthority {
    GitJsonl { paths: Vec<PathBuf> },
    LocalIndexOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryIndexState {
    Fresh,
    Stale,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSummary {
    pub root: PathBuf,
    pub branch: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFederationStatusProjection {
    pub cwd: PathBuf,
    pub mode: CoordinationMode,
    pub signals: Vec<String>,
    pub git: Option<GitSummary>,
    pub memory_authority: MemoryAuthority,
    pub memory_index: MemoryIndexState,
    pub recommended_behavior: String,
}

pub fn project_memory_federation_status(cwd: impl AsRef<Path>) -> MemoryFederationStatusProjection {
    let cwd = cwd.as_ref().to_path_buf();
    let git = git_summary(&cwd);
    let root = git
        .as_ref()
        .map(|summary| summary.root.as_path())
        .unwrap_or(cwd.as_path());
    let mut signals = Vec::new();

    if git.is_some() {
        signals.push("git".to_string());
    }

    let lifecycle = lifecycle_signals(root);
    signals.extend(lifecycle.iter().cloned());

    let federation = federation_signals(root);
    signals.extend(federation.iter().cloned());

    let tracked_jsonl = git
        .as_ref()
        .map(|summary| tracked_jsonl_facts(&summary.root))
        .unwrap_or_default();
    let memory_authority = if !tracked_jsonl.is_empty() {
        signals.push("memory:git-jsonl".to_string());
        MemoryAuthority::GitJsonl {
            paths: tracked_jsonl,
        }
    } else if has_local_memory_index(root) {
        signals.push("memory:local-index".to_string());
        MemoryAuthority::LocalIndexOnly
    } else {
        MemoryAuthority::None
    };

    let memory_index = memory_index_state(root, &memory_authority);
    let mode = if !federation.is_empty() {
        CoordinationMode::Federation
    } else if !lifecycle.is_empty() {
        CoordinationMode::LifecycleProject
    } else if git.is_some() {
        CoordinationMode::OrdinaryGit
    } else {
        CoordinationMode::OneOff
    };

    let recommended_behavior = recommendation(mode, &memory_authority, memory_index).to_string();

    MemoryFederationStatusProjection {
        cwd,
        mode,
        signals,
        git,
        memory_authority,
        memory_index,
        recommended_behavior,
    }
}

fn git_summary(cwd: &Path) -> Option<GitSummary> {
    let root = git_output(cwd, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root);
    let branch = git_output(cwd, &["branch", "--show-current"]).filter(|value| !value.is_empty());
    let dirty = git_output(cwd, &["status", "--porcelain"])
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    Some(GitSummary {
        root,
        branch,
        dirty,
    })
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn lifecycle_signals(root: &Path) -> Vec<String> {
    let mut signals = Vec::new();
    if root.join("AGENTS.md").exists() {
        signals.push("AGENTS.md".to_string());
    }
    if root.join("openspec").is_dir() {
        signals.push("openspec".to_string());
    }
    if root.join("CHANGELOG.md").exists() {
        signals.push("CHANGELOG.md".to_string());
    }
    if root.join("docs").is_dir() {
        signals.push("docs".to_string());
    }
    signals
}

fn federation_signals(root: &Path) -> Vec<String> {
    let mut signals = Vec::new();
    if let Some(worktree_list) = git_output(root, &["worktree", "list", "--porcelain"]) {
        let count = worktree_list
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count();
        if count > 1 {
            signals.push(format!("git-worktrees:{count}"));
        }
    }
    signals
}

fn tracked_jsonl_facts(root: &Path) -> Vec<PathBuf> {
    let Some(output) = git_output(root, &["ls-files", "*.jsonl"]) else {
        return Vec::new();
    };
    output
        .lines()
        .filter(|line| is_memory_fact_jsonl(line))
        .map(PathBuf::from)
        .collect()
}

fn is_memory_fact_jsonl(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.ends_with("facts.jsonl")
        && (normalized.starts_with("ai/memory/")
            || normalized.starts_with(".omegon/memory/")
            || normalized.starts_with("memory/")
            || normalized.starts_with("docs/memory/"))
}

fn has_local_memory_index(root: &Path) -> bool {
    [
        root.join("ai/memory/facts.db"),
        root.join(".omegon/memory/facts.db"),
        root.join("memory/facts.db"),
    ]
    .iter()
    .any(|path| path.exists())
}

fn memory_index_state(root: &Path, authority: &MemoryAuthority) -> MemoryIndexState {
    let index_paths = [
        root.join("ai/memory/facts.db"),
        root.join(".omegon/memory/facts.db"),
        root.join("memory/facts.db"),
    ];
    let Some(index_meta) = index_paths
        .iter()
        .find_map(|path| path.metadata().ok().and_then(|meta| meta.modified().ok()))
    else {
        return match authority {
            MemoryAuthority::GitJsonl { .. } => MemoryIndexState::Missing,
            MemoryAuthority::LocalIndexOnly => MemoryIndexState::Unknown,
            MemoryAuthority::None => MemoryIndexState::Missing,
        };
    };

    let MemoryAuthority::GitJsonl { paths } = authority else {
        return MemoryIndexState::Unknown;
    };

    let newest_fact = paths
        .iter()
        .filter_map(|path| root.join(path).metadata().ok()?.modified().ok())
        .max();
    match newest_fact {
        Some(newest) if newest > index_meta => MemoryIndexState::Stale,
        Some(_) => MemoryIndexState::Fresh,
        None => MemoryIndexState::Unknown,
    }
}

fn recommendation(
    mode: CoordinationMode,
    authority: &MemoryAuthority,
    index: MemoryIndexState,
) -> &'static str {
    match (mode, authority, index) {
        (CoordinationMode::OneOff, MemoryAuthority::None, _) => {
            "No Git-tracked memory authority detected; treat memory as local/session scoped."
        }
        (_, MemoryAuthority::GitJsonl { .. }, MemoryIndexState::Stale) => {
            "Git-tracked JSONL facts are authoritative; rebuild the local memory index, then use normal Git fetch/merge/rebase for checkout continuity."
        }
        (_, MemoryAuthority::GitJsonl { .. }, _) => {
            "Git-tracked JSONL facts are authoritative; use normal Git fetch/merge/rebase for checkout continuity."
        }
        (_, MemoryAuthority::LocalIndexOnly, _) => {
            "Only a local memory index was detected; do not treat it as cross-checkout coordination state."
        }
        _ => "No project memory facts detected; no memory synchronization action is applicable.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), &["init"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    #[test]
    fn non_git_directory_is_one_off_without_memory_authority() {
        let dir = tempfile::tempdir().expect("tempdir");
        let projection = project_memory_federation_status(dir.path());

        assert_eq!(projection.mode, CoordinationMode::OneOff);
        assert_eq!(projection.memory_authority, MemoryAuthority::None);
        assert!(projection.recommended_behavior.contains("local/session"));
    }

    #[test]
    fn git_repo_without_lifecycle_signals_is_ordinary_git() {
        let dir = init_repo();
        let projection = project_memory_federation_status(dir.path());

        assert_eq!(projection.mode, CoordinationMode::OrdinaryGit);
        assert!(projection.signals.contains(&"git".to_string()));
    }

    #[test]
    fn tracked_jsonl_facts_are_authoritative_memory() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join("ai/memory")).expect("memory dir");
        fs::write(
            dir.path().join("ai/memory/facts.jsonl"),
            "{\"id\":\"fact-1\"}\n",
        )
        .expect("facts");
        fs::write(dir.path().join("AGENTS.md"), "# Agent rules\n").expect("agents");
        git(dir.path(), &["add", "ai/memory/facts.jsonl", "AGENTS.md"]);
        git(dir.path(), &["commit", "-m", "seed"]);

        let projection = project_memory_federation_status(dir.path());

        assert_eq!(projection.mode, CoordinationMode::LifecycleProject);
        assert_eq!(
            projection.memory_authority,
            MemoryAuthority::GitJsonl {
                paths: vec![PathBuf::from("ai/memory/facts.jsonl")]
            }
        );
        assert_eq!(projection.memory_index, MemoryIndexState::Missing);
        assert!(
            projection
                .recommended_behavior
                .contains("Git-tracked JSONL")
        );
    }

    #[test]
    fn local_index_without_jsonl_is_not_checkout_authority() {
        let dir = init_repo();
        fs::create_dir_all(dir.path().join(".omegon/memory")).expect("memory dir");
        fs::write(dir.path().join(".omegon/memory/facts.db"), "index").expect("index");

        let projection = project_memory_federation_status(dir.path());

        assert_eq!(projection.memory_authority, MemoryAuthority::LocalIndexOnly);
        assert!(
            projection
                .recommended_behavior
                .contains("local memory index")
        );
    }
}

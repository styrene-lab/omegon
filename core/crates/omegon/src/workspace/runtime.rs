use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};

use super::types::{WorkspaceLease, WorkspaceRegistry};

const STALE_HEARTBEAT_SECS: i64 = 300;

pub fn runtime_dir(cwd: &Path) -> PathBuf {
    cwd.join(".omegon").join("runtime")
}

pub fn workspace_lease_path(cwd: &Path) -> PathBuf {
    runtime_dir(cwd).join("workspace.json")
}

pub fn workspace_registry_path(cwd: &Path) -> PathBuf {
    runtime_dir(cwd).join("workspaces.json")
}

pub fn ensure_runtime_dir(cwd: &Path) -> anyhow::Result<PathBuf> {
    let dir = runtime_dir(cwd);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

pub fn read_workspace_lease(cwd: &Path) -> anyhow::Result<Option<WorkspaceLease>> {
    let path = workspace_lease_path(cwd);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let lease = serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(lease))
}

pub fn write_workspace_lease(cwd: &Path, lease: &WorkspaceLease) -> anyhow::Result<()> {
    ensure_runtime_dir(cwd)?;
    let path = workspace_lease_path(cwd);
    let json = serde_json::to_string_pretty(lease)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn read_workspace_registry(cwd: &Path) -> anyhow::Result<Option<WorkspaceRegistry>> {
    let path = workspace_registry_path(cwd);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let registry = serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(registry))
}

pub fn write_workspace_registry(cwd: &Path, registry: &WorkspaceRegistry) -> anyhow::Result<()> {
    ensure_runtime_dir(cwd)?;
    let path = workspace_registry_path(cwd);
    let json = serde_json::to_string_pretty(registry)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn current_timestamp() -> String {
    Utc::now().to_rfc3339()
}

pub fn heartbeat_epoch_secs(heartbeat: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(heartbeat)
        .ok()
        .map(|dt| dt.timestamp())
}

pub fn workspace_id_from_path(path: &Path) -> String {
    let normalized = path
        .components()
        .filter_map(|component| {
            let text = component.as_os_str().to_string_lossy();
            if text == "/" || text.is_empty() {
                None
            } else {
                Some(text)
            }
        })
        .collect::<Vec<_>>()
        .join("::");
    if normalized.is_empty() {
        "root".into()
    } else {
        normalized
    }
}

pub fn heartbeat_is_stale(now_epoch_secs: i64, heartbeat_epoch_secs: i64) -> bool {
    now_epoch_secs.saturating_sub(heartbeat_epoch_secs) > STALE_HEARTBEAT_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::types::{
        Mutability, WorkspaceBackendKind, WorkspaceKind, WorkspaceRole, WorkspaceSummary,
        WorkspaceVcsRef,
    };

    #[test]
    fn runtime_paths_are_under_omegon_runtime() {
        let cwd = Path::new("/tmp/project");
        assert_eq!(runtime_dir(cwd), PathBuf::from("/tmp/project/.omegon/runtime"));
        assert_eq!(
            workspace_lease_path(cwd),
            PathBuf::from("/tmp/project/.omegon/runtime/workspace.json")
        );
        assert_eq!(
            workspace_registry_path(cwd),
            PathBuf::from("/tmp/project/.omegon/runtime/workspaces.json")
        );
    }

    #[test]
    fn workspace_id_is_deterministic_from_path() {
        assert_eq!(workspace_id_from_path(Path::new("/tmp/example-project")), "tmp::example-project");
    }

    #[test]
    fn heartbeat_staleness_threshold_is_deterministic() {
        assert!(!heartbeat_is_stale(1_000, 701));
        assert!(heartbeat_is_stale(1_000, 699));
    }

    #[test]
    fn registry_round_trip_io() {
        let dir = tempfile::tempdir().unwrap();
        let registry = WorkspaceRegistry {
            project_id: "proj".into(),
            repo_root: dir.path().display().to_string(),
            workspaces: vec![WorkspaceSummary {
                workspace_id: "ws".into(),
                label: "primary".into(),
                path: dir.path().display().to_string(),
                backend_kind: WorkspaceBackendKind::LocalDir,
                vcs_ref: Some(WorkspaceVcsRef {
                    vcs: "git".into(),
                    branch: Some("main".into()),
                    revision: None,
                    remote: Some("origin".into()),
                }),
                bindings: crate::workspace::types::WorkspaceBindings::default(),
                branch: "main".into(),
                role: WorkspaceRole::Primary,
                workspace_kind: WorkspaceKind::Mixed,
                mutability: Mutability::Mutable,
                owner_session_id: Some("s1".into()),
                last_heartbeat: current_timestamp(),
                stale: false,
            }],
        };
        write_workspace_registry(dir.path(), &registry).unwrap();
        let loaded = read_workspace_registry(dir.path()).unwrap().unwrap();
        assert_eq!(loaded, registry);
    }
}

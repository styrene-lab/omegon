use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};

use super::types::{WorkspaceLease, WorkspaceRegistry};

const STALE_HEARTBEAT_SECS: i64 = 300;

pub fn workspace_root(cwd: &Path) -> PathBuf {
    let mut dir = cwd.to_path_buf();
    loop {
        let git_path = dir.join(".git");
        let jj_path = dir.join(".jj");
        if git_path.is_dir() || git_path.is_file() || jj_path.is_dir() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    cwd.to_path_buf()
}

pub fn runtime_dir(cwd: &Path) -> PathBuf {
    workspace_root(cwd).join(".omegon").join("runtime")
}

/// Per-instance lease path: `.omegon/runtime/{instance_id}/workspace.json`.
pub fn instance_lease_path(cwd: &Path, instance_id: &str) -> PathBuf {
    crate::paths::runtime_instance_dir(cwd, instance_id).join("workspace.json")
}

/// Legacy shared lease path (pre-isolation). Used as read fallback.
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

fn ensure_instance_dir(cwd: &Path, instance_id: &str) -> anyhow::Result<PathBuf> {
    let dir = crate::paths::runtime_instance_dir(cwd, instance_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// Read a workspace lease — checks instance-specific paths first, then legacy.
///
/// Returns the first active (non-stale) lease found, or any lease if all are stale.
pub fn read_workspace_lease(cwd: &Path) -> anyhow::Result<Option<WorkspaceLease>> {
    // Try instance-specific leases first
    let active = read_all_active_leases(cwd);
    if let Some((_id, lease)) = active.into_iter().next() {
        return Ok(Some(lease));
    }
    // Fallback to legacy shared path
    let path = workspace_lease_path(cwd);
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let lease = serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(lease))
}

/// Write workspace lease to the instance-specific path.
pub fn write_workspace_lease(
    cwd: &Path,
    instance_id: &str,
    lease: &WorkspaceLease,
) -> anyhow::Result<()> {
    ensure_instance_dir(cwd, instance_id)?;
    let path = instance_lease_path(cwd, instance_id);
    let json = serde_json::to_string_pretty(lease)?;
    crate::filelock::atomic_write(&path, json.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Read all active (non-stale) instance leases.
pub fn read_all_active_leases(cwd: &Path) -> Vec<(String, WorkspaceLease)> {
    let rt_dir = runtime_dir(cwd);
    let now = Utc::now().timestamp();
    let mut leases = Vec::new();

    let entries = match std::fs::read_dir(&rt_dir) {
        Ok(e) => e,
        Err(_) => return leases,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        // Must match {mode}-{pid} pattern
        if !dir_name.contains('-') {
            continue;
        }
        let lease_file = path.join("workspace.json");
        if !lease_file.exists() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&lease_file) else {
            continue;
        };
        let Ok(lease) = serde_json::from_str::<WorkspaceLease>(&text) else {
            continue;
        };

        if let Some(epoch) = heartbeat_epoch_secs(&lease.last_heartbeat)
            && !heartbeat_is_stale(now, epoch)
        {
            leases.push((dir_name, lease));
        }
    }

    leases
}

pub fn read_workspace_registry(cwd: &Path) -> anyhow::Result<Option<WorkspaceRegistry>> {
    let path = workspace_registry_path(cwd);
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let registry =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(registry))
}

pub fn write_workspace_registry(cwd: &Path, registry: &WorkspaceRegistry) -> anyhow::Result<()> {
    ensure_runtime_dir(cwd)?;
    let path = workspace_registry_path(cwd);
    let json = serde_json::to_string_pretty(registry)?;
    crate::filelock::atomic_write_locked(&path, json.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Remove this instance's runtime directory on clean shutdown.
pub fn cleanup_instance(cwd: &Path, instance_id: &str) {
    let dir = crate::paths::runtime_instance_dir(cwd, instance_id);
    if dir.is_dir() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Prune stale instance directories (heartbeat older than 5 minutes AND PID dead).
pub fn prune_stale_instances(cwd: &Path) -> Vec<String> {
    let rt_dir = runtime_dir(cwd);
    let now = Utc::now().timestamp();
    let mut pruned = Vec::new();

    let entries = match std::fs::read_dir(&rt_dir) {
        Ok(e) => e,
        Err(_) => return pruned,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        if !dir_name.contains('-') {
            continue;
        }
        let lease_file = path.join("workspace.json");
        if !lease_file.exists() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&lease_file) else {
            continue;
        };
        let Ok(lease) = serde_json::from_str::<WorkspaceLease>(&text) else {
            continue;
        };

        let stale = heartbeat_epoch_secs(&lease.last_heartbeat)
            .map(|epoch| heartbeat_is_stale(now, epoch))
            .unwrap_or(true);

        if !stale {
            continue;
        }

        // Check if the PID is still alive
        let pid_alive = dir_name
            .rsplit_once('-')
            .and_then(|(_, pid_str)| pid_str.parse::<i32>().ok())
            .map(|pid| unsafe { libc::kill(pid, 0) } == 0)
            .unwrap_or(false);

        if !pid_alive {
            let _ = std::fs::remove_dir_all(&path);
            pruned.push(dir_name);
        }
    }

    // Also clean up legacy workspace.json if stale
    let legacy_path = workspace_lease_path(cwd);
    if legacy_path.exists()
        && let Ok(text) = std::fs::read_to_string(&legacy_path)
        && let Ok(lease) = serde_json::from_str::<WorkspaceLease>(&text)
    {
        let stale = heartbeat_epoch_secs(&lease.last_heartbeat)
            .map(|epoch| heartbeat_is_stale(now, epoch))
            .unwrap_or(true);
        if stale {
            let _ = std::fs::remove_file(&legacy_path);
            pruned.push("legacy".to_string());
        }
    }

    pruned
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
    fn runtime_paths_are_under_workspace_root_runtime() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let cwd = dir.path().join("nested/project");
        std::fs::create_dir_all(&cwd).unwrap();
        assert_eq!(workspace_root(&cwd), dir.path());
        assert_eq!(runtime_dir(&cwd), dir.path().join(".omegon/runtime"));
        assert_eq!(
            workspace_lease_path(&cwd),
            dir.path().join(".omegon/runtime/workspace.json")
        );
        assert_eq!(
            workspace_registry_path(&cwd),
            dir.path().join(".omegon/runtime/workspaces.json")
        );
    }

    #[test]
    fn workspace_id_is_deterministic_from_path() {
        assert_eq!(
            workspace_id_from_path(Path::new("/tmp/example-project")),
            "tmp::example-project"
        );
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
                archived: false,
                archived_at: None,
                archive_reason: None,
                stale: false,
            }],
        };
        write_workspace_registry(dir.path(), &registry).unwrap();
        let loaded = read_workspace_registry(dir.path()).unwrap().unwrap();
        assert_eq!(loaded, registry);
    }

    fn make_lease(path: &str, heartbeat: &str) -> WorkspaceLease {
        WorkspaceLease {
            project_id: "test-project".into(),
            workspace_id: "ws".into(),
            label: "test".into(),
            path: path.into(),
            backend_kind: WorkspaceBackendKind::LocalDir,
            vcs_ref: None,
            bindings: crate::workspace::types::WorkspaceBindings::default(),
            branch: "main".into(),
            role: WorkspaceRole::Primary,
            workspace_kind: WorkspaceKind::Mixed,
            mutability: Mutability::Mutable,
            owner_session_id: Some("s1".into()),
            owner_agent_id: None,
            created_at: current_timestamp(),
            last_heartbeat: heartbeat.into(),
            source: "test".into(),
            archived: false,
            archived_at: None,
            archive_reason: None,
            parent_workspace_id: None,
        }
    }

    #[test]
    fn instance_lease_path_is_namespaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = instance_lease_path(dir.path(), "tui-123");
        assert!(path.to_string_lossy().contains("tui-123"));
        assert!(path.to_string_lossy().ends_with("workspace.json"));
    }

    #[test]
    fn two_instances_write_separate_leases() {
        let dir = tempfile::tempdir().unwrap();
        let lease_a = make_lease("/a", &current_timestamp());
        let lease_b = make_lease("/b", &current_timestamp());

        write_workspace_lease(dir.path(), "tui-111", &lease_a).unwrap();
        write_workspace_lease(dir.path(), "acp-222", &lease_b).unwrap();

        let path_a = instance_lease_path(dir.path(), "tui-111");
        let path_b = instance_lease_path(dir.path(), "acp-222");
        assert!(path_a.exists());
        assert!(path_b.exists());
        assert_ne!(path_a, path_b);
    }

    #[test]
    fn read_all_active_leases_finds_both() {
        let dir = tempfile::tempdir().unwrap();
        let now = current_timestamp();
        let lease_a = make_lease("/a", &now);
        let lease_b = make_lease("/b", &now);

        write_workspace_lease(dir.path(), "tui-111", &lease_a).unwrap();
        write_workspace_lease(dir.path(), "acp-222", &lease_b).unwrap();

        let active = read_all_active_leases(dir.path());
        assert_eq!(active.len(), 2);
        let ids: Vec<&str> = active.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"tui-111"));
        assert!(ids.contains(&"acp-222"));
    }

    #[test]
    fn cleanup_instance_removes_dir() {
        let dir = tempfile::tempdir().unwrap();
        let lease = make_lease("/x", &current_timestamp());
        write_workspace_lease(dir.path(), "tui-999", &lease).unwrap();

        let inst_dir = crate::paths::runtime_instance_dir(dir.path(), "tui-999");
        assert!(inst_dir.is_dir());

        cleanup_instance(dir.path(), "tui-999");
        assert!(!inst_dir.exists());
    }

    #[test]
    fn prune_stale_removes_old_dirs() {
        let dir = tempfile::tempdir().unwrap();
        // Create a lease with a very old heartbeat (stale)
        let stale_lease = make_lease("/old", "2020-01-01T00:00:00Z");
        // pid 99999999 is almost certainly not running
        let inst_dir = crate::paths::runtime_instance_dir(dir.path(), "tui-99999999");
        std::fs::create_dir_all(&inst_dir).unwrap();
        let json = serde_json::to_string_pretty(&stale_lease).unwrap();
        std::fs::write(inst_dir.join("workspace.json"), json).unwrap();

        let pruned = prune_stale_instances(dir.path());
        assert!(pruned.contains(&"tui-99999999".to_string()));
        assert!(!inst_dir.exists());
    }

    #[test]
    fn read_workspace_lease_reads_instance_leases() {
        let dir = tempfile::tempdir().unwrap();
        let lease = make_lease("/test", &current_timestamp());
        write_workspace_lease(dir.path(), "tui-111", &lease).unwrap();

        let loaded = read_workspace_lease(dir.path()).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().path, "/test");
    }

    #[test]
    fn read_workspace_lease_falls_back_to_legacy() {
        let dir = tempfile::tempdir().unwrap();
        // Write to legacy path directly
        let rt = runtime_dir(dir.path());
        std::fs::create_dir_all(&rt).unwrap();
        let lease = make_lease("/legacy", &current_timestamp());
        let json = serde_json::to_string_pretty(&lease).unwrap();
        std::fs::write(rt.join("workspace.json"), json).unwrap();

        let loaded = read_workspace_lease(dir.path()).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().path, "/legacy");
    }
}

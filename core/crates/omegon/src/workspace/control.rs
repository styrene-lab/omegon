use std::path::{Path, PathBuf};

use omegon_traits::SlashCommandResponse;

use super::types::{
    AdmissionOutcome, Mutability, WorkspaceActionKind, WorkspaceAdmissionRequest,
    WorkspaceBackendKind, WorkspaceBindings, WorkspaceKind, WorkspaceLease, WorkspaceRegistry,
    WorkspaceRole, WorkspaceSummary, WorkspaceVcsRef,
};

pub struct WorkspaceControlContext<'a> {
    pub cwd: &'a Path,
    pub session_id: &'a str,
    pub instance_id: &'a str,
    pub owner_agent_id: &'a str,
}

impl<'a> WorkspaceControlContext<'a> {
    pub fn new(cwd: &'a Path, session_id: &'a str, instance_id: &'a str) -> Self {
        Self {
            cwd,
            session_id,
            instance_id,
            owner_agent_id: "omegon-local",
        }
    }

    pub fn with_owner_agent_id(mut self, owner_agent_id: &'a str) -> Self {
        self.owner_agent_id = owner_agent_id;
        self
    }
}

pub fn workspace_status_view_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let lease = super::runtime::read_workspace_lease(ctx.cwd).ok().flatten();
    let registry = super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten();

    let Some(lease) = lease else {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace: no local runtime metadata yet.".into()),
        };
    };

    let occupancy = registry
        .as_ref()
        .map(|registry| registry.workspaces.len())
        .unwrap_or(1);
    let owner = lease.owner_session_id.as_deref().unwrap_or("(none)");
    let milestone = lease.bindings.milestone_id.as_deref().unwrap_or("(none)");
    let node = lease.bindings.design_node_id.as_deref().unwrap_or("(none)");
    let change = lease
        .bindings
        .openspec_change
        .as_deref()
        .unwrap_or("(none)");
    let text = format!(
        "Workspace\n  ID:           {}\n  Label:        {}\n  Project:      {}\n  Path:         {}\n  Backend:      {}\n  Branch:       {}\n  Role:         {:?}\n  Kind:         {:?}\n  Mutability:   {:?}\n  Owner:        {}\n  Source:       {}\n  Milestone:    {}\n  Design Node:  {}\n  OpenSpec:     {}\n  Local Views:  {}",
        lease.workspace_id,
        lease.label,
        lease.project_id,
        lease.path,
        lease.backend_kind.as_str(),
        lease.branch,
        lease.role,
        lease.workspace_kind,
        lease.mutability,
        owner,
        lease.source,
        milestone,
        node,
        change,
        occupancy,
    );
    SlashCommandResponse {
        accepted: true,
        output: Some(text),
    }
}

pub fn workspace_list_view_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let registry = super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten();
    let Some(registry) = registry else {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace registry: no local runtime metadata yet.".into()),
        };
    };
    if registry.workspaces.is_empty() {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace registry is empty.".into()),
        };
    }
    let mut lines = vec![format!(
        "Workspaces\n  Project:      {}\n  Repo Root:    {}\n  Count:        {}\n",
        registry.project_id,
        registry.repo_root,
        registry.workspaces.len()
    )];
    for workspace in registry.workspaces {
        let owner = workspace.owner_session_id.as_deref().unwrap_or("(none)");
        let milestone = workspace
            .bindings
            .milestone_id
            .as_deref()
            .unwrap_or("(none)");
        let node = workspace
            .bindings
            .design_node_id
            .as_deref()
            .unwrap_or("(none)");
        let archive = if workspace.archived {
            format!(
                "archived at {} ({})",
                workspace.archived_at.as_deref().unwrap_or("unknown"),
                workspace.archive_reason.as_deref().unwrap_or("no reason")
            )
        } else {
            "active".to_string()
        };
        lines.push(format!(
            "- {} ({})\n    path: {}\n    backend: {}\n    branch: {}\n    role/kind: {:?} / {:?}\n    milestone/node: {} / {}\n    mutability: {:?}\n    owner: {}\n    archive: {}\n    stale: {}",
            workspace.workspace_id,
            workspace.label,
            workspace.path,
            workspace.backend_kind.as_str(),
            workspace.branch,
            workspace.role,
            workspace.workspace_kind,
            milestone,
            node,
            workspace.mutability,
            owner,
            archive,
            workspace.stale,
        ));
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(lines.join("\n")),
    }
}

pub fn workspace_role_view_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let lease = super::runtime::read_workspace_lease(ctx.cwd).ok().flatten();
    let Some(lease) = lease else {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace role: no lease metadata yet.".into()),
        };
    };
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Workspace Role\n  Current:      {}",
            lease.role.as_str(),
        )),
    }
}

pub fn workspace_role_set_response(
    ctx: &WorkspaceControlContext<'_>,
    role: WorkspaceRole,
) -> SlashCommandResponse {
    let mut lease = match super::runtime::read_workspace_lease(ctx.cwd).ok().flatten() {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace role cannot be set before workspace metadata exists.".into(),
                ),
            };
        }
    };
    lease.role = role;
    if let Err(err) = super::runtime::write_workspace_lease(ctx.cwd, ctx.instance_id, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to update workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.role = role;
            }
        }
        let _ = super::runtime::write_workspace_registry(ctx.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Workspace role set to {}.", role.as_str())),
    }
}

pub fn workspace_role_clear_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    if super::runtime::read_workspace_lease(ctx.cwd)
        .ok()
        .flatten()
        .is_none()
    {
        return SlashCommandResponse {
            accepted: false,
            output: Some(
                "Workspace role cannot be cleared before workspace metadata exists.".into(),
            ),
        };
    }
    workspace_role_set_response(ctx, WorkspaceRole::Primary)
        .map_output(|_| "Workspace role reset to primary.".to_string())
}

pub fn workspace_kind_view_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let lease = super::runtime::read_workspace_lease(ctx.cwd).ok().flatten();
    let inferred = super::infer::infer_workspace_kind(ctx.cwd);
    let Some(lease) = lease else {
        return SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace kind: no lease metadata yet. Inferred kind would be {}.",
                inferred.as_str()
            )),
        };
    };
    let declared = lease.workspace_kind;
    let source = if declared == inferred {
        "inferred/default"
    } else {
        "operator-declared"
    };
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Workspace Kind\n  Current:      {}\n  Inferred:     {}\n  Source:       {}",
            declared.as_str(),
            inferred.as_str(),
            source,
        )),
    }
}

pub fn workspace_kind_set_response(
    ctx: &WorkspaceControlContext<'_>,
    kind: WorkspaceKind,
) -> SlashCommandResponse {
    let mut lease = match super::runtime::read_workspace_lease(ctx.cwd).ok().flatten() {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace kind cannot be set before workspace metadata exists.".into(),
                ),
            };
        }
    };
    lease.workspace_kind = kind;
    if let Err(err) = super::runtime::write_workspace_lease(ctx.cwd, ctx.instance_id, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to update workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.workspace_kind = kind;
            }
        }
        let _ = super::runtime::write_workspace_registry(ctx.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Workspace kind set to {}.", kind.as_str())),
    }
}

pub fn workspace_kind_clear_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    if super::runtime::read_workspace_lease(ctx.cwd)
        .ok()
        .flatten()
        .is_none()
    {
        return SlashCommandResponse {
            accepted: false,
            output: Some(
                "Workspace kind cannot be cleared before workspace metadata exists.".into(),
            ),
        };
    }
    let inferred = super::infer::infer_workspace_kind(ctx.cwd);
    workspace_kind_set_response(ctx, inferred).map_output(|_| {
        format!(
            "Workspace kind reset to inferred value {}.",
            inferred.as_str()
        )
    })
}

pub fn workspace_bind_milestone_response(
    ctx: &WorkspaceControlContext<'_>,
    milestone_id: &str,
) -> SlashCommandResponse {
    match rewrite_current_workspace(ctx, |lease| {
        lease.bindings.milestone_id = Some(milestone_id.to_string());
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace {} ({}) bound to milestone {}.",
                lease.workspace_id, lease.label, milestone_id
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub fn workspace_bind_node_response(
    ctx: &WorkspaceControlContext<'_>,
    design_node_id: &str,
) -> SlashCommandResponse {
    match rewrite_current_workspace(ctx, |lease| {
        lease.bindings.design_node_id = Some(design_node_id.to_string());
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace {} ({}) bound to design node {}.",
                lease.workspace_id, lease.label, design_node_id
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub fn workspace_bind_clear_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    match rewrite_current_workspace(ctx, |lease| {
        lease.bindings = WorkspaceBindings::default();
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace {} ({}) lifecycle bindings cleared.",
                lease.workspace_id, lease.label
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub fn workspace_adopt_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let mut lease = match super::runtime::read_workspace_lease(ctx.cwd).ok().flatten() {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some("Workspace adopt requires existing local workspace metadata.".into()),
            };
        }
    };
    let heartbeat = super::runtime::heartbeat_epoch_secs(&lease.last_heartbeat);
    let now_epoch = chrono::Utc::now().timestamp();
    let request = WorkspaceAdmissionRequest {
        requested_role: lease.role,
        requested_kind: lease.workspace_kind,
        requested_mutability: lease.mutability,
        session_id: Some(ctx.session_id.to_string()),
        action: WorkspaceActionKind::SessionStart,
    };
    let outcome =
        super::admission::classify_admission(Some(&lease), &request, now_epoch, heartbeat);
    if !matches!(
        outcome,
        AdmissionOutcome::ConflictStaleLeaseAdoptable { .. }
    ) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Workspace adopt is only allowed for stale leases. Current admission state: {:?}",
                outcome
            )),
        };
    }
    lease.owner_session_id = Some(ctx.session_id.to_string());
    lease.owner_agent_id = Some(ctx.owner_agent_id.to_string());
    lease.last_heartbeat = super::runtime::current_timestamp();
    if let Err(err) = super::runtime::write_workspace_lease(ctx.cwd, ctx.instance_id, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to adopt workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.workspace_id == lease.workspace_id {
                workspace.owner_session_id = lease.owner_session_id.clone();
                workspace.last_heartbeat = lease.last_heartbeat.clone();
                workspace.stale = false;
            }
        }
        let _ = super::runtime::write_workspace_registry(ctx.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Adopted stale workspace lease for {} ({}).",
            lease.workspace_id, lease.label
        )),
    }
}

pub fn workspace_release_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let lease = match super::runtime::read_workspace_lease(ctx.cwd).ok().flatten() {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace release requires existing local workspace metadata.".into(),
                ),
            };
        }
    };
    if lease.owner_session_id.as_deref() != Some(ctx.session_id) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Workspace release requires current ownership. Current owner: {}",
                lease.owner_session_id.as_deref().unwrap_or("(none)")
            )),
        };
    }
    match rewrite_current_workspace(ctx, |lease| {
        lease.owner_session_id = None;
        lease.owner_agent_id = None;
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Released workspace {} ({}). It remains registered but is no longer owned.",
                lease.workspace_id, lease.label
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub fn workspace_archive_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let lease = match super::runtime::read_workspace_lease(ctx.cwd).ok().flatten() {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace archive requires existing local workspace metadata.".into(),
                ),
            };
        }
    };
    if lease.owner_session_id.is_some() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Workspace must be released before it can be archived.".into()),
        };
    }
    match rewrite_current_workspace(ctx, |lease| {
        lease.archived = true;
        lease.archived_at = Some(super::runtime::current_timestamp());
        lease.archive_reason = Some("operator".into());
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Archived workspace {} ({}). It remains on disk but is retired from active use.",
                lease.workspace_id, lease.label
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub fn workspace_prune_response(ctx: &WorkspaceControlContext<'_>) -> SlashCommandResponse {
    let mut registry = match super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten()
    {
        Some(registry) => registry,
        None => {
            return SlashCommandResponse {
                accepted: true,
                output: Some("Workspace registry: nothing to prune.".into()),
            };
        }
    };
    let before = registry.workspaces.len();
    registry.workspaces.retain(|workspace| {
        let path = Path::new(&workspace.path);
        path.exists() || !workspace.archived
    });
    let removed = before.saturating_sub(registry.workspaces.len());
    if let Err(err) = super::runtime::write_workspace_registry(ctx.cwd, &registry) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to write workspace registry: {err}")),
        };
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Pruned {} workspace registry entries.", removed)),
    }
}

pub fn workspace_destroy_response(
    ctx: &WorkspaceControlContext<'_>,
    target: &str,
) -> SlashCommandResponse {
    let registry = match super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten()
    {
        Some(registry) => registry,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some("Workspace destroy requires existing local registry metadata.".into()),
            };
        }
    };
    let workspace = match find_workspace_target(&registry, target) {
        Ok(workspace) => workspace,
        Err(message) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(message),
            };
        }
    };
    if workspace.path == ctx.cwd.display().to_string() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Refusing to destroy the current active workspace.".into()),
        };
    }
    if workspace.role == WorkspaceRole::Primary {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Refusing to destroy the primary workspace.".into()),
        };
    }
    if workspace.owner_session_id.is_some() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Workspace must be released before it can be destroyed.".into()),
        };
    }
    if !workspace.archived {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Workspace must be archived before it can be destroyed.".into()),
        };
    }

    let repo_root = Path::new(&registry.repo_root);
    let workspace_path = Path::new(&workspace.path);
    let removal = match workspace.backend_kind {
        WorkspaceBackendKind::GitWorktree | WorkspaceBackendKind::JjCheckout => {
            omegon_git::worktree::remove_smart(repo_root, &workspace.label, workspace_path)
                .map_err(|err| format!("Failed to remove workspace backend: {err}"))
        }
        WorkspaceBackendKind::LocalDir | WorkspaceBackendKind::GitClone => {
            safe_remove_workspace_dir(ctx.cwd, repo_root, workspace_path)
        }
        WorkspaceBackendKind::RemoteDir | WorkspaceBackendKind::PodVolume => Err(
            "Workspace destroy is not yet implemented for remote-dir/pod-volume backends.".into(),
        ),
    };
    if let Err(message) = removal {
        return SlashCommandResponse {
            accepted: false,
            output: Some(message),
        };
    }

    let mut updated = registry.clone();
    updated
        .workspaces
        .retain(|entry| entry.workspace_id != workspace.workspace_id);
    if let Err(err) = super::runtime::write_workspace_registry(ctx.cwd, &updated) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Destroyed workspace but failed to update registry: {err}"
            )),
        };
    }

    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Destroyed archived workspace {} ({}).",
            workspace.workspace_id, workspace.label
        )),
    }
}

pub fn workspace_new_response(
    ctx: &WorkspaceControlContext<'_>,
    label: &str,
) -> SlashCommandResponse {
    let project_root = crate::setup::find_project_root(ctx.cwd);
    let parent = match super::runtime::read_workspace_lease(ctx.cwd).ok().flatten() {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace creation requires existing local workspace metadata.".into(),
                ),
            };
        }
    };
    let sanitized = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase();
    if sanitized.is_empty() {
        return SlashCommandResponse {
            accepted: false,
            output: Some(
                "Workspace label must contain at least one alphanumeric character.".into(),
            ),
        };
    }
    let workspace_path = sibling_workspace_path(&project_root, &sanitized);
    let branch = format!("workspace/{}", sanitized);
    let info = match omegon_git::worktree::create_smart(
        &project_root,
        &workspace_path,
        &sanitized,
        &branch,
    ) {
        Ok(info) => info,
        Err(err) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Failed to create sibling workspace: {err}")),
            };
        }
    };
    let backend_kind = match info.backend {
        "jj" => WorkspaceBackendKind::JjCheckout,
        _ => WorkspaceBackendKind::GitWorktree,
    };
    let now = super::runtime::current_timestamp();
    let new_workspace_id = super::runtime::workspace_id_from_path(&workspace_path);
    let new_lease = WorkspaceLease {
        project_id: parent.project_id.clone(),
        workspace_id: new_workspace_id.clone(),
        label: sanitized.clone(),
        path: workspace_path.display().to_string(),
        backend_kind,
        vcs_ref: Some(WorkspaceVcsRef {
            vcs: if info.backend == "jj" {
                "jj".into()
            } else {
                "git".into()
            },
            branch: Some(info.branch.clone()),
            revision: None,
            remote: Some("origin".into()),
        }),
        bindings: parent.bindings.clone(),
        branch: info.branch.clone(),
        role: WorkspaceRole::Feature,
        workspace_kind: parent.workspace_kind,
        mutability: Mutability::Mutable,
        owner_session_id: None,
        owner_agent_id: None,
        created_at: now.clone(),
        last_heartbeat: now.clone(),
        archived: false,
        archived_at: None,
        archive_reason: None,
        parent_workspace_id: Some(parent.workspace_id.clone()),
        source: "operator".into(),
    };
    if let Err(err) =
        super::runtime::write_workspace_lease(&workspace_path, ctx.instance_id, &new_lease)
    {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Created workspace but failed to write lease metadata: {err}"
            )),
        };
    }
    let mut registry = super::runtime::read_workspace_registry(ctx.cwd)
        .ok()
        .flatten()
        .unwrap_or(WorkspaceRegistry {
            project_id: parent.project_id.clone(),
            repo_root: project_root.display().to_string(),
            workspaces: vec![],
        });
    registry.project_id = parent.project_id.clone();
    registry.repo_root = project_root.display().to_string();
    registry
        .workspaces
        .retain(|ws| ws.workspace_id != new_workspace_id);
    registry.workspaces.push(WorkspaceSummary {
        workspace_id: new_lease.workspace_id.clone(),
        label: new_lease.label.clone(),
        path: new_lease.path.clone(),
        backend_kind: new_lease.backend_kind,
        vcs_ref: new_lease.vcs_ref.clone(),
        bindings: new_lease.bindings.clone(),
        branch: new_lease.branch.clone(),
        role: new_lease.role,
        workspace_kind: new_lease.workspace_kind,
        mutability: new_lease.mutability,
        owner_session_id: new_lease.owner_session_id.clone(),
        last_heartbeat: new_lease.last_heartbeat.clone(),
        archived: new_lease.archived,
        archived_at: new_lease.archived_at.clone(),
        archive_reason: new_lease.archive_reason.clone(),
        stale: false,
    });
    let _ = super::runtime::write_workspace_registry(ctx.cwd, &registry);
    let _ = super::runtime::write_workspace_registry(&workspace_path, &registry);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Created sibling workspace '{}' at {} using {}.",
            sanitized,
            workspace_path.display(),
            backend_kind.as_str()
        )),
    }
}

fn rewrite_current_workspace<F>(
    ctx: &WorkspaceControlContext<'_>,
    mutator: F,
) -> Result<WorkspaceLease, String>
where
    F: FnOnce(&mut WorkspaceLease),
{
    let mut lease = super::runtime::read_workspace_lease(ctx.cwd)
        .map_err(|err| format!("Failed to read workspace lease: {err}"))?
        .ok_or_else(|| "Workspace metadata does not exist yet.".to_string())?;
    mutator(&mut lease);
    super::runtime::write_workspace_lease(ctx.cwd, ctx.instance_id, &lease)
        .map_err(|err| format!("Failed to update workspace lease: {err}"))?;
    if let Some(mut registry) = super::runtime::read_workspace_registry(ctx.cwd)
        .map_err(|err| format!("Failed to read workspace registry: {err}"))?
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.bindings = lease.bindings.clone();
                workspace.role = lease.role;
                workspace.workspace_kind = lease.workspace_kind;
                workspace.owner_session_id = lease.owner_session_id.clone();
                workspace.last_heartbeat = lease.last_heartbeat.clone();
                workspace.archived = lease.archived;
                workspace.archived_at = lease.archived_at.clone();
                workspace.archive_reason = lease.archive_reason.clone();
            }
        }
        super::runtime::write_workspace_registry(ctx.cwd, &registry)
            .map_err(|err| format!("Failed to update workspace registry: {err}"))?;
    }
    Ok(lease)
}

fn find_workspace_target(
    registry: &WorkspaceRegistry,
    target: &str,
) -> Result<WorkspaceSummary, String> {
    if let Some(workspace) = registry
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == target)
    {
        return Ok(workspace.clone());
    }

    let matches = registry
        .workspaces
        .iter()
        .filter(|workspace| workspace.label == target)
        .cloned()
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!("Workspace '{target}' not found in local registry.")),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(format!(
            "Workspace label '{target}' is ambiguous; use workspace_id instead."
        )),
    }
}

fn safe_remove_workspace_dir(
    current_cwd: &Path,
    repo_root: &Path,
    workspace_path: &Path,
) -> Result<(), String> {
    if workspace_path == current_cwd {
        return Err("Refusing to destroy the current active workspace path.".into());
    }
    if workspace_path == repo_root {
        return Err("Refusing to destroy the project root workspace.".into());
    }
    if workspace_path.exists() {
        std::fs::remove_dir_all(workspace_path)
            .map_err(|err| format!("Failed to remove workspace directory: {err}"))?;
    }
    Ok(())
}

fn sibling_workspace_path(project_root: &Path, sanitized: &str) -> PathBuf {
    project_root.parent().unwrap_or(project_root).join(format!(
        "{}-{}",
        project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace"),
        sanitized
    ))
}

trait SlashCommandResponseExt {
    fn map_output<F>(self, f: F) -> Self
    where
        F: FnOnce(String) -> String;
}

impl SlashCommandResponseExt for SlashCommandResponse {
    fn map_output<F>(mut self, f: F) -> Self
    where
        F: FnOnce(String) -> String,
    {
        if self.accepted
            && let Some(output) = self.output.take()
        {
            self.output = Some(f(output));
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lease(cwd: &Path, session_id: Option<&str>) -> WorkspaceLease {
        WorkspaceLease {
            project_id: "project".into(),
            workspace_id: super::super::runtime::workspace_id_from_path(cwd),
            label: "main".into(),
            path: cwd.display().to_string(),
            backend_kind: WorkspaceBackendKind::LocalDir,
            vcs_ref: None,
            bindings: WorkspaceBindings::default(),
            branch: "main".into(),
            role: WorkspaceRole::Primary,
            workspace_kind: WorkspaceKind::Code,
            mutability: Mutability::Mutable,
            owner_session_id: session_id.map(ToOwned::to_owned),
            owner_agent_id: session_id.map(|_| "omegon-local".to_string()),
            created_at: super::super::runtime::current_timestamp(),
            last_heartbeat: super::super::runtime::current_timestamp(),
            archived: false,
            archived_at: None,
            archive_reason: None,
            parent_workspace_id: None,
            source: "test".into(),
        }
    }

    fn test_registry(cwd: &Path, lease: &WorkspaceLease) -> WorkspaceRegistry {
        WorkspaceRegistry {
            project_id: lease.project_id.clone(),
            repo_root: cwd.display().to_string(),
            workspaces: vec![WorkspaceSummary {
                workspace_id: lease.workspace_id.clone(),
                label: lease.label.clone(),
                path: lease.path.clone(),
                backend_kind: lease.backend_kind,
                vcs_ref: lease.vcs_ref.clone(),
                bindings: lease.bindings.clone(),
                branch: lease.branch.clone(),
                role: lease.role,
                workspace_kind: lease.workspace_kind,
                mutability: lease.mutability,
                owner_session_id: lease.owner_session_id.clone(),
                last_heartbeat: lease.last_heartbeat.clone(),
                archived: lease.archived,
                archived_at: lease.archived_at.clone(),
                archive_reason: lease.archive_reason.clone(),
                stale: false,
            }],
        }
    }

    #[test]
    fn role_and_kind_mutations_update_lease_and_registry() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = WorkspaceControlContext::new(dir.path(), "session-1", "test-1");
        let lease = test_lease(dir.path(), Some("session-1"));
        super::super::runtime::write_workspace_lease(dir.path(), "test-1", &lease).unwrap();
        super::super::runtime::write_workspace_registry(
            dir.path(),
            &test_registry(dir.path(), &lease),
        )
        .unwrap();

        let role = workspace_role_set_response(&ctx, WorkspaceRole::Release);
        assert!(role.accepted);
        let kind = workspace_kind_set_response(&ctx, WorkspaceKind::Spec);
        assert!(kind.accepted);

        let updated = super::super::runtime::read_workspace_lease(dir.path())
            .unwrap()
            .unwrap();
        assert_eq!(updated.role, WorkspaceRole::Release);
        assert_eq!(updated.workspace_kind, WorkspaceKind::Spec);
        let registry = super::super::runtime::read_workspace_registry(dir.path())
            .unwrap()
            .unwrap();
        assert_eq!(registry.workspaces[0].role, WorkspaceRole::Release);
        assert_eq!(registry.workspaces[0].workspace_kind, WorkspaceKind::Spec);
    }

    #[test]
    fn release_requires_current_owner_then_archive_marks_workspace_retired() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = WorkspaceControlContext::new(dir.path(), "session-1", "test-1");
        let mut lease = test_lease(dir.path(), Some("other-session"));
        super::super::runtime::write_workspace_lease(dir.path(), "test-1", &lease).unwrap();
        super::super::runtime::write_workspace_registry(
            dir.path(),
            &test_registry(dir.path(), &lease),
        )
        .unwrap();

        let denied = workspace_release_response(&ctx);
        assert!(!denied.accepted);

        lease.owner_session_id = Some("session-1".into());
        super::super::runtime::write_workspace_lease(dir.path(), "test-1", &lease).unwrap();
        let released = workspace_release_response(&ctx);
        assert!(released.accepted);
        let archived = workspace_archive_response(&ctx);
        assert!(archived.accepted);
        let updated = super::super::runtime::read_workspace_lease(dir.path())
            .unwrap()
            .unwrap();
        assert!(updated.archived);
        assert!(updated.owner_session_id.is_none());
    }

    #[test]
    fn destroy_rejects_primary_current_or_unarchived_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = WorkspaceControlContext::new(dir.path(), "session-1", "test-1");
        let lease = test_lease(dir.path(), None);
        super::super::runtime::write_workspace_registry(
            dir.path(),
            &test_registry(dir.path(), &lease),
        )
        .unwrap();

        let denied = workspace_destroy_response(&ctx, "main");
        assert!(!denied.accepted);
        assert!(denied.output.unwrap().contains("current active workspace"));
    }
}

//! Project DashboardHandles + HarnessStatus into IpcStateSnapshot.

use omegon_traits::{
    IpcChangeSnapshot, IpcChildSnapshot, IpcCleaveSnapshot, IpcDesignCounts, IpcDesignTreeSnapshot,
    IpcFocusedNode, IpcHarnessSnapshot, IpcHealthSnapshot, IpcHealthState, IpcMemorySnapshot,
    IpcNodeBrief, IpcOpenSpecSnapshot, IpcProviderSnapshot, IpcSessionSnapshot, IpcStateSnapshot,
    OmegonAutonomyMode, OmegonControlPlane, OmegonDeploymentKind, OmegonIdentity,
    OmegonInstanceDescriptor, OmegonOwnerKind, OmegonOwnership, OmegonPlacement,
    OmegonPlacementKind, OmegonRole, OmegonRuntime, OmegonRuntimeHealth, OmegonRuntimeProfile,
};

use crate::tui::dashboard::{DashboardHandles, SharedSessionStats};

/// Build a full state snapshot from the shared dashboard handles.
/// Always returns a valid snapshot even if some handles are unavailable.
pub fn build_state_snapshot(
    handles: &DashboardHandles,
    omegon_version: &str,
    cwd: &str,
    started_at: &str,
    server_instance_id: &str,
    session_id: &str,
) -> IpcStateSnapshot {
    let session = project_session(handles, cwd, started_at, session_id);
    let design_tree = project_design_tree(handles);
    let openspec = project_openspec(handles);
    let cleave = project_cleave(handles);
    let harness = project_harness(handles);
    let health = project_health(handles);
    let instance = project_instance(
        handles,
        cwd,
        &session,
        &harness,
        &health,
        omegon_version,
        server_instance_id,
    );

    IpcStateSnapshot {
        schema_version: omegon_traits::IPC_PROTOCOL_VERSION,
        omegon_version: omegon_version.to_string(),
        instance,
        session,
        design_tree,
        openspec,
        cleave,
        harness,
        health,
    }
}

fn project_session(
    handles: &DashboardHandles,
    cwd: &str,
    started_at: &str,
    session_id: &str,
) -> IpcSessionSnapshot {
    let stats = handles
        .session
        .lock()
        .map(|s| SharedSessionStats {
            turns: s.turns,
            tool_calls: s.tool_calls,
            compactions: s.compactions,
            busy: s.busy,
        })
        .unwrap_or_default();

    let (git_branch, git_detached) = if let Some(ref h) = handles.harness
        && let Ok(s) = h.lock()
    {
        (s.git_branch.clone(), s.git_detached)
    } else {
        (None, false)
    };

    IpcSessionSnapshot {
        cwd: cwd.to_string(),
        pid: std::process::id(),
        started_at: started_at.to_string(),
        turns: stats.turns,
        tool_calls: stats.tool_calls,
        compactions: stats.compactions,
        busy: stats.busy,
        git_branch,
        git_detached,
        session_id: Some(session_id.to_string()),
    }
}

fn project_design_tree(handles: &DashboardHandles) -> IpcDesignTreeSnapshot {
    let Some(ref lp_lock) = handles.lifecycle else {
        return IpcDesignTreeSnapshot {
            counts: IpcDesignCounts::default(),
            focused: None,
            implementing: vec![],
            actionable: vec![],
            nodes: vec![],
        };
    };
    let Ok(lp) = lp_lock.lock() else {
        return IpcDesignTreeSnapshot {
            counts: IpcDesignCounts::default(),
            focused: None,
            implementing: vec![],
            actionable: vec![],
            nodes: vec![],
        };
    };

    use crate::lifecycle::types::NodeStatus;

    let all = lp.all_nodes();
    let mut counts = IpcDesignCounts::default();
    counts.total = all.len();

    let mut nodes = Vec::with_capacity(all.len());
    let mut implementing = vec![];
    let mut actionable = vec![];

    for node in all.values() {
        match node.status {
            NodeStatus::Seed => counts.seed += 1,
            NodeStatus::Exploring => counts.exploring += 1,
            NodeStatus::Resolved => counts.resolved += 1,
            NodeStatus::Decided => counts.decided += 1,
            NodeStatus::Implementing => counts.implementing += 1,
            NodeStatus::Implemented => counts.implemented += 1,
            NodeStatus::Blocked => counts.blocked += 1,
            NodeStatus::Deferred | NodeStatus::Archived => counts.deferred += 1,
        }
        counts.open_questions += node.open_questions.len();

        let brief = IpcNodeBrief {
            id: node.id.clone(),
            title: node.title.clone(),
            status: node.status.as_str().to_string(),
            parent: node.parent.clone(),
            open_questions: node.open_questions.len(),
            tags: node.tags.clone(),
        };

        if node.status == NodeStatus::Implementing {
            implementing.push(brief.clone());
        }
        if matches!(node.status, NodeStatus::Exploring | NodeStatus::Decided)
            && !node.open_questions.is_empty()
        {
            actionable.push(brief.clone());
        }
        nodes.push(brief);
    }

    let focused = lp
        .focused_node_id()
        .and_then(|id| lp.get_node(id))
        .map(|n| IpcFocusedNode {
            id: n.id.clone(),
            title: n.title.clone(),
            status: n.status.as_str().to_string(),
            open_questions: n.open_questions.clone(),
            decisions: 0,
            children: all
                .values()
                .filter(|c| c.parent.as_deref() == Some(&n.id))
                .count(),
        });

    IpcDesignTreeSnapshot {
        counts,
        focused,
        implementing,
        actionable,
        nodes,
    }
}

fn project_openspec(handles: &DashboardHandles) -> IpcOpenSpecSnapshot {
    let Some(ref lp_lock) = handles.lifecycle else {
        return IpcOpenSpecSnapshot {
            changes: vec![],
            total_tasks: 0,
            done_tasks: 0,
        };
    };
    let Ok(lp) = lp_lock.lock() else {
        return IpcOpenSpecSnapshot {
            changes: vec![],
            total_tasks: 0,
            done_tasks: 0,
        };
    };

    let changes: Vec<IpcChangeSnapshot> = lp
        .changes()
        .iter()
        .map(|c| IpcChangeSnapshot {
            name: c.name.clone(),
            stage: format!("{:?}", c.stage).to_lowercase(),
            total_tasks: c.total_tasks,
            done_tasks: c.done_tasks,
        })
        .collect();

    let total_tasks: usize = changes.iter().map(|c| c.total_tasks).sum();
    let done_tasks: usize = changes.iter().map(|c| c.done_tasks).sum();

    IpcOpenSpecSnapshot {
        changes,
        total_tasks,
        done_tasks,
    }
}

fn project_cleave(handles: &DashboardHandles) -> IpcCleaveSnapshot {
    let Some(ref cp_lock) = handles.cleave else {
        return IpcCleaveSnapshot {
            active: false,
            total_children: 0,
            completed: 0,
            failed: 0,
            children: vec![],
        };
    };
    let Ok(cp) = cp_lock.lock() else {
        return IpcCleaveSnapshot {
            active: false,
            total_children: 0,
            completed: 0,
            failed: 0,
            children: vec![],
        };
    };

    IpcCleaveSnapshot {
        active: cp.active,
        total_children: cp.total_children,
        completed: cp.completed,
        failed: cp.failed,
        children: cp
            .children
            .iter()
            .map(|c| IpcChildSnapshot {
                label: c.label.clone(),
                status: c.status.clone(),
                duration_secs: c.duration_secs,
            })
            .collect(),
    }
}

pub fn project_instance_descriptor(
    handles: &DashboardHandles,
    cwd: &str,
    session: &IpcSessionSnapshot,
    harness: &IpcHarnessSnapshot,
    health: &IpcHealthSnapshot,
    omegon_version: &str,
    server_instance_id: &str,
) -> OmegonInstanceDescriptor {
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("HOST").ok());
    let workspace_id = workspace_id_from_cwd(cwd);
    let auth = handles
        .harness
        .as_ref()
        .and_then(|lock| lock.lock().ok())
        .map(|h| (h.web_auth_mode.clone(), h.web_auth_source.clone()));

    OmegonInstanceDescriptor {
        schema_version: omegon_traits::IPC_PROTOCOL_VERSION,
        identity: OmegonIdentity {
            instance_id: server_instance_id.to_string(),
            workspace_id,
            session_id: session
                .session_id
                .clone()
                .unwrap_or_else(|| "detached".into()),
            role: OmegonRole::PrimaryDriver,
            profile: harness.runtime_profile.clone(),
        },
        ownership: OmegonOwnership {
            owner_kind: OmegonOwnerKind::Operator,
            owner_id: "local-terminal".into(),
            parent_instance_id: None,
        },
        placement: OmegonPlacement {
            kind: OmegonPlacementKind::LocalProcess,
            host,
            pid: Some(std::process::id()),
            cwd: cwd.to_string(),
            namespace: None,
            pod_name: None,
            container_name: None,
        },
        control_plane: OmegonControlPlane {
            server_instance_id: server_instance_id.to_string(),
            protocol_version: omegon_traits::IPC_PROTOCOL_VERSION,
            schema_version: omegon_traits::IPC_PROTOCOL_VERSION,
            omegon_version: omegon_version.to_string(),
            capabilities: omegon_traits::IpcCapability::v1_server_set()
                .into_iter()
                .map(str::to_string)
                .collect(),
            ipc_socket_path: Some(
                std::path::Path::new(cwd)
                    .join(".omegon")
                    .join("ipc.sock")
                    .display()
                    .to_string(),
            ),
            http_base: None,
            startup_url: None,
            state_url: None,
            ws_url: None,
            auth_mode: auth.as_ref().and_then(|(mode, _)| mode.clone()),
            auth_source: auth.as_ref().and_then(|(_, source)| source.clone()),
            http_transport_security: None,
            ws_transport_security: None,
        },
        runtime: OmegonRuntime {
            deployment_kind: OmegonDeploymentKind::InteractiveTui,
            runtime_mode: omegon_traits::OmegonRuntimeMode::Standalone,
            runtime_profile: OmegonRuntimeProfile::PrimaryInteractive,
            autonomy_mode: OmegonAutonomyMode::OperatorDriven,
            health: match health.state {
                IpcHealthState::Ready => OmegonRuntimeHealth::Ready,
                IpcHealthState::Degraded => OmegonRuntimeHealth::Degraded,
                IpcHealthState::Starting => OmegonRuntimeHealth::Starting,
                IpcHealthState::Failed => OmegonRuntimeHealth::Failed,
            },
            provider_ok: health.provider_ok,
            memory_ok: health.memory_ok,
            cleave_available: harness.cleave_available,
            queued_events: 0,
            transport_warnings: vec![],
            runtime_dir: None,
            context_class: Some(harness.context_class.clone()),
            thinking_level: Some(harness.thinking_level.clone()),
            capability_tier: Some(harness.capability_tier.clone()),
        },
    }
}

fn project_instance(
    handles: &DashboardHandles,
    cwd: &str,
    session: &IpcSessionSnapshot,
    harness: &IpcHarnessSnapshot,
    health: &IpcHealthSnapshot,
    omegon_version: &str,
    server_instance_id: &str,
) -> OmegonInstanceDescriptor {
    project_instance_descriptor(
        handles,
        cwd,
        session,
        harness,
        health,
        omegon_version,
        server_instance_id,
    )
}

fn workspace_id_from_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_matches('/');
    if trimmed.is_empty() {
        return "root".into();
    }
    trimmed.replace('/', "::")
}

fn project_harness(handles: &DashboardHandles) -> IpcHarnessSnapshot {
    let Some(ref h_lock) = handles.harness else {
        return IpcHarnessSnapshot {
            context_class: "Squad".into(),
            thinking_level: "Medium".into(),
            capability_tier: "victory".into(),
            runtime_profile: "primary-interactive".into(),
            autonomy_mode: "operator-driven".into(),
            memory_available: false,
            cleave_available: false,
            memory_warning: None,
            memory: IpcMemorySnapshot {
                active_facts: 0,
                project_facts: 0,
                working_facts: 0,
                episodes: 0,
            },
            providers: vec![],
            mcp_server_count: 0,
            mcp_tool_count: 0,
            active_persona: None,
            active_tone: None,
            active_delegate_count: 0,
        };
    };
    let Ok(h) = h_lock.lock() else {
        return IpcHarnessSnapshot {
            context_class: "Squad".into(),
            thinking_level: "Medium".into(),
            capability_tier: "victory".into(),
            runtime_profile: "primary-interactive".into(),
            autonomy_mode: "operator-driven".into(),
            memory_available: false,
            cleave_available: false,
            memory_warning: None,
            memory: IpcMemorySnapshot {
                active_facts: 0,
                project_facts: 0,
                working_facts: 0,
                episodes: 0,
            },
            providers: vec![],
            mcp_server_count: 0,
            mcp_tool_count: 0,
            active_persona: None,
            active_tone: None,
            active_delegate_count: 0,
        };
    };

    IpcHarnessSnapshot {
        context_class: h.context_class.clone(),
        thinking_level: h.thinking_level.clone(),
        capability_tier: h.capability_tier.clone(),
        runtime_profile: h.runtime_profile.as_str().to_string(),
        autonomy_mode: match h.autonomy_mode {
            omegon_traits::OmegonAutonomyMode::OperatorDriven => "operator-driven".into(),
            omegon_traits::OmegonAutonomyMode::GuardedAutonomous => "guarded-autonomous".into(),
            omegon_traits::OmegonAutonomyMode::Autonomous => "autonomous".into(),
        },
        memory_available: h.memory_available,
        cleave_available: h.cleave_available,
        memory_warning: h.memory_warning.clone(),
        memory: IpcMemorySnapshot {
            active_facts: h.memory.active_facts,
            project_facts: h.memory.project_facts,
            working_facts: h.memory.working_facts,
            episodes: h.memory.episodes,
        },
        providers: h
            .providers
            .iter()
            .map(|p| IpcProviderSnapshot {
                name: p.name.clone(),
                authenticated: p.authenticated,
                model: p.model.clone(),
                runtime_status: p.runtime_status.map(|s| format!("{:?}", s).to_lowercase()),
                recent_failure_count: p.recent_failure_count,
                last_failure_kind: p.last_failure_kind.clone(),
            })
            .collect(),
        mcp_server_count: h.mcp_servers.iter().filter(|s| s.connected).count(),
        mcp_tool_count: h.mcp_tool_count(),
        active_persona: h.active_persona.as_ref().map(|p| p.name.clone()),
        active_tone: h.active_tone.as_ref().map(|t| t.name.clone()),
        active_delegate_count: h.active_delegates.len(),
    }
}

fn project_health(handles: &DashboardHandles) -> IpcHealthSnapshot {
    let now = chrono::Utc::now().to_rfc3339();
    let (memory_ok, provider_ok) = if let Some(ref h_lock) = handles.harness
        && let Ok(h) = h_lock.lock()
    {
        let mem_ok = h.memory_available || h.memory_warning.is_none();
        let prov_ok = h.providers.iter().any(|p| {
            p.authenticated
                && !matches!(
                    p.runtime_status,
                    Some(crate::status::ProviderRuntimeStatus::Degraded)
                )
        });
        (mem_ok, prov_ok)
    } else {
        (true, false)
    };

    IpcHealthSnapshot {
        state: IpcHealthState::Ready,
        memory_ok,
        provider_ok,
        checked_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn build_state_snapshot_includes_instance_descriptor() {
        let handles = DashboardHandles {
            harness: Some(Arc::new(Mutex::new(crate::status::HarnessStatus {
                context_class: "Squad".into(),
                thinking_level: "high".into(),
                capability_tier: "victory".into(),
                runtime_profile: omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
                autonomy_mode: omegon_traits::OmegonAutonomyMode::OperatorDriven,
                memory_available: true,
                cleave_available: true,
                web_auth_mode: Some("ephemeral-bearer".into()),
                web_auth_source: Some("generated".into()),
                ..Default::default()
            }))),
            ..Default::default()
        };

        let snap = build_state_snapshot(
            &handles,
            "0.15.10-rc.15",
            "/tmp/example-project",
            "2026-04-05T12:00:00Z",
            "instance-123",
            "session-abc",
        );

        assert_eq!(snap.instance.identity.instance_id, "instance-123");
        assert_eq!(snap.instance.identity.session_id, "session-abc");
        assert_eq!(snap.instance.identity.workspace_id, "tmp::example-project");
        assert_eq!(snap.instance.identity.profile, "primary-interactive");
        assert_eq!(snap.harness.runtime_profile, "primary-interactive");
        assert_eq!(snap.harness.autonomy_mode, "operator-driven");
        assert_eq!(
            snap.instance.control_plane.server_instance_id,
            "instance-123"
        );
        assert_eq!(
            snap.instance.control_plane.schema_version,
            omegon_traits::IPC_PROTOCOL_VERSION
        );
        assert_eq!(snap.instance.control_plane.omegon_version, "0.15.10-rc.15");
        assert_eq!(snap.session.session_id.as_deref(), Some("session-abc"));
        assert_eq!(
            snap.instance.runtime.thinking_level.as_deref(),
            Some("high")
        );
    }
}

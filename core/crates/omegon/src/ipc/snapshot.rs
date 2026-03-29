//! Project DashboardHandles + HarnessStatus into IpcStateSnapshot.

use omegon_traits::{
    IpcCleaveSnapshot, IpcChildSnapshot, IpcDesignCounts, IpcDesignTreeSnapshot,
    IpcFocusedNode, IpcHealthSnapshot, IpcHealthState, IpcHarnessSnapshot, IpcMemorySnapshot,
    IpcNodeBrief, IpcOpenSpecSnapshot, IpcChangeSnapshot, IpcProviderSnapshot,
    IpcSessionSnapshot, IpcStateSnapshot,
};

use crate::tui::dashboard::DashboardHandles;

/// Build a full state snapshot from the shared dashboard handles.
/// Always returns a valid snapshot even if some handles are unavailable.
pub fn build_state_snapshot(
    handles: &DashboardHandles,
    omegon_version: &str,
    cwd: &str,
    started_at: &str,
) -> IpcStateSnapshot {
    let session = project_session(handles, cwd, started_at);
    let design_tree = project_design_tree(handles);
    let openspec = project_openspec(handles);
    let cleave = project_cleave(handles);
    let harness = project_harness(handles);
    let health = project_health(handles);

    IpcStateSnapshot {
        schema_version: omegon_traits::IPC_PROTOCOL_VERSION,
        omegon_version: omegon_version.to_string(),
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
) -> IpcSessionSnapshot {
    let (turns, tool_calls, compactions) =
        if let Ok(s) = handles.session.lock() {
            (s.turns, s.tool_calls, s.compactions)
        } else {
            (0, 0, 0)
        };

    let (git_branch, git_detached) =
        if let Some(ref h) = handles.harness
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
        turns,
        tool_calls,
        compactions,
        busy: false, // populated by IpcConnection per-request
        git_branch,
        git_detached,
        session_id: None,
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
            NodeStatus::Deferred => counts.deferred += 1,
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

    let focused = lp.focused_node_id().and_then(|id| lp.get_node(id)).map(|n| {
        IpcFocusedNode {
            id: n.id.clone(),
            title: n.title.clone(),
            status: n.status.as_str().to_string(),
            open_questions: n.open_questions.clone(),
            decisions: 0,
            children: all.values().filter(|c| c.parent.as_deref() == Some(&n.id)).count(),
        }
    });

    IpcDesignTreeSnapshot { counts, focused, implementing, actionable, nodes }
}

fn project_openspec(handles: &DashboardHandles) -> IpcOpenSpecSnapshot {
    let Some(ref lp_lock) = handles.lifecycle else {
        return IpcOpenSpecSnapshot { changes: vec![], total_tasks: 0, done_tasks: 0 };
    };
    let Ok(lp) = lp_lock.lock() else {
        return IpcOpenSpecSnapshot { changes: vec![], total_tasks: 0, done_tasks: 0 };
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

    IpcOpenSpecSnapshot { changes, total_tasks, done_tasks }
}

fn project_cleave(handles: &DashboardHandles) -> IpcCleaveSnapshot {
    let Some(ref cp_lock) = handles.cleave else {
        return IpcCleaveSnapshot { active: false, total_children: 0, completed: 0, failed: 0, children: vec![] };
    };
    let Ok(cp) = cp_lock.lock() else {
        return IpcCleaveSnapshot { active: false, total_children: 0, completed: 0, failed: 0, children: vec![] };
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

fn project_harness(handles: &DashboardHandles) -> IpcHarnessSnapshot {
    let Some(ref h_lock) = handles.harness else {
        return IpcHarnessSnapshot {
            context_class: "Squad".into(),
            thinking_level: "Medium".into(),
            capability_tier: "victory".into(),
            memory_available: false,
            cleave_available: false,
            memory_warning: None,
            memory: IpcMemorySnapshot { active_facts: 0, project_facts: 0, working_facts: 0, episodes: 0 },
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
            memory_available: false,
            cleave_available: false,
            memory_warning: None,
            memory: IpcMemorySnapshot { active_facts: 0, project_facts: 0, working_facts: 0, episodes: 0 },
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
    let (memory_ok, provider_ok) =
        if let Some(ref h_lock) = handles.harness
            && let Ok(h) = h_lock.lock()
        {
            let mem_ok = h.memory_available || h.memory_warning.is_none();
            let prov_ok = h.providers.iter().any(|p| p.authenticated);
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


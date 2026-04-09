//! JSON API endpoints for the web dashboard.
//!
//! GET /api/state — full agent state snapshot.
//! Designed to be the canonical state shape that any web UI consumes.

use crate::status::HarnessStatus;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use omegon_traits::{DaemonEventEnvelope, OmegonInstanceDescriptor};
use serde::Serialize;

use super::{ControlPlaneState, WebState};
use crate::lifecycle::types::*;

/// Full agent state snapshot — the canonical shape for web consumers.
#[derive(Serialize)]
pub struct StateSnapshot {
    pub instance: OmegonInstanceDescriptor,
    pub design: DesignSnapshot,
    pub openspec: OpenSpecSnapshot,
    pub cleave: CleaveSnapshot,
    pub session: SessionSnapshot,
    pub harness: Option<HarnessStatus>,
}

#[derive(Serialize)]
pub struct DesignSnapshot {
    pub counts: DesignCounts,
    pub focused: Option<FocusedNode>,
    pub implementing: Vec<NodeBrief>,
    pub actionable: Vec<NodeBrief>,
    pub all_nodes: Vec<NodeBrief>,
}

#[derive(Serialize)]
pub struct DesignCounts {
    pub total: usize,
    pub seed: usize,
    pub exploring: usize,
    pub resolved: usize,
    pub decided: usize,
    pub implementing: usize,
    pub implemented: usize,
    pub blocked: usize,
    pub deferred: usize,
    pub open_questions: usize,
}

#[derive(Serialize)]
pub struct FocusedNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub open_questions: Vec<String>,
    pub decisions: usize,
    pub children: usize,
}

#[derive(Clone, Serialize)]
pub struct NodeBrief {
    pub id: String,
    pub title: String,
    pub status: String,
    pub parent: Option<String>,
    pub open_questions: usize,
    pub openspec_change: Option<String>,
    pub dependencies: Vec<String>,
    pub branches: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Serialize)]
pub struct OpenSpecSnapshot {
    pub changes: Vec<ChangeSnapshot>,
    pub total_tasks: usize,
    pub done_tasks: usize,
}

#[derive(Serialize)]
pub struct ChangeSnapshot {
    pub name: String,
    pub stage: String,
    pub has_specs: bool,
    pub has_tasks: bool,
    pub total_tasks: usize,
    pub done_tasks: usize,
}

#[derive(Serialize)]
pub struct CleaveSnapshot {
    pub active: bool,
    pub total_children: usize,
    pub completed: usize,
    pub failed: usize,
    pub children: Vec<ChildSnapshot>,
}

#[derive(Serialize)]
pub struct ChildRuntimeSnapshot {
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub context_class: Option<String>,
    pub enabled_tools: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub skills: Vec<String>,
    pub enabled_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
    pub preloaded_files: Vec<String>,
}

#[derive(Serialize)]
pub struct ChildSnapshot {
    pub label: String,
    pub status: String,
    pub duration_secs: Option<f64>,
    pub runtime: Option<ChildRuntimeSnapshot>,
}

#[derive(Serialize)]
pub struct SessionSnapshot {
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
}

/// Graph data for force-directed visualization.
#[derive(Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
}

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub group: u8, // 0=seed, 1=exploring, 2=decided, 3=implementing, 4=implemented, 5=blocked
    pub questions: usize,
    pub has_openspec: bool,
}

#[derive(Serialize)]
pub struct GraphLink {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub link_type: String, // "parent", "dependency", "related"
}

#[derive(Serialize)]
pub struct ProbeResponse {
    pub ok: bool,
    pub state: ControlPlaneState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct EventAccepted {
    pub accepted: bool,
    pub queued_events: usize,
}

/// GET /api/startup — machine-readable dashboard startup/discovery metadata.
pub async fn get_startup(
    State(state): State<WebState>,
) -> Result<Json<super::WebStartupInfo>, StatusCode> {
    match state.startup_info.lock() {
        Ok(guard) => guard
            .clone()
            .map(Json)
            .ok_or(StatusCode::SERVICE_UNAVAILABLE),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /api/healthz — control-plane liveness probe.
pub async fn get_health(State(state): State<WebState>) -> (StatusCode, Json<ProbeResponse>) {
    match state.control_plane_state.lock() {
        Ok(guard) => (
            StatusCode::OK,
            Json(ProbeResponse {
                ok: true,
                state: *guard,
            }),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProbeResponse {
                ok: false,
                state: ControlPlaneState::Failed,
            }),
        ),
    }
}

/// GET /api/readyz — control-plane readiness probe.
pub async fn get_ready(State(state): State<WebState>) -> (StatusCode, Json<ProbeResponse>) {
    match state.control_plane_state.lock() {
        Ok(guard) => {
            let is_ready = matches!(*guard, ControlPlaneState::Ready);
            (
                if is_ready {
                    StatusCode::OK
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                },
                Json(ProbeResponse {
                    ok: is_ready,
                    state: *guard,
                }),
            )
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProbeResponse {
                ok: false,
                state: ControlPlaneState::Failed,
            }),
        ),
    }
}

/// POST /api/events — authenticated local event ingress for daemon/runtime triggers.
pub async fn post_event(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(event): Json<DaemonEventEnvelope>,
) -> (StatusCode, Json<EventAccepted>) {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    if !state.web_auth.verify_query_token(bearer) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(EventAccepted {
                accepted: false,
                queued_events: 0,
            }),
        );
    }

    match state.daemon_events.lock() {
        Ok(mut queue) => {
            queue.push(event);
            let queued_events = queue.len();
            if let Ok(mut status) = state.daemon_status.lock() {
                status.queued_events = queued_events;
            }
            (
                StatusCode::ACCEPTED,
                Json(EventAccepted {
                    accepted: true,
                    queued_events,
                }),
            )
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(EventAccepted {
                accepted: false,
                queued_events: 0,
            }),
        ),
    }
}

/// GET /api/graph — graph data for force-directed layout.
pub async fn get_graph(State(state): State<WebState>) -> Json<GraphData> {
    Json(build_graph_data(&state.handles))
}

pub fn build_graph_data(handles: &crate::tui::dashboard::DashboardHandles) -> GraphData {
    let mut nodes = Vec::new();
    let mut links = Vec::new();

    if let Some(ref lp_lock) = handles.lifecycle
        && let Ok(lp) = lp_lock.lock()
    {
        let all = lp.all_nodes();
        for node in all.values() {
            let group = match node.status {
                NodeStatus::Seed => 0,
                NodeStatus::Exploring => 1,
                NodeStatus::Resolved | NodeStatus::Decided => 2,
                NodeStatus::Implementing => 3,
                NodeStatus::Implemented => 4,
                NodeStatus::Blocked => 5,
                NodeStatus::Deferred | NodeStatus::Archived => 6,
            };
            nodes.push(GraphNode {
                id: node.id.clone(),
                title: node.title.clone(),
                status: node.status.as_str().to_string(),
                group,
                questions: node.open_questions.len(),
                has_openspec: node.openspec_change.is_some(),
            });

            if let Some(ref parent) = node.parent {
                links.push(GraphLink {
                    source: parent.clone(),
                    target: node.id.clone(),
                    link_type: "parent".into(),
                });
            }
            for dep in &node.dependencies {
                links.push(GraphLink {
                    source: dep.clone(),
                    target: node.id.clone(),
                    link_type: "dependency".into(),
                });
            }
        }
    }

    GraphData { nodes, links }
}

/// GET /api/state — build a full snapshot from the shared handles.
pub async fn get_state(State(state): State<WebState>) -> Json<StateSnapshot> {
    let snapshot = build_snapshot(&state);
    Json(snapshot)
}

/// Build a StateSnapshot from the shared handles.
/// Also used by the WebSocket handler for initial snapshots.
pub fn build_snapshot(state: &WebState) -> StateSnapshot {
    let mut design = DesignSnapshot {
        counts: DesignCounts {
            total: 0,
            seed: 0,
            exploring: 0,
            resolved: 0,
            decided: 0,
            implementing: 0,
            implemented: 0,
            blocked: 0,
            deferred: 0,
            open_questions: 0,
        },
        focused: None,
        implementing: Vec::new(),
        actionable: Vec::new(),
        all_nodes: Vec::new(),
    };

    let mut openspec = OpenSpecSnapshot {
        changes: Vec::new(),
        total_tasks: 0,
        done_tasks: 0,
    };

    // Read lifecycle state
    if let Some(ref lp_lock) = state.handles.lifecycle
        && let Ok(lp) = lp_lock.lock()
    {
        let nodes = lp.all_nodes();
        design.counts.total = nodes.len();

        for node in nodes.values() {
            match node.status {
                NodeStatus::Seed => design.counts.seed += 1,
                NodeStatus::Exploring => design.counts.exploring += 1,
                NodeStatus::Resolved => design.counts.resolved += 1,
                NodeStatus::Decided => design.counts.decided += 1,
                NodeStatus::Implementing => design.counts.implementing += 1,
                NodeStatus::Implemented => design.counts.implemented += 1,
                NodeStatus::Blocked => design.counts.blocked += 1,
                NodeStatus::Deferred | NodeStatus::Archived => design.counts.deferred += 1,
            }
            design.counts.open_questions += node.open_questions.len();

            let brief = NodeBrief {
                id: node.id.clone(),
                title: node.title.clone(),
                status: node.status.as_str().to_string(),
                parent: node.parent.clone(),
                open_questions: node.open_questions.len(),
                openspec_change: node.openspec_change.clone(),
                dependencies: node.dependencies.clone(),
                branches: node.branches.clone(),
                tags: node.tags.clone(),
            };

            if matches!(node.status, NodeStatus::Implementing) {
                design.implementing.push(brief.clone());
            }
            if matches!(node.status, NodeStatus::Seed | NodeStatus::Exploring)
                && !node.open_questions.is_empty()
            {
                design.actionable.push(brief.clone());
            }
            design.all_nodes.push(brief);
        }

        // Focused node
        if let Some(id) = lp.focused_node_id()
            && let Some(node) = lp.get_node(id)
        {
            let sections = crate::lifecycle::design::read_node_sections(node);
            let children = crate::lifecycle::design::get_children(nodes, id);
            design.focused = Some(FocusedNode {
                id: node.id.clone(),
                title: node.title.clone(),
                status: node.status.as_str().to_string(),
                open_questions: node.open_questions.clone(),
                decisions: sections.map(|s| s.decisions.len()).unwrap_or(0),
                children: children.len(),
            });
        }

        // OpenSpec changes
        for change in lp.changes() {
            if matches!(change.stage, ChangeStage::Archived) {
                continue;
            }
            openspec.total_tasks += change.total_tasks;
            openspec.done_tasks += change.done_tasks;
            openspec.changes.push(ChangeSnapshot {
                name: change.name.clone(),
                stage: change.stage.as_str().to_string(),
                has_specs: change.has_specs,
                has_tasks: change.has_tasks,
                total_tasks: change.total_tasks,
                done_tasks: change.done_tasks,
            });
        }
    }

    // Read cleave state
    let cleave = if let Some(ref cp_lock) = state.handles.cleave {
        if let Ok(cp) = cp_lock.lock() {
            CleaveSnapshot {
                active: cp.active,
                total_children: cp.total_children,
                completed: cp.completed,
                failed: cp.failed,
                children: cp
                    .children
                    .iter()
                    .map(|c| ChildSnapshot {
                        label: c.label.clone(),
                        status: c.status.clone(),
                        duration_secs: c.duration_secs,
                        runtime: c.runtime.as_ref().map(|runtime| ChildRuntimeSnapshot {
                            model: runtime.model.clone(),
                            thinking_level: runtime.thinking_level.clone(),
                            context_class: runtime.context_class.clone(),
                            enabled_tools: runtime.enabled_tools.clone(),
                            disabled_tools: runtime.disabled_tools.clone(),
                            skills: runtime.skills.clone(),
                            enabled_extensions: runtime.enabled_extensions.clone(),
                            disabled_extensions: runtime.disabled_extensions.clone(),
                            preloaded_files: runtime.preloaded_files.clone(),
                        }),
                    })
                    .collect(),
            }
        } else {
            CleaveSnapshot {
                active: false,
                total_children: 0,
                completed: 0,
                failed: 0,
                children: Vec::new(),
            }
        }
    } else {
        CleaveSnapshot {
            active: false,
            total_children: 0,
            completed: 0,
            failed: 0,
            children: Vec::new(),
        }
    };

    // Read session stats from shared handle
    let session = if let Ok(ss) = state.handles.session.lock() {
        SessionSnapshot {
            turns: ss.turns,
            tool_calls: ss.tool_calls,
            compactions: ss.compactions,
        }
    } else {
        SessionSnapshot {
            turns: 0,
            tool_calls: 0,
            compactions: 0,
        }
    };

    let harness = state
        .handles
        .harness
        .as_ref()
        .and_then(|h| h.lock().ok().map(|guard| guard.clone()));

    let instance = state
        .startup_info
        .lock()
        .ok()
        .and_then(|guard| {
            guard
                .as_ref()
                .and_then(|startup| startup.instance_descriptor.clone())
        })
        .unwrap_or_else(|| {
            let cwd = std::env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            let session = omegon_traits::IpcSessionSnapshot {
                cwd: cwd.clone(),
                pid: std::process::id(),
                started_at: chrono::Utc::now().to_rfc3339(),
                turns: session.turns,
                tool_calls: session.tool_calls,
                compactions: session.compactions,
                busy: state
                    .handles
                    .session
                    .lock()
                    .map(|s| s.busy)
                    .unwrap_or(false),
                git_branch: harness.as_ref().and_then(|h| h.git_branch.clone()),
                git_detached: harness.as_ref().is_some_and(|h| h.git_detached),
                session_id: None,
            };
            let harness_projection = omegon_traits::IpcHarnessSnapshot {
                context_class: harness
                    .as_ref()
                    .map(|h| h.context_class.clone())
                    .unwrap_or_else(|| "Squad".into()),
                thinking_level: harness
                    .as_ref()
                    .map(|h| h.thinking_level.clone())
                    .unwrap_or_else(|| "Medium".into()),
                capability_tier: harness
                    .as_ref()
                    .map(|h| h.capability_tier.clone())
                    .unwrap_or_else(|| "victory".into()),
                runtime_profile: harness
                    .as_ref()
                    .map(|h| h.runtime_profile.as_str().to_string())
                    .unwrap_or_else(|| "primary-interactive".into()),
                autonomy_mode: harness
                    .as_ref()
                    .map(|h| match h.autonomy_mode {
                        omegon_traits::OmegonAutonomyMode::OperatorDriven => "operator-driven".to_string(),
                        omegon_traits::OmegonAutonomyMode::GuardedAutonomous => "guarded-autonomous".to_string(),
                        omegon_traits::OmegonAutonomyMode::Autonomous => "autonomous".to_string(),
                    })
                    .unwrap_or_else(|| "operator-driven".into()),
                memory_available: harness.as_ref().is_some_and(|h| h.memory_available),
                cleave_available: harness.as_ref().is_some_and(|h| h.cleave_available),
                memory_warning: harness.as_ref().and_then(|h| h.memory_warning.clone()),
                memory: omegon_traits::IpcMemorySnapshot {
                    active_facts: harness.as_ref().map(|h| h.memory.active_facts).unwrap_or(0),
                    project_facts: harness
                        .as_ref()
                        .map(|h| h.memory.project_facts)
                        .unwrap_or(0),
                    working_facts: harness
                        .as_ref()
                        .map(|h| h.memory.working_facts)
                        .unwrap_or(0),
                    episodes: harness.as_ref().map(|h| h.memory.episodes).unwrap_or(0),
                },
                providers: vec![],
                mcp_server_count: harness
                    .as_ref()
                    .map(|h| h.mcp_servers.iter().filter(|s| s.connected).count())
                    .unwrap_or(0),
                mcp_tool_count: harness.as_ref().map(|h| h.mcp_tool_count()).unwrap_or(0),
                active_persona: harness
                    .as_ref()
                    .and_then(|h| h.active_persona.as_ref().map(|p| p.name.clone())),
                active_tone: harness
                    .as_ref()
                    .and_then(|h| h.active_tone.as_ref().map(|t| t.name.clone())),
                active_delegate_count: harness
                    .as_ref()
                    .map(|h| h.active_delegates.len())
                    .unwrap_or(0),
            };
            let health = omegon_traits::IpcHealthSnapshot {
                state: omegon_traits::IpcHealthState::Ready,
                memory_ok: harness_projection.memory_available
                    || harness_projection.memory_warning.is_none(),
                provider_ok: harness
                    .as_ref()
                    .is_some_and(|h| h.providers.iter().any(|p| p.authenticated)),
                checked_at: chrono::Utc::now().to_rfc3339(),
            };
            crate::ipc::snapshot::project_instance_descriptor(
                &state.handles,
                &cwd,
                &session,
                &harness_projection,
                &health,
                env!("CARGO_PKG_VERSION"),
                "web-compat",
            )
        });

    StateSnapshot {
        instance,
        design,
        openspec,
        cleave,
        session,
        harness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::dashboard::DashboardHandles;
    use crate::web::{ControlPlaneState, WebAuthState, WebDaemonStatus, WebStartupInfo};
    use std::sync::{Arc, Mutex};

    fn test_state() -> WebState {
        WebState {
            handles: DashboardHandles::default(),
            events_tx: tokio::sync::broadcast::channel(16).0,
            command_tx: tokio::sync::mpsc::channel(16).0,
            web_auth: std::sync::Arc::new(WebAuthState::ephemeral_generated("test".into())),
            startup_info: std::sync::Arc::new(std::sync::Mutex::new(Some(WebStartupInfo {
                schema_version: 2,
                addr: "127.0.0.1:7842".into(),
                http_base: "http://127.0.0.1:7842".into(),
                state_url: "http://127.0.0.1:7842/api/state".into(),
                startup_url: "http://127.0.0.1:7842/api/startup".into(),
                health_url: "http://127.0.0.1:7842/api/healthz".into(),
                ready_url: "http://127.0.0.1:7842/api/readyz".into(),
                ws_url: "ws://127.0.0.1:7842/ws?token=test".into(),
                token: "test".into(),
                auth_mode: "ephemeral-bearer".into(),
                auth_source: "generated".into(),
                control_plane_state: ControlPlaneState::Ready,
                daemon_status: WebDaemonStatus::default(),
                instance_descriptor: None,
            }))),
            control_plane_state: std::sync::Arc::new(std::sync::Mutex::new(
                ControlPlaneState::Ready,
            )),
            daemon_events: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            daemon_status: std::sync::Arc::new(std::sync::Mutex::new(WebDaemonStatus::default())),
        }
    }

    #[test]
    fn empty_snapshot() {
        let snap = build_snapshot(&test_state());
        assert_eq!(snap.design.counts.total, 0);
        assert!(snap.design.focused.is_none());
        assert!(snap.openspec.changes.is_empty());
        assert!(!snap.cleave.active);
        assert!(snap.harness.is_none());
        assert_eq!(snap.instance.identity.instance_id, "web-compat");
    }

    #[test]
    fn snapshot_includes_harness_when_available() {
        let mut state = test_state();
        state.handles = DashboardHandles {
            harness: Some(Arc::new(Mutex::new(crate::status::HarnessStatus {
                thinking_level: "high".into(),
                capability_tier: "victory".into(),
                memory_available: true,
                cleave_available: true,
                ..Default::default()
            }))),
            ..Default::default()
        };

        let snap = build_snapshot(&state);
        let harness = snap.harness.expect("harness snapshot");
        assert_eq!(harness.thinking_level, "high");
        assert_eq!(harness.capability_tier, "victory");
        assert!(harness.memory_available);
        assert!(harness.cleave_available);
    }

    #[tokio::test]
    async fn startup_payload_is_available() {
        let payload = get_startup(axum::extract::State(test_state()))
            .await
            .unwrap()
            .0;

        assert_eq!(payload.schema_version, 2);
        assert_eq!(payload.state_url, "http://127.0.0.1:7842/api/state");
        assert_eq!(payload.health_url, "http://127.0.0.1:7842/api/healthz");
        assert_eq!(payload.ready_url, "http://127.0.0.1:7842/api/readyz");
        assert_eq!(payload.auth_mode, "ephemeral-bearer");
        assert_eq!(payload.daemon_status.queued_events, 0);
        assert!(payload.daemon_status.transport_warnings.is_empty());
        assert!(payload.instance_descriptor.is_none());
    }

    #[test]
    fn fallback_instance_descriptor_carries_control_plane_version_identity() {
        let snap = build_snapshot(&test_state());
        assert_eq!(
            snap.instance.control_plane.schema_version,
            omegon_traits::IPC_PROTOCOL_VERSION
        );
        assert_eq!(
            snap.instance.control_plane.omegon_version,
            env!("CARGO_PKG_VERSION")
        );
    }

    #[tokio::test]
    async fn health_probe_reports_alive() {
        let (status, Json(payload)) = get_health(axum::extract::State(test_state())).await;
        assert_eq!(status, StatusCode::OK);
        assert!(payload.ok);
        assert_eq!(payload.state, ControlPlaneState::Ready);
    }

    #[tokio::test]
    async fn ready_probe_reports_ready() {
        let (status, Json(payload)) = get_ready(axum::extract::State(test_state())).await;
        assert_eq!(status, StatusCode::OK);
        assert!(payload.ok);
        assert_eq!(payload.state, ControlPlaneState::Ready);
    }

    #[tokio::test]
    async fn post_event_requires_bearer_token() {
        let headers = HeaderMap::new();
        let event = DaemonEventEnvelope {
            event_id: "evt-1".into(),
            source: "manual/test".into(),
            trigger_kind: "manual".into(),
            payload: serde_json::json!({"ok": true}),
        };
        let (status, Json(payload)) = post_event(
            axum::extract::State(test_state()),
            headers,
            Json(event),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!payload.accepted);
    }

    #[tokio::test]
    async fn post_event_accepts_valid_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let state = test_state();
        let event = DaemonEventEnvelope {
            event_id: "evt-1".into(),
            source: "manual/test".into(),
            trigger_kind: "manual".into(),
            payload: serde_json::json!({"ok": true}),
        };
        let (status, Json(payload)) = post_event(axum::extract::State(state.clone()), headers, Json(event)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(payload.accepted);
        assert_eq!(payload.queued_events, 1);
        assert_eq!(state.daemon_events.lock().unwrap().len(), 1);
        assert_eq!(state.daemon_status.lock().unwrap().queued_events, 1);
    }

    #[test]
    fn build_snapshot_includes_child_runtime_profile() {
        let runtime = crate::features::cleave::ChildRuntimeSummary {
            model: Some("anthropic:claude-sonnet-4-6".into()),
            thinking_level: Some("high".into()),
            context_class: Some("legion".into()),
            enabled_tools: vec!["read".into()],
            disabled_tools: vec!["bash".into()],
            skills: vec!["security".into()],
            enabled_extensions: vec!["alpha".into()],
            disabled_extensions: vec!["beta".into()],
            preloaded_files: vec!["docs/runtime-preload.md".into()],
        };
        let mut state = test_state();
        state.handles = DashboardHandles {
            cleave: Some(std::sync::Arc::new(std::sync::Mutex::new(
                crate::features::cleave::CleaveProgress {
                    active: true,
                    run_id: "run-1".into(),
                    total_children: 1,
                    completed: 0,
                    failed: 0,
                    children: vec![crate::features::cleave::ChildProgress {
                        label: "child-1".into(),
                        status: "running".into(),
                        duration_secs: None,
                        supervision_mode: None,
                        pid: None,
                        last_tool: None,
                        last_turn: None,
                        started_at: None,
                        last_activity_at: None,
                        tokens_in: 0,
                        tokens_out: 0,
                        runtime: Some(runtime),
                    }],
                    total_tokens_in: 0,
                    total_tokens_out: 0,
                },
            ))),
            ..DashboardHandles::default()
        };

        let snap = build_snapshot(&state);
        let child = &snap.cleave.children[0];
        let runtime = child.runtime.as_ref().expect("runtime should be present");
        assert_eq!(runtime.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(runtime.context_class.as_deref(), Some("legion"));
        assert_eq!(runtime.disabled_tools, vec!["bash"]);
        assert_eq!(runtime.enabled_extensions, vec!["alpha"]);
        assert_eq!(runtime.preloaded_files, vec!["docs/runtime-preload.md"]);
    }

    #[test]
    fn graph_node_serializes() {
        let node = GraphNode {
            id: "test".into(),
            title: "Test".into(),
            status: "exploring".into(),
            group: 1,
            questions: 2,
            has_openspec: false,
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("\"group\":1"));
        assert!(json.contains("\"questions\":2"));
    }

    #[test]
    fn graph_link_type_field_name() {
        let link = GraphLink {
            source: "a".into(),
            target: "b".into(),
            link_type: "parent".into(),
        };
        let json = serde_json::to_string(&link).unwrap();
        // "type" not "link_type" due to #[serde(rename)]
        assert!(json.contains("\"type\":\"parent\""), "got: {json}");
    }
}

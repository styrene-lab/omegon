//! JSON API endpoints for the web dashboard.
//!
//! GET /api/state — full agent state snapshot.
//! Designed to be the canonical state shape that any web UI consumes.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use super::{ControlPlaneState, WebState};
use crate::lifecycle::types::*;

/// Full agent state snapshot — the canonical shape for web consumers.
#[derive(Serialize)]
pub struct StateSnapshot {
    pub design: DesignSnapshot,
    pub openspec: OpenSpecSnapshot,
    pub cleave: CleaveSnapshot,
    pub session: SessionSnapshot,
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
pub struct ChildSnapshot {
    pub label: String,
    pub status: String,
    pub duration_secs: Option<f64>,
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

/// GET /api/startup — machine-readable dashboard startup/discovery metadata.
pub async fn get_startup(
    State(state): State<WebState>,
) -> Result<Json<super::WebStartupInfo>, StatusCode> {
    match state.startup_info.lock() {
        Ok(guard) => guard.clone().map(Json).ok_or(StatusCode::SERVICE_UNAVAILABLE),
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

/// GET /api/graph — graph data for force-directed layout.
pub async fn get_graph(State(state): State<WebState>) -> Json<GraphData> {
    let mut nodes = Vec::new();
    let mut links = Vec::new();

    if let Some(ref lp_lock) = state.handles.lifecycle
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

            // Parent → child edges
            if let Some(ref parent) = node.parent {
                links.push(GraphLink {
                    source: parent.clone(),
                    target: node.id.clone(),
                    link_type: "parent".into(),
                });
            }
            // Dependencies
            for dep in &node.dependencies {
                links.push(GraphLink {
                    source: dep.clone(),
                    target: node.id.clone(),
                    link_type: "dependency".into(),
                });
            }
        }
    }

    Json(GraphData { nodes, links })
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

    StateSnapshot {
        design,
        openspec,
        cleave,
        session,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::dashboard::DashboardHandles;
    use crate::web::{ControlPlaneState, WebAuthState, WebStartupInfo};

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
            }))),
            control_plane_state: std::sync::Arc::new(std::sync::Mutex::new(
                ControlPlaneState::Ready,
            )),
        }
    }

    #[test]
    fn empty_snapshot() {
        let snap = build_snapshot(&test_state());
        assert_eq!(snap.design.counts.total, 0);
        assert!(snap.design.focused.is_none());
        assert!(snap.openspec.changes.is_empty());
        assert!(!snap.cleave.active);
    }

    #[tokio::test]
    async fn startup_payload_is_available() {
        let payload = get_startup(axum::extract::State(test_state())).await.unwrap().0;

        assert_eq!(payload.schema_version, 2);
        assert_eq!(payload.state_url, "http://127.0.0.1:7842/api/state");
        assert_eq!(payload.health_url, "http://127.0.0.1:7842/api/healthz");
        assert_eq!(payload.ready_url, "http://127.0.0.1:7842/api/readyz");
        assert_eq!(payload.auth_mode, "ephemeral-bearer");
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

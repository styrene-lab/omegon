//! Browser-facing semantic surface snapshot DTOs.
//!
//! These DTOs are intentionally web-owned rather than a direct serialization of
//! TUI structs. They give the browser and Auspex a stable native contract while
//! preserving the renderer-neutral surface vocabulary as the source of truth.

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

use super::WebState;

pub const WEB_SURFACES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct WebSurfacesSnapshot {
    pub schema_version: u32,
    pub session_id: String,
    pub revision: u64,
    pub generated_at: String,
    pub surfaces: WebSurfaceBundle,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSurfaceBundle {
    pub conversation: WebConversationSurface,
    pub editor: WebEditorSurface,
    pub command: WebCommandSurface,
    pub command_menu: WebCommandMenuSurface,
    pub dashboard: WebDashboardSurface,
    pub footer: WebFooterSurface,
    pub instruments: WebInstrumentsSurface,
    pub memory_status: WebMemoryStatusSurface,
    pub operations: WebOperationsSurface,
    pub plan: WebPlanSurface,
    pub runtime: WebRuntimeSurface,
    pub settings: WebSettingsSurface,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebConversationSurface {
    pub segments: Vec<WebConversationSegment>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebConversationSegment {
    pub index: usize,
    pub role: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub body: Option<String>,
    pub complete: bool,
    pub copyable: bool,
    pub selectable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebEditorSurface {
    pub accepts_prompt: bool,
    pub placeholder: String,
    pub queue_mode: String,
    pub supports_attachments: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebCommandSurface {
    pub pending_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebCommandMenuSurface {
    pub available: bool,
    pub open: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebDashboardSurface {
    pub session: WebDashboardSessionSurface,
    pub lifecycle_available: bool,
    pub cleave_available: bool,
    pub delegate_available: bool,
    pub harness_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebDashboardSessionSurface {
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebFooterSurface {
    pub busy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebInstrumentsSurface {
    pub active_tool: Option<String>,
    pub tools: Vec<WebToolRunSurface>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebToolRunSurface {
    pub id: String,
    pub name: String,
    pub status: String,
    pub args: Value,
    pub output_tail: Option<String>,
    pub result_summary: Option<String>,
    pub is_error: bool,
    pub elapsed_ms: Option<u64>,
    pub phase: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebMemoryStatusSurface {
    pub active_facts: usize,
    pub total_facts: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct WebPlanSurface {
    pub active: Option<WebPlanLane>,
    pub workstreams: Vec<WebPlanWorkstream>,
    pub reconciliation_issues: usize,
    pub promotion_nudges: Vec<String>,
    pub resume_candidates: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebPlanLane {
    pub plan_id: String,
    pub mode: String,
    pub guidance: String,
    pub status: String,
    pub scope: String,
    pub source: String,
    pub completed: usize,
    pub total: usize,
    pub items: Vec<WebPlanItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebPlanItem {
    pub id: Option<String>,
    pub label: String,
    pub status: String,
    pub intent: Option<String>,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebPlanWorkstream {
    pub id: String,
    pub title: String,
    pub status: String,
    pub completed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebOperationsSurface {
    pub active_child_runtimes: usize,
    /// Aggregated delegate + cleave child runtimes, projected for the
    /// browser's Operations instrument. Empty when nothing is running.
    pub kind: Option<String>,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub children: Vec<WebOperationChild>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebOperationChild {
    pub label: String,
    pub status: String,
    pub activity: Option<String>,
    pub tasks_done: usize,
    pub tasks_total: usize,
    pub result_summary: Option<String>,
}

/// Runtime telemetry for the top HUD strip: context routing, autonomy posture,
/// capability grade, and repo state. Projected from `HarnessStatus`.
#[derive(Debug, Clone, Serialize)]
pub struct WebRuntimeSurface {
    pub context_class: Option<String>,
    pub thinking_level: Option<String>,
    pub capability_grade: Option<String>,
    pub posture: Option<String>,
    pub operating_profile: Option<String>,
    pub autonomy_mode: Option<String>,
    pub session_kind: Option<String>,
    pub git_branch: Option<String>,
    pub active_persona: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSettingsSurface {
    pub auth_mode: Option<String>,
    pub auth_source: Option<String>,
}

pub fn project_web_surfaces(state: &WebState) -> WebSurfacesSnapshot {
    let session = state.handles.session.lock().ok();
    let harness = state.handles.harness.as_ref().and_then(|h| h.lock().ok());
    let startup = state
        .startup_info
        .lock()
        .ok()
        .and_then(|guard| guard.clone());

    WebSurfacesSnapshot {
        schema_version: WEB_SURFACES_SCHEMA_VERSION,
        session_id: "default".to_string(),
        revision: 0,
        generated_at: Utc::now().to_rfc3339(),
        surfaces: WebSurfaceBundle {
            conversation: WebConversationSurface {
                segments: state.conversation_segments(),
            },
            editor: WebEditorSurface {
                accepts_prompt: true,
                placeholder: "Ask anything, or type / for commands".to_string(),
                queue_mode: "until_ready".to_string(),
                supports_attachments: true,
            },
            command: WebCommandSurface {
                pending_prompt: None,
            },
            command_menu: WebCommandMenuSurface {
                available: true,
                open: false,
            },
            dashboard: WebDashboardSurface {
                session: WebDashboardSessionSurface {
                    turns: session.as_ref().map(|s| s.turns).unwrap_or(0),
                    tool_calls: session.as_ref().map(|s| s.tool_calls).unwrap_or(0),
                    compactions: session.as_ref().map(|s| s.compactions).unwrap_or(0),
                },
                lifecycle_available: state.handles.lifecycle.is_some(),
                cleave_available: state.handles.cleave.is_some(),
                delegate_available: state.handles.delegate.is_some(),
                harness_available: state.handles.harness.is_some(),
            },
            footer: WebFooterSurface {
                busy: session.as_ref().is_some_and(|s| s.busy),
            },
            instruments: project_instruments(state),
            memory_status: WebMemoryStatusSurface {
                active_facts: harness.as_ref().map(|h| h.memory.active_facts).unwrap_or(0),
                total_facts: harness.as_ref().map(|h| h.memory.total_facts).unwrap_or(0),
            },
            operations: project_operations(state),
            plan: project_plan(state),
            runtime: project_runtime(harness.as_deref()),
            settings: WebSettingsSurface {
                auth_mode: startup.as_ref().map(|s| s.auth_mode.clone()),
                auth_source: startup.as_ref().map(|s| s.auth_source.clone()),
            },
        },
    }
}

fn project_instruments(state: &WebState) -> WebInstrumentsSurface {
    let tools = state
        .tool_runs
        .lock()
        .map(|tools| tools.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let active_tool = tools
        .iter()
        .rev()
        .find(|tool| tool.status == "running")
        .map(|tool| tool.name.clone());
    WebInstrumentsSurface { active_tool, tools }
}

/// Project the latest typed plan surface received from `AgentEvent::PlanUpdated`
/// into the browser's right-rail Plan instrument. The web DTO keeps the same
/// semantic field names as `omegon_traits::PlanSurfaceProjection` while flattening
/// progress into `completed` / `total` for simple renderer binding.
fn project_plan(state: &WebState) -> WebPlanSurface {
    let projection = state
        .plan_surface
        .lock()
        .ok()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    WebPlanSurface {
        active: projection.active.map(|lane| WebPlanLane {
            plan_id: lane.plan_id,
            mode: lane.mode,
            guidance: lane.guidance,
            status: lane.status,
            scope: lane.scope,
            source: lane.source,
            completed: lane.progress.completed,
            total: lane.progress.total,
            items: lane
                .items
                .into_iter()
                .map(|item| WebPlanItem {
                    id: item.id,
                    label: item.label,
                    status: item.status,
                    intent: item.intent,
                    writable: item.writable,
                })
                .collect(),
        }),
        workstreams: projection
            .workstreams
            .into_iter()
            .map(|workstream| WebPlanWorkstream {
                id: workstream.id,
                title: workstream.title,
                status: workstream.status,
                completed: workstream.progress.completed,
                total: workstream.progress.total,
            })
            .collect(),
        reconciliation_issues: projection.reconciliation_issues.len(),
        promotion_nudges: projection.promotion_nudges,
        resume_candidates: projection.resume_candidates.len(),
    }
}

/// Project delegate + cleave child runtimes into the browser Operations
/// surface. Delegate takes precedence as the `kind` when both are active;
/// counts and children are merged so the instrument shows all live work.
fn project_operations(state: &WebState) -> WebOperationsSurface {
    let mut kind: Option<String> = None;
    let (mut running, mut completed, mut failed) = (0usize, 0usize, 0usize);
    let mut children: Vec<WebOperationChild> = Vec::new();

    if let Some(delegate) = state.handles.delegate.as_ref().and_then(|d| d.lock().ok())
        && (delegate.active || !delegate.children.is_empty())
    {
        kind = Some("delegate".to_string());
        running += delegate.running;
        completed += delegate.completed;
        failed += delegate.failed;
        for child in &delegate.children {
            children.push(WebOperationChild {
                label: child.label.clone(),
                status: child.status.clone(),
                activity: child.last_tool.clone(),
                tasks_done: child.tasks_done,
                tasks_total: child.tasks.len(),
                result_summary: child.result_summary.clone(),
            });
        }
    }

    if let Some(cleave) = state.handles.cleave.as_ref().and_then(|c| c.lock().ok())
        && (cleave.active || !cleave.children.is_empty())
    {
        if kind.is_none() {
            kind = Some("cleave".to_string());
        }
        running += cleave
            .children
            .iter()
            .filter(|c| c.status == "running")
            .count();
        completed += cleave.completed;
        failed += cleave.failed;
        for child in &cleave.children {
            children.push(WebOperationChild {
                label: child.label.clone(),
                status: child.status.clone(),
                activity: child.last_tool.clone(),
                tasks_done: child.tasks_done,
                tasks_total: child.tasks.len(),
                result_summary: None,
            });
        }
    }

    let active_child_runtimes = running;
    WebOperationsSurface {
        active_child_runtimes,
        kind,
        running,
        completed,
        failed,
        children,
    }
}

/// Project runtime telemetry for the top HUD strip from `HarnessStatus`.
fn project_runtime(harness: Option<&crate::status::HarnessStatus>) -> WebRuntimeSurface {
    let Some(h) = harness else {
        return WebRuntimeSurface {
            context_class: None,
            thinking_level: None,
            capability_grade: None,
            posture: None,
            operating_profile: None,
            autonomy_mode: None,
            session_kind: None,
            git_branch: None,
            active_persona: None,
        };
    };
    WebRuntimeSurface {
        context_class: Some(h.context_class.clone()),
        thinking_level: Some(h.thinking_level.clone()),
        capability_grade: Some(h.capability_grade.clone()),
        posture: Some(h.posture.clone()),
        operating_profile: Some(h.operating_profile.clone()),
        autonomy_mode: Some(format!("{:?}", h.autonomy_mode)),
        session_kind: Some(h.session_kind.clone()),
        git_branch: h.git_branch.clone(),
        active_persona: h.active_persona.as_ref().map(|p| p.name.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::cleave::{ChildProgress, CleaveProgress};
    use crate::features::delegate::{DelegateProgress, DelegateProgressChild};
    use crate::status::HarnessStatus;
    use std::sync::{Arc, Mutex};

    fn test_state() -> WebState {
        WebState::new(
            super::super::DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
        )
    }

    fn cleave_child(label: &str, status: &str) -> ChildProgress {
        ChildProgress {
            label: label.into(),
            status: status.into(),
            failure_kind: None,
            duration_secs: None,
            supervision_mode: None,
            pid: None,
            last_tool: Some("bash".into()),
            last_tool_activity: None,
            last_turn: Some(3),
            tasks: Vec::new(),
            tasks_done: 1,
            started_at: None,
            last_activity_at: None,
            tokens_in: 0,
            tokens_out: 0,
            runtime: None,
        }
    }

    fn delegate_child(label: &str, status: &str) -> DelegateProgressChild {
        DelegateProgressChild {
            task_id: "task-1".into(),
            label: label.into(),
            status: status.into(),
            last_tool: Some("write".into()),
            last_tool_activity: None,
            last_turn: Some(2),
            started_at: None,
            completed_at: None,
            result_summary: Some("scouted module".into()),
            failure_kind: None,
            tasks: Vec::new(),
            tasks_done: 2,
            route_decision: None,
            result_viewed: false,
        }
    }

    #[test]
    fn runtime_surface_is_empty_without_harness() {
        let rt = project_runtime(None);
        assert!(rt.autonomy_mode.is_none());
        assert!(rt.context_class.is_none());
        assert!(rt.active_persona.is_none());
    }

    #[test]
    fn runtime_surface_projects_harness_fields() {
        let h = HarnessStatus {
            context_class: "Standard".into(),
            thinking_level: "high".into(),
            capability_grade: "B".into(),
            posture: "Architect".into(),
            session_kind: "interactive".into(),
            ..Default::default()
        };
        let rt = project_runtime(Some(&h));
        assert_eq!(rt.context_class.as_deref(), Some("Standard"));
        assert_eq!(rt.capability_grade.as_deref(), Some("B"));
        assert_eq!(rt.posture.as_deref(), Some("Architect"));
        // autonomy_mode is always populated (Debug-formatted) when harness present
        assert!(rt.autonomy_mode.is_some());
    }

    #[test]
    fn operations_surface_is_empty_when_idle() {
        let ops = project_operations(&test_state());
        assert!(ops.kind.is_none());
        assert_eq!(ops.running, 0);
        assert_eq!(ops.completed, 0);
        assert_eq!(ops.failed, 0);
        assert!(ops.children.is_empty());
    }

    #[test]
    fn instruments_surface_is_empty_without_tool_events() {
        let instruments = project_instruments(&test_state());
        assert!(instruments.active_tool.is_none());
        assert!(instruments.tools.is_empty());
    }

    #[test]
    fn instruments_surface_reports_latest_running_tool() {
        let state = test_state();
        {
            let mut tools = state.tool_runs.lock().expect("tool runs lock");
            tools.push_back(WebToolRunSurface {
                id: "tool-1".into(),
                name: "read".into(),
                status: "completed".into(),
                args: serde_json::json!({"path": "src/lib.rs"}),
                output_tail: None,
                result_summary: Some("read source".into()),
                is_error: false,
                elapsed_ms: Some(12),
                phase: None,
            });
            tools.push_back(WebToolRunSurface {
                id: "tool-2".into(),
                name: "bash".into(),
                status: "running".into(),
                args: serde_json::json!({"command": "cargo test"}),
                output_tail: Some("running 8 tests".into()),
                result_summary: None,
                is_error: false,
                elapsed_ms: Some(1840),
                phase: Some("executing".into()),
            });
            tools.push_back(WebToolRunSurface {
                id: "tool-3".into(),
                name: "write".into(),
                status: "running".into(),
                args: serde_json::json!({"path": "src/web.rs"}),
                output_tail: None,
                result_summary: None,
                is_error: false,
                elapsed_ms: Some(400),
                phase: Some("applying".into()),
            });
        }

        let instruments = project_instruments(&state);
        assert_eq!(instruments.active_tool.as_deref(), Some("write"));
        assert_eq!(instruments.tools.len(), 3);
        assert_eq!(instruments.tools[0].name, "read");
        assert_eq!(
            instruments.tools[1].output_tail.as_deref(),
            Some("running 8 tests")
        );
        assert_eq!(instruments.tools[2].phase.as_deref(), Some("applying"));
    }

    #[test]
    fn plan_surface_is_empty_without_plan_update() {
        let plan = project_plan(&test_state());
        assert!(plan.active.is_none());
        assert!(plan.workstreams.is_empty());
        assert_eq!(plan.reconciliation_issues, 0);
        assert!(plan.promotion_nudges.is_empty());
        assert_eq!(plan.resume_candidates, 0);
    }

    #[test]
    fn plan_surface_projects_active_lane_and_workstreams() {
        let state = test_state();
        {
            let mut plan = state.plan_surface.lock().expect("plan surface lock");
            *plan = omegon_traits::PlanSurfaceProjection {
                active: Some(omegon_traits::PlanLaneProjection {
                    plan_id: "session:current".into(),
                    mode: "executing".into(),
                    guidance: "advance the current implementation slice".into(),
                    status: "active".into(),
                    scope: "session".into(),
                    source: "operator".into(),
                    progress: omegon_traits::PlanProgressProjection {
                        completed: 2,
                        total: 4,
                    },
                    items: vec![omegon_traits::PlanItemProjection {
                        id: Some("task-1".into()),
                        label: "Wire plan surface into the web rail".into(),
                        status: "active".into(),
                        intent: Some("implementation".into()),
                        writable: true,
                    }],
                }),
                workstreams: vec![omegon_traits::PlanWorkstreamProjection {
                    id: "ws-ui".into(),
                    title: "Web UI".into(),
                    status: "active".into(),
                    progress: omegon_traits::PlanProgressProjection {
                        completed: 3,
                        total: 5,
                    },
                }],
                promotion_nudges: vec!["record the web surface contract".into()],
                ..Default::default()
            };
        }

        let projected = project_plan(&state);
        let active = projected.active.expect("active plan lane");
        assert_eq!(active.plan_id, "session:current");
        assert_eq!(active.mode, "executing");
        assert_eq!(active.completed, 2);
        assert_eq!(active.total, 4);
        assert_eq!(active.items.len(), 1);
        assert_eq!(active.items[0].label, "Wire plan surface into the web rail");
        assert_eq!(active.items[0].intent.as_deref(), Some("implementation"));
        assert!(active.items[0].writable);
        assert_eq!(projected.workstreams.len(), 1);
        assert_eq!(projected.workstreams[0].completed, 3);
        assert_eq!(
            projected.promotion_nudges,
            vec!["record the web surface contract"]
        );
    }

    #[test]
    fn operations_surface_projects_delegate_children() {
        let mut state = test_state();
        state.handles.delegate = Some(Arc::new(Mutex::new(DelegateProgress {
            active: true,
            running: 1,
            completed: 1,
            failed: 0,
            pending_results: 1,
            children: vec![
                delegate_child("scout-mod", "running"),
                delegate_child("scout-tests", "completed"),
            ],
        })));

        let ops = project_operations(&state);
        assert_eq!(ops.kind.as_deref(), Some("delegate"));
        assert_eq!(ops.running, 1);
        assert_eq!(ops.completed, 1);
        assert_eq!(ops.children.len(), 2);
        let first = &ops.children[0];
        assert_eq!(first.label, "scout-mod");
        assert_eq!(first.activity.as_deref(), Some("write"));
        assert_eq!(first.tasks_done, 2);
        assert_eq!(first.result_summary.as_deref(), Some("scouted module"));
    }

    #[test]
    fn operations_surface_merges_delegate_and_cleave() {
        let mut state = test_state();
        state.handles.delegate = Some(Arc::new(Mutex::new(DelegateProgress {
            active: true,
            running: 1,
            completed: 0,
            failed: 0,
            pending_results: 0,
            children: vec![delegate_child("deleg-a", "running")],
        })));
        state.handles.cleave = Some(Arc::new(Mutex::new(CleaveProgress {
            active: true,
            run_id: "run-9".into(),
            total_children: 2,
            completed: 1,
            failed: 0,
            children: vec![
                cleave_child("cleave-a", "running"),
                cleave_child("cleave-b", "completed"),
            ],
            total_tokens_in: 0,
            total_tokens_out: 0,
        })));

        let ops = project_operations(&state);
        // delegate wins the kind label when both are active
        assert_eq!(ops.kind.as_deref(), Some("delegate"));
        // running = delegate.running (1) + cleave children with status running (1)
        assert_eq!(ops.running, 2);
        // completed = delegate.completed (0) + cleave.completed (1)
        assert_eq!(ops.completed, 1);
        // all children merged: 1 delegate + 2 cleave
        assert_eq!(ops.children.len(), 3);
    }

    #[test]
    fn snapshot_bundle_reports_capability_flags() {
        let mut state = test_state();
        state.handles.delegate = Some(Arc::new(Mutex::new(DelegateProgress::default())));
        let snap = project_web_surfaces(&state);
        assert_eq!(snap.schema_version, WEB_SURFACES_SCHEMA_VERSION);
        assert!(snap.surfaces.dashboard.delegate_available);
        assert!(!snap.surfaces.dashboard.cleave_available);
    }
}

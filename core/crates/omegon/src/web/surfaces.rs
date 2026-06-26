//! Browser-facing semantic surface snapshot DTOs.
//!
//! These DTOs are intentionally web-owned rather than a direct serialization of
//! TUI structs. They give the browser and Auspex a stable native contract while
//! preserving the renderer-neutral surface vocabulary as the source of truth.

use chrono::Utc;
use serde::Serialize;

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
}

#[derive(Debug, Clone, Serialize)]
pub struct WebMemoryStatusSurface {
    pub active_facts: usize,
    pub total_facts: usize,
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
            instruments: WebInstrumentsSurface { active_tool: None },
            memory_status: WebMemoryStatusSurface {
                active_facts: harness.as_ref().map(|h| h.memory.active_facts).unwrap_or(0),
                total_facts: harness.as_ref().map(|h| h.memory.total_facts).unwrap_or(0),
            },
            operations: project_operations(state),
            runtime: project_runtime(harness.as_deref()),
            settings: WebSettingsSurface {
                auth_mode: startup.as_ref().map(|s| s.auth_mode.clone()),
                auth_source: startup.as_ref().map(|s| s.auth_source.clone()),
            },
        },
    }
}

/// Project delegate + cleave child runtimes into the browser Operations
/// surface. Delegate takes precedence as the `kind` when both are active;
/// counts and children are merged so the instrument shows all live work.
fn project_operations(state: &WebState) -> WebOperationsSurface {
    let mut kind: Option<String> = None;
    let (mut running, mut completed, mut failed) = (0usize, 0usize, 0usize);
    let mut children: Vec<WebOperationChild> = Vec::new();

    if let Some(delegate) = state.handles.delegate.as_ref().and_then(|d| d.lock().ok()) {
        if delegate.active || !delegate.children.is_empty() {
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
    }

    if let Some(cleave) = state.handles.cleave.as_ref().and_then(|c| c.lock().ok()) {
        if cleave.active || !cleave.children.is_empty() {
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

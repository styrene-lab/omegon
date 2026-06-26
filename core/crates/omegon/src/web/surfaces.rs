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
            operations: WebOperationsSurface {
                active_child_runtimes: harness
                    .as_ref()
                    .map(|h| h.active_delegates.len())
                    .unwrap_or(0),
            },
            settings: WebSettingsSurface {
                auth_mode: startup.as_ref().map(|s| s.auth_mode.clone()),
                auth_source: startup.as_ref().map(|s| s.auth_source.clone()),
            },
        },
    }
}

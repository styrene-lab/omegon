//! JSON API endpoints for the web dashboard.
//!
//! GET /api/state — full agent state snapshot.
//! Designed to be the canonical state shape that any web UI consumes.

use crate::status::HarnessStatus;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use omegon_traits::{DaemonEventEnvelope, OmegonInstanceDescriptor};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize)]
pub struct WebSessionListResponse {
    pub sessions: Vec<WebSessionSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSessionShowResponse {
    pub session: WebSessionSummary,
    pub snapshot: super::surfaces::WebSurfacesSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSessionSummary {
    pub session_id: String,
    pub cwd: String,
    pub created_at: String,
    pub turns: u32,
    pub tool_calls: u32,
    pub last_prompt_snippet: String,
    pub current: bool,
}

fn web_session_summary(entry: crate::session::SessionEntry) -> WebSessionSummary {
    WebSessionSummary {
        session_id: entry.meta.session_id,
        cwd: entry.meta.cwd,
        created_at: entry.meta.created_at,
        turns: entry.meta.turns,
        tool_calls: entry.meta.tool_calls,
        last_prompt_snippet: entry.meta.last_prompt_snippet,
        current: false,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebActionRequest {
    pub schema_version: u32,
    pub action_id: String,
    pub client_id: String,
    #[serde(default = "default_web_session_id")]
    pub session_id: String,
    pub action: WebActionPayload,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebActionPayload {
    SubmitPrompt {
        text: String,
        #[serde(default)]
        attachments: Vec<String>,
    },
    CancelActiveTurn,
    RunSlashCommand {
        raw: String,
    },
    RespondPermission {
        request_id: String,
        allow: bool,
    },
    RespondOperatorWait {
        request_id: String,
        completed: bool,
    },
    CopyLatestResponse,
    SelectSegment {
        index: usize,
    },
    CopySegment {
        index: usize,
    },
}

fn default_web_session_id() -> String {
    "default".to_string()
}

fn web_outcome_accepted(
    session_id: String,
    action_id: String,
    message: Option<String>,
) -> crate::ui_runtime::envelope::UiActionOutcomeEnvelope {
    crate::ui_runtime::envelope::UiActionOutcomeEnvelope::accepted(
        session_id,
        action_id,
        Some(0),
        message,
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct WebCapabilityDescriptor {
    pub interactive: bool,
    pub chat: bool,
    pub hosted_web_ui: bool,
    pub surface_api: bool,
    pub supports_tool_approval: bool,
    pub supports_operator_wait: bool,
    pub supports_session_resume: bool,
    pub supports_attachments: bool,
    pub supports_auspex_proxy: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebAttachmentCreateRequest {
    pub filename: String,
    pub content_type: Option<String>,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAttachmentResponse {
    pub id: String,
    pub filename: String,
    pub content_type: Option<String>,
    pub size_bytes: usize,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebAttachmentGetResponse {
    pub attachment: WebAttachmentResponse,
    pub data_base64: String,
}

fn sanitize_web_attachment_filename(filename: &str) -> Option<String> {
    if filename.contains(['/', '\\', '\0']) || filename.contains("..") {
        return None;
    }
    let name = std::path::Path::new(filename).file_name()?.to_str()?.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(name.to_string())
}

fn web_attachment_root() -> std::path::PathBuf {
    std::env::temp_dir().join("omegon-web-attachments")
}

#[derive(Debug, Clone, Serialize)]
pub struct WebLaunchContextResponse {
    pub mode: String,
    pub proxied_by: Option<String>,
    pub back_url: Option<String>,
    pub policy_owner: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantRunsListResponse {
    pub runs: Vec<crate::capabilities::runs::AssistantRunSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantRunShowResponse {
    pub run: crate::capabilities::runs::AssistantRunSummary,
}

pub async fn get_assistant_runs(
    State(state): State<WebState>,
) -> Result<Json<AssistantRunsListResponse>, StatusCode> {
    let store =
        crate::capabilities::runs::SqliteAssistantRunStore::open(&state.assistant_runs_db_path)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(AssistantRunsListResponse {
        runs: store
            .list()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    }))
}

pub async fn get_assistant_run(
    State(state): State<WebState>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<Json<AssistantRunShowResponse>, StatusCode> {
    let store =
        crate::capabilities::runs::SqliteAssistantRunStore::open(&state.assistant_runs_db_path)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let run = store
        .get(&run_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(AssistantRunShowResponse { run }))
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityAssistantsResponse {
    pub assistants: Vec<crate::capabilities::profiles::AssistantListItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantReadinessResponse {
    pub assistant: crate::capabilities::profiles::AssistantListItem,
}

pub async fn get_capability_assistant_readiness(
    State(state): State<WebState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<AssistantReadinessResponse>, StatusCode> {
    let snapshot = capability_inventory_snapshot(state)?;
    let assistant = snapshot
        .assistant_list
        .into_iter()
        .find(|assistant| assistant.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(AssistantReadinessResponse { assistant }))
}

pub async fn get_capability_assistants(
    State(state): State<WebState>,
) -> Result<Json<CapabilityAssistantsResponse>, StatusCode> {
    let snapshot = capability_inventory_snapshot(state)?;
    Ok(Json(CapabilityAssistantsResponse {
        assistants: snapshot.assistant_list,
    }))
}

fn capability_inventory_snapshot(
    state: WebState,
) -> Result<crate::capabilities::inventory::CapabilityInventorySnapshot, StatusCode> {
    let home = crate::paths::omegon_home().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let armory_home = home.join("armory");
    let project_armory = cwd.join("../omegon-armory");
    let armory_root =
        if !armory_home.join("profiles").exists() && project_armory.join("profiles").exists() {
            project_armory.as_path()
        } else {
            armory_home.as_path()
        };
    let roots = crate::capabilities::inventory::CapabilityInventoryRoots {
        extensions_dir: &home.join("extensions"),
        armory_root,
        catalog_dir: &home.join("catalog"),
    };
    let secret_inputs = state
        .secrets
        .as_ref()
        .map(
            |secrets| crate::capabilities::secrets::SecretReadinessInputs {
                session_diagnostics: secrets
                    .session_diagnostics()
                    .into_iter()
                    .map(
                        |diag| crate::capabilities::secrets::SecretSessionDiagnostic {
                            name: diag.name,
                            warmed: diag.warmed,
                        },
                    )
                    .collect(),
                recipe_descriptors: secrets
                    .list_recipe_descriptors()
                    .into_iter()
                    .map(
                        |descriptor| crate::capabilities::secrets::SecretRecipeDescriptorSummary {
                            name: descriptor.name,
                            kind: descriptor.kind,
                        },
                    )
                    .collect(),
            },
        )
        .unwrap_or_default();
    crate::capabilities::inventory::build_capability_inventory_snapshot_with_secrets(
        roots,
        secret_inputs,
    )
    .map_err(|error| {
        tracing::error!(?error, "capability inventory snapshot failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

/// GET /api/capabilities — assistant capability inventory snapshot.
pub async fn get_capabilities(
    State(state): State<WebState>,
) -> Result<Json<crate::capabilities::inventory::CapabilityInventorySnapshot>, StatusCode> {
    Ok(Json(capability_inventory_snapshot(state)?))
}

/// GET /api/web/sessions — browser-native saved session list.
pub async fn get_web_sessions() -> Result<Json<WebSessionListResponse>, StatusCode> {
    let cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut sessions: Vec<WebSessionSummary> = crate::session::list_sessions(&cwd)
        .into_iter()
        .map(web_session_summary)
        .collect();
    sessions.insert(
        0,
        WebSessionSummary {
            session_id: "default".to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            turns: 0,
            tool_calls: 0,
            last_prompt_snippet: "Current live session".to_string(),
            current: true,
        },
    );
    Ok(Json(WebSessionListResponse { sessions }))
}

/// GET /api/web/sessions/{session_id} — session metadata plus current web surface snapshot.
pub async fn get_web_session(
    State(state): State<WebState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<WebSessionShowResponse>, StatusCode> {
    let cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let session = if session_id == "default" {
        WebSessionSummary {
            session_id: "default".to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            turns: state
                .handles
                .session
                .lock()
                .ok()
                .map(|s| s.turns)
                .unwrap_or(0),
            tool_calls: state
                .handles
                .session
                .lock()
                .ok()
                .map(|s| s.tool_calls)
                .unwrap_or(0),
            last_prompt_snippet: "Current live session".to_string(),
            current: true,
        }
    } else {
        crate::session::list_sessions(&cwd)
            .into_iter()
            .find(|entry| entry.meta.session_id == session_id)
            .map(web_session_summary)
            .ok_or(StatusCode::NOT_FOUND)?
    };

    Ok(Json(WebSessionShowResponse {
        session,
        snapshot: super::surfaces::project_web_surfaces(&state),
    }))
}

/// GET /api/web/attachments/{id} — retrieve a staged browser attachment.
pub async fn get_web_attachment(
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<WebAttachmentGetResponse>, StatusCode> {
    if id.contains(['/', '\\', '\0']) || id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let dir = web_attachment_root().join(&id);
    let meta_path = dir.join("meta.json");
    let data_path = dir.join("data.bin");
    if !meta_path.exists() || !data_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }
    let meta = std::fs::read_to_string(meta_path).map_err(|_| StatusCode::NOT_FOUND)?;
    let attachment: WebAttachmentResponse =
        serde_json::from_str(&meta).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let bytes = std::fs::read(data_path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(WebAttachmentGetResponse {
        attachment,
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }))
}

/// POST /api/web/attachments — stage a browser-provided attachment by id.
pub async fn post_web_attachment(
    Json(request): Json<WebAttachmentCreateRequest>,
) -> Result<(StatusCode, Json<WebAttachmentResponse>), StatusCode> {
    const MAX_ATTACHMENT_BYTES: usize = 16 * 1024 * 1024;
    let filename =
        sanitize_web_attachment_filename(&request.filename).ok_or(StatusCode::BAD_REQUEST)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(request.data_base64.as_bytes())
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let id = format!(
        "att-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let root = web_attachment_root();
    let dir = root.join(&id);
    std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let data_path = dir.join("data.bin");
    std::fs::write(&data_path, &bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let expires_at = (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
    let response = WebAttachmentResponse {
        id,
        filename,
        content_type: request.content_type,
        size_bytes: bytes.len(),
        expires_at,
    };
    let meta = serde_json::to_string(&response).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(dir.join("meta.json"), meta).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// POST /api/web/actions — browser-native semantic action ingress.
pub async fn post_web_action(
    State(state): State<WebState>,
    Json(request): Json<WebActionRequest>,
) -> (
    StatusCode,
    Json<crate::ui_runtime::envelope::UiActionOutcomeEnvelope>,
) {
    if request.schema_version != 1 {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                    request.session_id,
                    request.action_id,
                    "unsupported schema_version",
                ),
            ),
        );
    }
    if request.session_id != "default" {
        return (
            StatusCode::NOT_FOUND,
            Json(
                crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                    request.session_id,
                    request.action_id,
                    "unknown session_id",
                ),
            ),
        );
    }

    let send_result = match request.action {
        WebActionPayload::SubmitPrompt { text, attachments } => {
            if !attachments.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                            request.session_id,
                            request.action_id,
                            "attachment prompt conversion is not implemented yet",
                        ),
                    ),
                );
            }
            if text.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                            request.session_id,
                            request.action_id,
                            "prompt text cannot be empty",
                        ),
                    ),
                );
            }
            state
                .command_tx
                .try_send(super::WebCommand::UserPrompt(text))
        }
        WebActionPayload::CancelActiveTurn => state.command_tx.try_send(super::WebCommand::Cancel),
        WebActionPayload::RunSlashCommand { raw } => {
            let trimmed = raw.trim();
            if !trimmed.starts_with('/') {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                            request.session_id,
                            request.action_id,
                            "not a slash command",
                        ),
                    ),
                );
            }
            let command = trimmed.trim_start_matches('/');
            let (name, args) = command.split_once(' ').unwrap_or((command, ""));
            state.command_tx.try_send(super::WebCommand::SlashCommand {
                name: name.to_string(),
                args: args.to_string(),
                respond_to: None,
            })
        }
        WebActionPayload::RespondPermission { request_id, allow } => {
            let decision = if allow {
                omegon_traits::PermissionResponse::Allow
            } else {
                omegon_traits::PermissionResponse::Deny
            };
            return match state.answer_permission(&request_id, decision) {
                Ok(()) => (
                    StatusCode::ACCEPTED,
                    Json(web_outcome_accepted(
                        request.session_id,
                        request.action_id,
                        Some(
                            if allow {
                                "permission allowed"
                            } else {
                                "permission denied"
                            }
                            .to_string(),
                        ),
                    )),
                ),
                Err(reason) => (
                    StatusCode::NOT_FOUND,
                    Json(
                        crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                            request.session_id,
                            request.action_id,
                            reason,
                        ),
                    ),
                ),
            };
        }
        WebActionPayload::RespondOperatorWait {
            request_id,
            completed,
        } => {
            return match state.answer_operator_wait(&request_id, completed) {
                Ok(()) => (
                    StatusCode::ACCEPTED,
                    Json(web_outcome_accepted(
                        request.session_id,
                        request.action_id,
                        Some(
                            if completed {
                                "operator action completed"
                            } else {
                                "operator action cancelled"
                            }
                            .to_string(),
                        ),
                    )),
                ),
                Err(reason) => (
                    StatusCode::NOT_FOUND,
                    Json(
                        crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                            request.session_id,
                            request.action_id,
                            reason,
                        ),
                    ),
                ),
            };
        }
        WebActionPayload::CopyLatestResponse
        | WebActionPayload::SelectSegment { .. }
        | WebActionPayload::CopySegment { .. } => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(
                    crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                        request.session_id,
                        request.action_id,
                        "action requires renderer-neutral runtime state wiring",
                    ),
                ),
            );
        }
    };

    match send_result {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(web_outcome_accepted(
                request.session_id,
                request.action_id,
                Some("action queued".to_string()),
            )),
        ),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                    request.session_id,
                    request.action_id,
                    "command queue unavailable",
                ),
            ),
        ),
    }
}

/// GET /api/web/surfaces — browser-native semantic surface snapshot.
pub async fn get_web_surfaces(
    State(state): State<WebState>,
) -> Json<super::surfaces::WebSurfacesSnapshot> {
    Json(super::surfaces::project_web_surfaces(&state))
}

/// GET /api/web/capabilities — web/Auspex capability descriptor.
pub async fn get_web_capabilities() -> Json<WebCapabilityDescriptor> {
    Json(WebCapabilityDescriptor {
        interactive: true,
        chat: true,
        hosted_web_ui: true,
        surface_api: true,
        supports_tool_approval: true,
        supports_operator_wait: true,
        supports_session_resume: true,
        supports_attachments: true,
        supports_auspex_proxy: true,
    })
}

/// GET /api/web/launch-context — describes how the web UI was launched.
pub async fn get_web_launch_context(headers: HeaderMap) -> Json<WebLaunchContextResponse> {
    let proxied_by = headers
        .get("x-omegon-proxied-by")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.to_string());
    let back_url = headers
        .get("x-omegon-back-url")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("http://") || v.starts_with("https://"))
        .map(|v| v.to_string());
    let policy_owner = if proxied_by.is_some() {
        "auspex"
    } else {
        "omegon"
    };

    Json(WebLaunchContextResponse {
        mode: if proxied_by.is_some() {
            "proxied"
        } else {
            "direct"
        }
        .to_string(),
        proxied_by,
        back_url,
        policy_owner: policy_owner.to_string(),
    })
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

    let caller_role = match event.caller_role.as_deref().unwrap_or("admin") {
        "read" => crate::control_actions::ControlRole::Read,
        "edit" => crate::control_actions::ControlRole::Edit,
        _ => crate::control_actions::ControlRole::Admin,
    };
    let required = crate::control_actions::classify_daemon_trigger(&event.trigger_kind).role;
    if !crate::control_actions::is_role_sufficient(caller_role, required) {
        return (
            StatusCode::FORBIDDEN,
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

    if let Some(ref lifecycle) = handles.lifecycle
        && let Ok(lp) = lifecycle.provider().lock()
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
    if let Some(ref lifecycle) = state.handles.lifecycle
        && let Ok(lp) = lifecycle.provider().lock()
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
    }

    if let Some(ref lifecycle) = state.handles.lifecycle
        && let Ok(snapshot) = lifecycle.openspec_snapshot(Default::default())
    {
        openspec.total_tasks = snapshot.total_tasks;
        openspec.done_tasks = snapshot.done_tasks;
        openspec.changes = snapshot
            .changes
            .into_iter()
            .map(|change| ChangeSnapshot {
                name: change.name,
                stage: change.lifecycle_state,
                has_specs: change.has_specs,
                has_tasks: change.has_tasks,
                total_tasks: change.total_tasks,
                done_tasks: change.done_tasks,
            })
            .collect();
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
                    .unwrap_or_else(|| "Compact".into()),
                thinking_level: harness
                    .as_ref()
                    .map(|h| h.thinking_level.clone())
                    .unwrap_or_else(|| "Medium".into()),
                capability_tier: harness
                    .as_ref()
                    .map(|h| h.capability_grade.clone())
                    .unwrap_or_else(|| "B".into()),
                runtime_profile: harness
                    .as_ref()
                    .map(|h| h.runtime_profile.as_str().to_string())
                    .unwrap_or_else(|| "primary-interactive".into()),
                autonomy_mode: harness
                    .as_ref()
                    .map(|h| match h.autonomy_mode {
                        omegon_traits::OmegonAutonomyMode::OperatorDriven => {
                            "operator-driven".to_string()
                        }
                        omegon_traits::OmegonAutonomyMode::GuardedAutonomous => {
                            "guarded-autonomous".to_string()
                        }
                        omegon_traits::OmegonAutonomyMode::Autonomous => "autonomous".to_string(),
                    })
                    .unwrap_or_else(|| "operator-driven".into()),
                dispatcher: omegon_traits::IpcDispatcherSnapshot {
                    available_options: harness
                        .as_ref()
                        .map(|h| h.dispatcher.available_options.clone())
                        .unwrap_or_else(|| {
                            vec![
                                "F".into(),
                                "D".into(),
                                "C".into(),
                                "B".into(),
                                "A".into(),
                                "S".into(),
                            ]
                        }),
                    switch_state: harness
                        .as_ref()
                        .map(|h| h.dispatcher.switch_state.clone())
                        .unwrap_or_else(|| "idle".into()),
                    request_id: harness
                        .as_ref()
                        .and_then(|h| h.dispatcher.request_id.clone()),
                    expected_profile: harness
                        .as_ref()
                        .and_then(|h| h.dispatcher.expected_profile.clone()),
                    expected_model: harness
                        .as_ref()
                        .and_then(|h| h.dispatcher.expected_model.clone()),
                    active_profile: harness
                        .as_ref()
                        .and_then(|h| h.dispatcher.active_profile.clone())
                        .or_else(|| Some("B".into())),
                    active_model: harness
                        .as_ref()
                        .and_then(|h| h.dispatcher.active_model.clone()),
                    failure_code: harness
                        .as_ref()
                        .and_then(|h| h.dispatcher.failure_code.clone()),
                    note: harness.as_ref().and_then(|h| h.dispatcher.note.clone()),
                },
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
                execution_substrate: harness
                    .as_ref()
                    .map(|h| h.execution_substrate.clone())
                    .or_else(|| Some(crate::execution_substrate::detect())),
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
                acp_url: None,
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
            secrets: None,
            assistant_runs_db_path: std::sync::Arc::new(
                tempfile::tempdir()
                    .unwrap()
                    .path()
                    .join("assistant-runs.db"),
            ),
            daemon_events: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            daemon_status: std::sync::Arc::new(std::sync::Mutex::new(WebDaemonStatus::default())),
            pending_permissions: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            pending_operator_waits: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    fn write_blocked_agent(home: &std::path::Path) {
        let agent_dir = home.join("catalog").join("blocked-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("agent.toml"),
            r#"[agent]
id = "blocked-agent"
name = "Blocked Agent"
version = "0.1.0"
description = "Requires a missing secret"
domain = "security"

[secrets]
required = ["MISSING_REQUIRED_TOKEN"]
"#,
        )
        .unwrap();
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    #[tokio::test]
    async fn web_capabilities_describe_initial_browser_contract() {
        let response = get_web_capabilities().await.0;

        assert!(response.interactive);
        assert!(response.chat);
        assert!(response.hosted_web_ui);
        assert!(response.supports_tool_approval);
        assert!(response.supports_operator_wait);
        assert!(response.supports_auspex_proxy);
        assert!(response.surface_api);
        assert!(response.supports_session_resume);
        assert!(response.supports_attachments);
    }

    #[tokio::test]
    async fn web_launch_context_defaults_to_direct_omegon_owned() {
        let response = get_web_launch_context(HeaderMap::new()).await.0;

        assert_eq!(response.mode, "direct");
        assert_eq!(response.proxied_by, None);
        assert_eq!(response.back_url, None);
        assert_eq!(response.policy_owner, "omegon");
    }

    #[tokio::test]
    async fn web_surfaces_snapshot_exposes_expected_surface_keys() {
        let response = get_web_surfaces(axum::extract::State(test_state())).await.0;

        assert_eq!(
            response.schema_version,
            super::super::surfaces::WEB_SURFACES_SCHEMA_VERSION
        );
        assert_eq!(response.session_id, "default");
        assert_eq!(response.revision, 0);
        assert!(response.generated_at.contains('T'));
        assert!(response.surfaces.editor.accepts_prompt);
        assert_eq!(
            response.surfaces.editor.placeholder,
            "Ask anything, or type / for commands"
        );
        assert!(response.surfaces.editor.supports_attachments);
        assert!(response.surfaces.command.pending_prompt.is_none());
        assert!(response.surfaces.command_menu.available);
        assert_eq!(response.surfaces.conversation.segments.len(), 0);
        assert_eq!(response.surfaces.dashboard.session.turns, 0);
        assert!(!response.surfaces.footer.busy);
        assert_eq!(response.surfaces.memory_status.active_facts, 0);
        assert_eq!(response.surfaces.operations.active_child_runtimes, 0);
        assert_eq!(
            response.surfaces.settings.auth_mode.as_deref(),
            Some("ephemeral-bearer")
        );
    }

    #[tokio::test]
    async fn web_attachments_stage_and_read_browser_payload() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let cwd = std::env::current_dir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_current_dir(home.path()).unwrap();

        let (status, created) = post_web_attachment(Json(WebAttachmentCreateRequest {
            filename: "note.txt".to_string(),
            content_type: Some("text/plain".to_string()),
            data_base64: base64::engine::general_purpose::STANDARD.encode(b"hello"),
        }))
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created.filename, "note.txt");
        assert_eq!(created.size_bytes, 5);

        let fetched = get_web_attachment(axum::extract::Path(created.id.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(fetched.attachment.id, created.id);
        assert_eq!(
            fetched.data_base64,
            base64::engine::general_purpose::STANDARD.encode(b"hello")
        );

        std::env::set_current_dir(cwd).unwrap();
    }

    #[tokio::test]
    async fn web_attachments_reject_path_traversal_names() {
        let response = post_web_attachment(Json(WebAttachmentCreateRequest {
            filename: "../secret.txt".to_string(),
            content_type: None,
            data_base64: base64::engine::general_purpose::STANDARD.encode(b"bad"),
        }))
        .await;

        assert!(matches!(response, Err(StatusCode::BAD_REQUEST)));
    }

    #[tokio::test]
    async fn web_sessions_endpoint_lists_default_session() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let cwd = std::env::current_dir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_current_dir(home.path()).unwrap();

        let response = get_web_sessions().await.unwrap().0;

        std::env::set_current_dir(cwd).unwrap();
        assert_eq!(response.sessions.len(), 1);
        assert_eq!(response.sessions[0].session_id, "default");
        assert!(response.sessions[0].current);
    }

    #[tokio::test]
    async fn web_session_endpoint_returns_default_snapshot() {
        let response = get_web_session(
            axum::extract::State(test_state()),
            axum::extract::Path("default".to_string()),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.session.session_id, "default");
        assert!(response.session.current);
        assert_eq!(response.snapshot.session_id, "default");
    }

    #[tokio::test]
    async fn web_session_endpoint_404s_missing_session() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let cwd = std::env::current_dir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_current_dir(home.path()).unwrap();

        let response = get_web_session(
            axum::extract::State(test_state()),
            axum::extract::Path("missing".to_string()),
        )
        .await;

        std::env::set_current_dir(cwd).unwrap();
        assert!(matches!(response, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn web_action_submit_prompt_queues_web_command() {
        let mut state = test_state();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        state.command_tx = tx;

        let (status, response) = post_web_action(
            axum::extract::State(state),
            Json(WebActionRequest {
                schema_version: 1,
                action_id: "a1".to_string(),
                client_id: "browser-tab".to_string(),
                session_id: "default".to_string(),
                action: WebActionPayload::SubmitPrompt {
                    text: "hello web".to_string(),
                    attachments: Vec::new(),
                },
            }),
        )
        .await;

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(
            response.status,
            crate::ui_runtime::envelope::UiActionOutcomeStatus::Accepted
        );
        match rx.recv().await.unwrap() {
            super::super::WebCommand::UserPrompt(text) => assert_eq!(text, "hello web"),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn web_action_run_slash_command_queues_web_command() {
        let mut state = test_state();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        state.command_tx = tx;

        let (status, _) = post_web_action(
            axum::extract::State(state),
            Json(WebActionRequest {
                schema_version: 1,
                action_id: "a2".to_string(),
                client_id: "browser-tab".to_string(),
                session_id: "default".to_string(),
                action: WebActionPayload::RunSlashCommand {
                    raw: "/model list".to_string(),
                },
            }),
        )
        .await;

        assert_eq!(status, StatusCode::ACCEPTED);
        match rx.recv().await.unwrap() {
            super::super::WebCommand::SlashCommand { name, args, .. } => {
                assert_eq!(name, "model");
                assert_eq!(args, "list");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn web_action_rejects_unknown_session() {
        let (status, response) = post_web_action(
            axum::extract::State(test_state()),
            Json(WebActionRequest {
                schema_version: 1,
                action_id: "a3".to_string(),
                client_id: "browser-tab".to_string(),
                session_id: "missing".to_string(),
                action: WebActionPayload::CancelActiveTurn,
            }),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            response.status,
            crate::ui_runtime::envelope::UiActionOutcomeStatus::Rejected
        );
        assert_eq!(response.error.as_deref(), Some("unknown session_id"));
    }

    #[tokio::test]
    async fn assistant_runs_endpoint_returns_empty_runtime_projection() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let cwd = std::env::current_dir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_current_dir(home.path()).unwrap();
        let response = get_assistant_runs(axum::extract::State(test_state()))
            .await
            .unwrap()
            .0;
        std::env::set_current_dir(cwd).unwrap();
        assert!(response.runs.is_empty());
    }

    #[tokio::test]
    async fn assistant_run_endpoint_404s_missing_runtime_run() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let cwd = std::env::current_dir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_current_dir(home.path()).unwrap();
        let err = get_assistant_run(
            axum::extract::State(test_state()),
            axum::extract::Path("missing".into()),
        )
        .await
        .unwrap_err();
        std::env::set_current_dir(cwd).unwrap();
        assert_eq!(err, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn capabilities_endpoint_reports_blocked_assistant_launch_readiness() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let agent_dir = home.path().join("catalog").join("blocked-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("agent.toml"),
            r#"[agent]
id = "blocked-agent"
name = "Blocked Agent"
version = "0.1.0"
description = "Requires a missing secret"
domain = "security"

[secrets]
required = ["MISSING_REQUIRED_TOKEN"]
"#,
        )
        .unwrap();
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = get_capabilities(axum::extract::State(test_state()))
            .await
            .unwrap()
            .0;

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        let profile = response
            .assistant_profiles
            .iter()
            .find(|profile| profile.id == "blocked-agent")
            .expect("blocked assistant profile");
        assert_eq!(
            profile.launch_readiness.status,
            crate::capabilities::profiles::AssistantLaunchStatus::Blocked
        );
        assert!(profile.launch_readiness.blockers.iter().any(|blocker| {
            blocker.kind
                == crate::capabilities::profiles::AssistantLaunchBlockerKind::RequiredSecretMissing
                && blocker.id == "MISSING_REQUIRED_TOKEN"
        }));
    }

    #[tokio::test]
    async fn capability_assistants_endpoint_returns_compact_blocked_readiness() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        write_blocked_agent(home.path());
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = get_capability_assistants(axum::extract::State(test_state()))
            .await
            .unwrap()
            .0;

        restore_env("OMEGON_HOME", previous_home);

        let assistant = response
            .assistants
            .iter()
            .find(|assistant| assistant.id == "blocked-agent")
            .expect("blocked assistant list item");
        assert_eq!(
            assistant.launch_readiness.status,
            crate::capabilities::profiles::AssistantLaunchStatus::Blocked
        );
        assert_eq!(assistant.required_secret_count, 1);
        assert_eq!(assistant.blocker_count, 1);
    }

    #[tokio::test]
    async fn capability_assistant_readiness_endpoint_returns_single_assistant() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        write_blocked_agent(home.path());
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = get_capability_assistant_readiness(
            axum::extract::State(test_state()),
            axum::extract::Path("blocked-agent".to_string()),
        )
        .await
        .unwrap()
        .0;

        restore_env("OMEGON_HOME", previous_home);

        assert_eq!(response.assistant.id, "blocked-agent");
        assert_eq!(
            response.assistant.launch_readiness.status,
            crate::capabilities::profiles::AssistantLaunchStatus::Blocked
        );
    }

    #[tokio::test]
    async fn capability_assistant_readiness_endpoint_404s_missing_assistant() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = get_capability_assistant_readiness(
            axum::extract::State(test_state()),
            axum::extract::Path("missing".to_string()),
        )
        .await;

        restore_env("OMEGON_HOME", previous_home);

        assert!(matches!(response, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn capabilities_endpoints_skip_invalid_local_inventory_entries() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let ext_dir = home.path().join("extensions").join("valid-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            r#"[extension]
name = "valid-ext"
version = "0.1.0"
description = "Valid extension"

[runtime]
type = "native"
binary = "bin/valid-ext"
"#,
        )
        .unwrap();
        let broken_ext = home.path().join("extensions").join("broken-ext");
        std::fs::create_dir_all(&broken_ext).unwrap();
        std::fs::write(broken_ext.join("manifest.toml"), "not = [valid toml").unwrap();

        write_blocked_agent(home.path());
        let broken_agent = home.path().join("catalog").join("broken-agent");
        std::fs::create_dir_all(&broken_agent).unwrap();
        std::fs::write(
            broken_agent.join("agent.toml"),
            "[agent
id = ",
        )
        .unwrap();

        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let capabilities = get_capabilities(axum::extract::State(test_state()))
            .await
            .expect("capabilities should degrade bad local entries")
            .0;
        let assistants = get_capability_assistants(axum::extract::State(test_state()))
            .await
            .expect("assistant capabilities should degrade bad local entries")
            .0;

        restore_env("OMEGON_HOME", previous_home);

        assert!(
            capabilities
                .installed_extensions
                .iter()
                .any(|ext| ext.name == "valid-ext")
        );
        assert!(
            capabilities
                .assistant_profiles
                .iter()
                .any(|profile| profile.id == "blocked-agent")
        );
        assert!(
            assistants
                .assistants
                .iter()
                .any(|assistant| assistant.id == "blocked-agent")
        );
        assert!(
            !assistants
                .assistants
                .iter()
                .any(|assistant| assistant.id == "broken-agent")
        );
    }

    #[tokio::test]
    async fn capabilities_endpoint_reports_secret_metadata_without_values() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let ext_dir = home.path().join("extensions").join("secure-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            r#"[extension]
name = "secure-ext"
version = "0.1.0"
description = "Secure extension"

[runtime]
type = "native"
binary = "bin/secure-ext"

[secrets]
required = ["BRAVE_API_KEY"]
"#,
        )
        .unwrap();
        let secrets = Arc::new(omegon_secrets::SecretsManager::new(home.path()).unwrap());
        secrets
            .set_recipe("BRAVE_API_KEY", "env:OMEGON_TEST_BRAVE_KEY")
            .unwrap();
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let mut state = test_state();
        state.secrets = Some(secrets);
        let response = get_capabilities(axum::extract::State(state))
            .await
            .unwrap()
            .0;

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        let readiness = response
            .secret_readiness
            .secrets
            .iter()
            .find(|secret| secret.name == "BRAVE_API_KEY")
            .expect("BRAVE_API_KEY readiness");
        assert_eq!(
            readiness.status,
            crate::capabilities::secrets::SecretReadinessStatus::Configured
        );
        assert_eq!(readiness.recipe_kind.as_deref(), Some("env"));
        let payload = serde_json::to_string(&response).unwrap();
        assert!(!payload.contains("brave-test-key"));
        assert!(!payload.contains("OMEGON_TEST_BRAVE_KEY"));
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
                capability_grade: "B".into(),
                memory_available: true,
                cleave_available: true,
                ..Default::default()
            }))),
            ..Default::default()
        };

        let snap = build_snapshot(&state);
        let harness = snap.harness.expect("harness snapshot");
        assert_eq!(harness.thinking_level, "high");
        assert_eq!(harness.capability_grade, "B");
        assert!(harness.memory_available);
        assert!(harness.cleave_available);
        assert!(!harness.execution_substrate.paths.workspace.is_empty());
        assert_eq!(
            snap.instance.runtime.execution_substrate,
            Some(harness.execution_substrate)
        );
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
            caller_role: Some("admin".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };
        let (status, Json(payload)) =
            post_event(axum::extract::State(test_state()), headers, Json(event)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!payload.accepted);
    }

    #[tokio::test]
    async fn post_event_rejects_insufficient_role_for_shutdown_trigger() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let state = test_state();
        let event = DaemonEventEnvelope {
            event_id: "evt-shutdown-read".into(),
            source: "manual/test".into(),
            trigger_kind: "shutdown".into(),
            payload: serde_json::json!({}),
            caller_role: Some("read".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };
        let (status, Json(payload)) =
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!payload.accepted);
        assert_eq!(payload.queued_events, 0);
        assert!(state.daemon_events.lock().unwrap().is_empty());
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
            caller_role: Some("admin".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };
        let (status, Json(payload)) =
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(payload.accepted);
        assert_eq!(payload.queued_events, 1);
        assert_eq!(state.daemon_events.lock().unwrap().len(), 1);
        assert_eq!(state.daemon_status.lock().unwrap().queued_events, 1);
    }

    #[tokio::test]
    async fn post_event_accepts_new_session_trigger() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let state = test_state();
        let event = DaemonEventEnvelope {
            event_id: "evt-new-session".into(),
            source: "manual/test".into(),
            trigger_kind: "new-session".into(),
            payload: serde_json::json!({}),
            caller_role: Some("admin".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };
        let (status, Json(payload)) =
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(payload.accepted);
        assert_eq!(payload.queued_events, 1);
        assert_eq!(state.daemon_events.lock().unwrap().len(), 1);
        assert_eq!(state.daemon_status.lock().unwrap().queued_events, 1);
    }

    #[tokio::test]
    async fn post_event_accepts_shutdown_trigger() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let state = test_state();
        let event = DaemonEventEnvelope {
            event_id: "evt-shutdown".into(),
            source: "manual/test".into(),
            trigger_kind: "shutdown".into(),
            payload: serde_json::json!({}),
            caller_role: Some("admin".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };
        let (status, Json(payload)) =
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await;
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
            context_class: Some("massive".into()),
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
                        failure_kind: None,
                        duration_secs: None,
                        supervision_mode: None,
                        pid: None,
                        last_tool: None,
                        last_tool_activity: None,
                        last_turn: None,
                        tasks: Vec::new(),
                        tasks_done: 0,
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
        assert_eq!(
            runtime.model.as_deref(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(runtime.context_class.as_deref(), Some("massive"));
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

// ── Eval API endpoints ─────────────────────────────────────────────────

/// GET /api/evals — list all stored score cards (summary view).
pub async fn get_evals() -> Result<Json<EvalListResponse>, StatusCode> {
    let entries = crate::eval::store::list().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rankings = crate::eval::store::rankings_from(&entries)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(EvalListResponse { entries, rankings }))
}

#[derive(Serialize)]
pub struct EvalListResponse {
    pub entries: Vec<crate::eval::store::ScoreCardEntry>,
    pub rankings: Vec<crate::eval::store::RankingEntry>,
}

/// GET /api/evals/:id — full score card by storage ID.
pub async fn get_eval(
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<crate::eval::report::ScoreCard>, StatusCode> {
    let card = crate::eval::store::load(&id).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(card))
}

/// GET /api/evals/compare — diff two score cards.
/// Query params: ?a=<id>&b=<id>
pub async fn get_eval_compare(
    axum::extract::Query(params): axum::extract::Query<CompareParams>,
) -> Result<Json<crate::eval::store::ScoreCardDiff>, StatusCode> {
    let diff =
        crate::eval::store::compare(&params.a, &params.b).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(diff))
}

#[derive(serde::Deserialize)]
pub struct CompareParams {
    pub a: String,
    pub b: String,
}

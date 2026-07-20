//! JSON API endpoints for the web dashboard.
//!
//! GET /api/state — full agent state snapshot.
//! Designed to be the canonical state shape that any web UI consumes.

use crate::status::HarnessStatus;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use futures_util::stream;
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

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatusResponse {
    pub schema_version: u8,
    pub state: ControlPlaneState,
    pub ready: bool,
    pub startup: Option<RuntimeStartupSummary>,
    pub daemon: super::WebDaemonStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStartupSummary {
    pub http_base: String,
    pub ws_url: String,
    pub acp_url: Option<String>,
    pub auth_mode: String,
    pub auth_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeCapabilitiesResponse {
    pub schema_version: u8,
    pub probes: RuntimeProbeCapabilities,
    pub browser_web: WebCapabilityDescriptor,
    pub rbac: super::rbac::RbacPolicyDescriptor,
    pub acp_websocket: bool,
    pub acp_websocket_path: &'static str,
    pub daemon_event_ingress: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatusResponse {
    pub schema_version: u8,
    pub providers: Vec<crate::status::ProviderStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtensionsStatusResponse {
    pub schema_version: u8,
    pub extensions: Vec<crate::capabilities::extensions::ExtensionCapabilitySummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceLeasesResponse {
    pub schema_version: u8,
    pub cwd: String,
    pub leases: Vec<WorkspaceLeaseStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceLeaseStatus {
    pub instance_id: String,
    pub lease: crate::workspace::types::WorkspaceLease,
}

#[derive(Serialize)]
pub struct LifecycleSnapshotResponse {
    pub schema_version: u8,
    pub design: DesignSnapshot,
    pub openspec: OpenSpecSnapshot,
}

#[derive(Serialize)]
pub struct LifecycleDesignResponse {
    pub schema_version: u8,
    pub design: DesignSnapshot,
}

#[derive(Serialize)]
pub struct LifecycleDesignNodeResponse {
    pub schema_version: u8,
    pub node: NodeBrief,
}

#[derive(Serialize)]
pub struct LifecycleDesignReadyResponse {
    pub schema_version: u8,
    pub nodes: Vec<crate::lifecycle::query::ReadyNode>,
}

#[derive(Serialize)]
pub struct LifecycleDesignBlockedResponse {
    pub schema_version: u8,
    pub nodes: Vec<crate::lifecycle::query::BlockedNode>,
}

#[derive(Serialize)]
pub struct LifecycleDesignFrontierResponse {
    pub schema_version: u8,
    pub nodes: Vec<crate::lifecycle::query::FrontierNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbeCapabilities {
    pub healthz: bool,
    pub readyz: bool,
    pub startup: bool,
    pub state_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct EventAccepted {
    pub accepted: bool,
    pub queued_events: usize,
}

pub enum EventIngressOutcome {
    Accepted(StatusCode, EventAccepted),
    Rbac(StatusCode, super::rbac::RbacErrorResponse),
}

impl IntoResponse for EventIngressOutcome {
    fn into_response(self) -> Response {
        match self {
            Self::Accepted(status, payload) => (status, Json(payload)).into_response(),
            Self::Rbac(status, payload) => (status, Json(payload)).into_response(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonEventsResponse {
    pub schema_version: u8,
    pub queued_events: usize,
    pub processed_events: usize,
    pub events: Vec<DaemonEventEnvelope>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonEventStreamEnvelope {
    pub schema_version: u8,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSessionListResponse {
    pub sessions: Vec<WebSessionSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSessionShowResponse {
    pub schema_version: u8,
    pub session: WebSessionSummary,
    pub allocation_mode: String,
    pub links: WebSessionLinks,
    pub snapshot: super::surfaces::WebSurfacesSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSessionLinks {
    pub surfaces: Option<String>,
    pub actions: Option<String>,
    pub stream: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeSessionCreateRequest {
    #[serde(default)]
    pub assistant_profile_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NativeSessionCreateResponse {
    pub schema_version: u8,
    pub session: WebSessionSummary,
    pub allocation_mode: String,
    pub assistant_profile_id: Option<String>,
    pub assistant: Option<crate::capabilities::profiles::AssistantListItem>,
    pub links: WebSessionLinks,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebSessionSummary {
    pub session_id: String,
    pub cwd: String,
    pub created_at: String,
    pub turns: u32,
    pub tool_calls: u32,
    pub description: String,
    pub last_prompt_snippet: String,
    pub current: bool,
}

fn web_session_summary(entry: crate::session::SessionEntry) -> WebSessionSummary {
    let description = crate::session::session_display_description(&entry.meta);
    WebSessionSummary {
        session_id: entry.meta.session_id,
        cwd: entry.meta.cwd,
        created_at: entry.meta.created_at,
        turns: entry.meta.turns,
        tool_calls: entry.meta.tool_calls,
        description,
        last_prompt_snippet: entry.meta.last_prompt_snippet,
        current: false,
    }
}

fn native_default_session_links() -> WebSessionLinks {
    WebSessionLinks {
        surfaces: Some("/api/sessions/default/surfaces".to_string()),
        actions: Some("/api/sessions/default/actions".to_string()),
        stream: Some("/api/sessions/default/surfaces/stream".to_string()),
    }
}

const NATIVE_SESSION_ALLOCATION_MODE: &str = "singleton-live";
const HISTORICAL_SESSION_ALLOCATION_MODE: &str = "historical-read-only";

fn historical_web_session_links(session_id: &str) -> WebSessionLinks {
    WebSessionLinks {
        surfaces: Some(format!("/api/web/sessions/{session_id}/surfaces")),
        actions: None,
        stream: None,
    }
}

fn default_live_session_summary(state: &WebState) -> Result<WebSessionSummary, StatusCode> {
    let cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let session = state
        .handles
        .session
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(WebSessionSummary {
        session_id: "default".to_string(),
        cwd: cwd.to_string_lossy().to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        turns: session.turns,
        tool_calls: session.tool_calls,
        description: "Current live session".to_string(),
        last_prompt_snippet: "Current live session".to_string(),
        current: true,
    })
}

fn validate_native_session_id(session_id: &str) -> Result<(), StatusCode> {
    if session_id == "default" {
        Ok(())
    } else {
        Err(StatusCode::NOT_FOUND)
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
        #[serde(default)]
        always: bool,
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
    /// HTTP snapshot/query surface, anchored by GET /api/web/surfaces.
    pub surface_api: bool,
    /// WebSocket live event surface, anchored by GET /api/web/surfaces/stream.
    pub surface_stream: bool,
    /// HTTP action submission surface, anchored by POST /api/web/actions.
    pub actions_api: bool,
    /// Legacy dashboard/control websocket remains available for compatibility.
    pub legacy_ws: bool,
    /// Browser-compatible ACP transport for agent session protocol frames.
    pub acp_websocket_path: &'static str,
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

#[derive(Debug, Clone, Serialize)]
pub struct AssistantProfileDetailResponse {
    pub schema_version: u8,
    pub profile: crate::capabilities::profiles::AssistantProfileSummary,
    pub assistant: crate::capabilities::profiles::AssistantListItem,
    pub readiness_href: String,
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

pub async fn get_assistant_profile(
    State(state): State<WebState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<AssistantProfileDetailResponse>, StatusCode> {
    let snapshot = capability_inventory_snapshot(state)?;
    let profile = snapshot
        .assistant_profiles
        .into_iter()
        .find(|profile| profile.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let assistant = snapshot
        .assistant_list
        .into_iter()
        .find(|assistant| assistant.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(AssistantProfileDetailResponse {
        schema_version: 1,
        readiness_href: format!("/api/capabilities/assistants/{id}/readiness"),
        profile,
        assistant,
    }))
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
                checked_names: Vec::new(),
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

/// POST /api/sessions — create/attach the native first-party live session.
///
/// Phase 1 exposes the singleton in-process agent session through the native
/// session resource shape. Additional durable/multi-session allocation can
/// extend this without changing the client-facing links.
pub async fn post_native_session(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(request): Json<NativeSessionCreateRequest>,
) -> Result<(StatusCode, Json<NativeSessionCreateResponse>), StatusCode> {
    let principal =
        super::rbac::principal_from_headers(&state, &headers).map_err(|error| error.status())?;
    if let Err(error) = super::rbac::require_principal_operation(
        &principal,
        omegon_rbac::OmegonOperation::NativeSessionCreate,
        &super::rbac::RbacContext {
            route: "/api/sessions",
            assistant_profile_id: request.assistant_profile_id.as_deref(),
            ..super::rbac::RbacContext::default()
        },
    ) {
        return Err(error.status());
    }

    if let Some(cwd) = request.cwd {
        let current = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if cwd != current.to_string_lossy() {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let assistant = if let Some(profile_id) = request.assistant_profile_id {
        let snapshot = capability_inventory_snapshot(state.clone())?;
        Some(
            snapshot
                .assistant_list
                .into_iter()
                .find(|assistant| assistant.id == profile_id)
                .ok_or(StatusCode::NOT_FOUND)?,
        )
    } else {
        None
    };
    let assistant_profile_id = assistant.as_ref().map(|assistant| assistant.id.clone());
    let session = default_live_session_summary(&state)?;
    Ok((
        StatusCode::CREATED,
        Json(NativeSessionCreateResponse {
            schema_version: 1,
            allocation_mode: NATIVE_SESSION_ALLOCATION_MODE.to_string(),
            assistant_profile_id,
            assistant,
            links: native_default_session_links(),
            session,
        }),
    ))
}

/// GET /api/sessions/{session_id} — native first-party session metadata.
pub async fn get_native_session(
    State(state): State<WebState>,
    headers: HeaderMap,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<WebSessionShowResponse>, StatusCode> {
    validate_native_session_id(&session_id)?;
    let principal =
        super::rbac::principal_from_headers(&state, &headers).map_err(|error| error.status())?;
    if let Err(error) = super::rbac::require_principal_operation(
        &principal,
        omegon_rbac::OmegonOperation::NativeSessionRead,
        &super::rbac::RbacContext {
            route: "/api/sessions/{session_id}",
            session_id: Some(&session_id),
            ..super::rbac::RbacContext::default()
        },
    ) {
        return Err(error.status());
    }

    Ok(Json(WebSessionShowResponse {
        schema_version: 1,
        session: default_live_session_summary(&state)?,
        allocation_mode: NATIVE_SESSION_ALLOCATION_MODE.to_string(),
        links: native_default_session_links(),
        snapshot: super::surfaces::project_web_surfaces(&state),
    }))
}

/// GET /api/sessions/{session_id}/surfaces — native session-scoped surface snapshot.
pub async fn get_native_session_surfaces(
    State(state): State<WebState>,
    headers: HeaderMap,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<super::surfaces::WebSurfacesSnapshot>, StatusCode> {
    validate_native_session_id(&session_id)?;
    let principal =
        super::rbac::principal_from_headers(&state, &headers).map_err(|error| error.status())?;
    if let Err(error) = super::rbac::require_principal_operation(
        &principal,
        omegon_rbac::OmegonOperation::SurfaceRead,
        &super::rbac::RbacContext {
            route: "/api/sessions/{session_id}/surfaces",
            session_id: Some(&session_id),
            ..super::rbac::RbacContext::default()
        },
    ) {
        return Err(error.status());
    }

    Ok(Json(super::surfaces::project_web_surfaces(&state)))
}

/// POST /api/sessions/{session_id}/actions — native session-scoped action ingress.
pub async fn post_native_session_action(
    State(state): State<WebState>,
    headers: HeaderMap,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(mut request): Json<WebActionRequest>,
) -> (
    StatusCode,
    Json<crate::ui_runtime::envelope::UiActionOutcomeEnvelope>,
) {
    let action_id = request.action_id.clone();
    if validate_native_session_id(&session_id).is_err() {
        return (
            StatusCode::NOT_FOUND,
            Json(
                crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                    session_id,
                    action_id,
                    "unknown session_id",
                ),
            ),
        );
    }

    let principal = match super::rbac::principal_from_headers(&state, &headers) {
        Ok(principal) => principal,
        Err(error) => {
            return (
                error.status(),
                Json(
                    crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                        session_id,
                        action_id,
                        error.response().reason,
                    ),
                ),
            );
        }
    };
    if let Err(error) = super::rbac::require_principal_operation(
        &principal,
        omegon_rbac::OmegonOperation::NativeSessionAction,
        &super::rbac::RbacContext {
            route: "/api/sessions/{session_id}/actions",
            session_id: Some(&session_id),
            action_id: Some(&action_id),
            client_id: Some(&request.client_id),
            ..super::rbac::RbacContext::default()
        },
    ) {
        return (
            error.status(),
            Json(
                crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                    session_id,
                    action_id,
                    error.response().reason,
                ),
            ),
        );
    }

    request.session_id = session_id;
    post_web_action(State(state), headers, Json(request)).await
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
            description: "Current live session".to_string(),
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
            description: "Current live session".to_string(),
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

    let links = if session.session_id == "default" {
        native_default_session_links()
    } else {
        historical_web_session_links(&session.session_id)
    };

    let allocation_mode = if session.current {
        NATIVE_SESSION_ALLOCATION_MODE
    } else {
        HISTORICAL_SESSION_ALLOCATION_MODE
    };

    Ok(Json(WebSessionShowResponse {
        schema_version: 1,
        session,
        allocation_mode: allocation_mode.to_string(),
        links,
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
    // Also materialize under the original (sanitized) filename so downstream
    // image detection — which keys off the extension — works when this
    // attachment is later resolved into a prompt's attachment paths.
    let named_path = dir.join(&filename);
    if named_path != data_path {
        std::fs::write(&named_path, &bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
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

/// Resolve staged attachment ids to on-disk file paths (with original
/// extensions) for inclusion in a prompt. Returns `Err(reason)` naming the
/// first id that is malformed, unknown, or missing its staged file.
fn resolve_web_attachment_paths(ids: &[String]) -> Result<Vec<String>, String> {
    let mut paths = Vec::with_capacity(ids.len());
    for id in ids {
        if id.contains(['/', '\\', '\0']) || id.trim().is_empty() {
            return Err(format!("invalid attachment id '{id}'"));
        }
        let dir = web_attachment_root().join(id);
        let meta_path = dir.join("meta.json");
        let meta = std::fs::read_to_string(&meta_path)
            .map_err(|_| format!("unknown or expired attachment '{id}'"))?;
        let attachment: WebAttachmentResponse = serde_json::from_str(&meta)
            .map_err(|_| format!("corrupt attachment metadata for '{id}'"))?;
        let named = dir.join(&attachment.filename);
        let path = if named.exists() {
            named
        } else {
            dir.join("data.bin")
        };
        if !path.exists() {
            return Err(format!("attachment data missing for '{id}'"));
        }
        paths.push(path.to_string_lossy().into_owned());
    }
    Ok(paths)
}

/// POST /api/web/actions — browser-native semantic action ingress.
pub async fn post_web_action(
    State(state): State<WebState>,
    headers: HeaderMap,
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

    let principal = match super::rbac::principal_from_headers(&state, &headers) {
        Ok(principal) => principal,
        Err(error) => {
            return (
                error.status(),
                Json(
                    crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                        request.session_id,
                        request.action_id,
                        error.response().reason,
                    ),
                ),
            );
        }
    };
    if let Err(error) = super::rbac::require_principal_operation(
        &principal,
        omegon_rbac::OmegonOperation::NativeSessionAction,
        &super::rbac::RbacContext {
            route: "/api/web/actions",
            session_id: Some(&request.session_id),
            action_id: Some(&request.action_id),
            client_id: Some(&request.client_id),
            ..super::rbac::RbacContext::default()
        },
    ) {
        return (
            error.status(),
            Json(
                crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                    request.session_id,
                    request.action_id,
                    error.response().reason,
                ),
            ),
        );
    }

    let send_result = match request.action {
        WebActionPayload::SubmitPrompt { text, attachments } => {
            let image_paths = if attachments.is_empty() {
                Vec::new()
            } else {
                match resolve_web_attachment_paths(&attachments) {
                    Ok(paths) => paths,
                    Err(reason) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(
                                crate::ui_runtime::envelope::UiActionOutcomeEnvelope::rejected(
                                    request.session_id,
                                    request.action_id,
                                    &reason,
                                ),
                            ),
                        );
                    }
                }
            };
            // An image-only prompt (text empty, attachments present) is valid.
            if text.trim().is_empty() && image_paths.is_empty() {
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
            let segment_text = if text.trim().is_empty() {
                format!("[{} attachment(s)]", image_paths.len())
            } else if image_paths.is_empty() {
                text.clone()
            } else {
                format!("{text}  [{} attachment(s)]", image_paths.len())
            };
            state.record_user_segment(&segment_text);
            state
                .command_tx
                .try_send(super::WebCommand::UserPrompt { text, image_paths })
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
        WebActionPayload::RespondPermission {
            request_id,
            allow,
            always,
        } => {
            let decision = if always {
                omegon_traits::PermissionResponse::AlwaysAllow
            } else if allow {
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
                            match decision {
                                omegon_traits::PermissionResponse::Allow => "permission allowed",
                                omegon_traits::PermissionResponse::AlwaysAllow => {
                                    "permission always allowed"
                                }
                                omegon_traits::PermissionResponse::Deny => "permission denied",
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
    headers: HeaderMap,
) -> Result<Json<super::surfaces::WebSurfacesSnapshot>, StatusCode> {
    let principal =
        super::rbac::principal_from_headers(&state, &headers).map_err(|error| error.status())?;
    if let Err(error) = super::rbac::require_principal_operation(
        &principal,
        omegon_rbac::OmegonOperation::SurfaceRead,
        &super::rbac::RbacContext {
            route: "/api/web/surfaces",
            ..super::rbac::RbacContext::default()
        },
    ) {
        return Err(error.status());
    }

    Ok(Json(super::surfaces::project_web_surfaces(&state)))
}

/// GET /api/web/capabilities — web/Auspex capability descriptor.
fn web_capabilities_descriptor() -> WebCapabilityDescriptor {
    WebCapabilityDescriptor {
        interactive: true,
        chat: true,
        hosted_web_ui: true,
        surface_api: true,
        surface_stream: true,
        actions_api: true,
        legacy_ws: true,
        acp_websocket_path: "/api/acp",
        supports_tool_approval: true,
        supports_operator_wait: true,
        supports_session_resume: true,
        supports_attachments: true,
        supports_auspex_proxy: true,
    }
}

pub async fn get_web_capabilities() -> Json<WebCapabilityDescriptor> {
    Json(web_capabilities_descriptor())
}

/// GET /api/web/launch-context — describes how the web UI was launched.
pub async fn get_web_launch_context(headers: HeaderMap) -> Json<WebLaunchContextResponse> {
    let proxied_by = headers
        .get("omegon-principal-issuer")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.to_string());
    let back_url = headers
        .get("omegon-back-url")
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

/// GET /api/providers/status — provider auth/runtime readiness from harness status.
pub async fn get_providers_status(
    State(state): State<WebState>,
) -> Result<Json<ProviderStatusResponse>, StatusCode> {
    let providers = state
        .handles
        .harness
        .as_ref()
        .and_then(|harness| harness.lock().ok())
        .map(|harness| harness.providers.clone())
        .unwrap_or_default();
    Ok(Json(ProviderStatusResponse {
        schema_version: 1,
        providers,
    }))
}

/// GET /api/extensions — installed extension capability/status inventory.
pub async fn get_extensions_status(
    State(state): State<WebState>,
) -> Result<Json<ExtensionsStatusResponse>, StatusCode> {
    let snapshot = capability_inventory_snapshot(state)?;
    Ok(Json(ExtensionsStatusResponse {
        schema_version: 1,
        extensions: snapshot.installed_extensions,
    }))
}

/// GET /api/workspaces/leases — active workspace lease inventory for this checkout.
pub async fn get_workspace_leases_status() -> Result<Json<WorkspaceLeasesResponse>, StatusCode> {
    let cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut leases: Vec<_> = crate::workspace::runtime::read_all_active_leases(&cwd)
        .into_iter()
        .map(|(instance_id, lease)| WorkspaceLeaseStatus { instance_id, lease })
        .collect();
    leases.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    Ok(Json(WorkspaceLeasesResponse {
        schema_version: 1,
        cwd: cwd.display().to_string(),
        leases,
    }))
}

/// GET /api/lifecycle/snapshot — lifecycle read model for web/console clients.
pub async fn get_lifecycle_snapshot(
    State(state): State<WebState>,
) -> Json<LifecycleSnapshotResponse> {
    let snapshot = build_snapshot(&state);
    Json(LifecycleSnapshotResponse {
        schema_version: 1,
        design: snapshot.design,
        openspec: snapshot.openspec,
    })
}

/// GET /api/lifecycle/design — design tree read model.
pub async fn get_lifecycle_design(State(state): State<WebState>) -> Json<LifecycleDesignResponse> {
    let snapshot = build_snapshot(&state);
    Json(LifecycleDesignResponse {
        schema_version: 1,
        design: snapshot.design,
    })
}

/// GET /api/lifecycle/design/{id} — compact design node read model.
pub async fn get_lifecycle_design_node(
    axum::extract::Path(id): axum::extract::Path<String>,
    State(state): State<WebState>,
) -> Result<Json<LifecycleDesignNodeResponse>, StatusCode> {
    let Some(lifecycle) = state.handles.lifecycle.as_ref() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let provider = lifecycle.provider();
    let guard = provider
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(node) = guard.get_node(&id) else {
        return Err(StatusCode::NOT_FOUND);
    };
    Ok(Json(LifecycleDesignNodeResponse {
        schema_version: 1,
        node: node_brief(node),
    }))
}

fn lifecycle_nodes(
    state: &WebState,
) -> Result<std::collections::HashMap<String, crate::lifecycle::types::DesignNode>, StatusCode> {
    let Some(lifecycle) = state.handles.lifecycle.as_ref() else {
        return Ok(std::collections::HashMap::new());
    };
    let provider = lifecycle.provider();
    let guard = provider
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(guard.all_nodes().clone())
}

/// GET /api/lifecycle/design/ready — decided nodes whose dependencies are implemented.
pub async fn get_lifecycle_design_ready(
    State(state): State<WebState>,
) -> Result<Json<LifecycleDesignReadyResponse>, StatusCode> {
    let nodes = lifecycle_nodes(&state)?;
    Ok(Json(LifecycleDesignReadyResponse {
        schema_version: 1,
        nodes: crate::lifecycle::query::ready(&nodes),
    }))
}

/// GET /api/lifecycle/design/blocked — blocked nodes and nodes blocked by dependencies.
pub async fn get_lifecycle_design_blocked(
    State(state): State<WebState>,
) -> Result<Json<LifecycleDesignBlockedResponse>, StatusCode> {
    let nodes = lifecycle_nodes(&state)?;
    Ok(Json(LifecycleDesignBlockedResponse {
        schema_version: 1,
        nodes: crate::lifecycle::query::blocked(&nodes),
    }))
}

/// GET /api/lifecycle/design/frontier — nodes with unresolved open questions.
pub async fn get_lifecycle_design_frontier(
    State(state): State<WebState>,
) -> Result<Json<LifecycleDesignFrontierResponse>, StatusCode> {
    let nodes = lifecycle_nodes(&state)?;
    Ok(Json(LifecycleDesignFrontierResponse {
        schema_version: 1,
        nodes: crate::lifecycle::query::frontier(&nodes),
    }))
}

/// GET /api/runtime/status — runtime/control-plane readiness snapshot.
pub async fn get_runtime_status(
    State(state): State<WebState>,
) -> Result<Json<RuntimeStatusResponse>, StatusCode> {
    let control_state = *state
        .control_plane_state
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let startup = state
        .startup_info
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .clone();
    let daemon = state
        .daemon_status
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .clone();
    Ok(Json(RuntimeStatusResponse {
        schema_version: 1,
        state: control_state,
        ready: matches!(control_state, ControlPlaneState::Ready),
        startup: startup.map(|info| RuntimeStartupSummary {
            http_base: info.http_base,
            ws_url: info.ws_url,
            acp_url: info.acp_url,
            auth_mode: info.auth_mode,
            auth_source: info.auth_source,
        }),
        daemon,
    }))
}

/// GET /api/runtime/capabilities — stable runtime API capability descriptor.
pub async fn get_runtime_capabilities(
    State(state): State<WebState>,
) -> Json<RuntimeCapabilitiesResponse> {
    let role = super::rbac::current_web_role(&state);
    Json(RuntimeCapabilitiesResponse {
        schema_version: 1,
        probes: RuntimeProbeCapabilities {
            healthz: true,
            readyz: true,
            startup: true,
            state_snapshot: true,
        },
        browser_web: web_capabilities_descriptor(),
        rbac: super::rbac::policy_descriptor(role),
        acp_websocket: true,
        acp_websocket_path: "/api/acp",
        daemon_event_ingress: true,
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

pub fn daemon_event_stream_envelope(
    event: &omegon_traits::AgentEvent,
) -> Option<DaemonEventStreamEnvelope> {
    use omegon_traits::AgentEvent;
    let (event_type, payload) = match event {
        AgentEvent::TurnStart { turn } => {
            ("session.turn_started", serde_json::json!({ "turn": turn }))
        }
        AgentEvent::TurnEnd(summary) => (
            "session.turn_ended",
            serde_json::json!({
                "turn": summary.turn,
                "tool_calls": summary.stats_tool_calls,
                "model": summary.model,
                "provider": summary.provider,
                "estimated_tokens": summary.estimated_tokens,
            }),
        ),
        AgentEvent::AgentEnd => ("session.ended", serde_json::json!({})),
        AgentEvent::RouteChanged {
            state,
            selected,
            serving,
            warning,
            message,
        } => (
            "provider.status_changed",
            serde_json::json!({
                "state": state,
                "selected": selected,
                "serving": serving,
                "warning": warning,
                "message": message,
            }),
        ),
        AgentEvent::SkillActivation { event } => (
            "extension.status_changed",
            serde_json::to_value(event).unwrap_or_else(|_| serde_json::json!({})),
        ),
        AgentEvent::OperatorCopyBlock {
            label,
            text,
            kind,
            copy_attempt,
        } => (
            "session.operator_copy_block",
            serde_json::json!({
                "label": label,
                "text": text,
                "kind": kind.as_str(),
                "copy_status": copy_attempt.as_ref().map(|status| status.label()),
            }),
        ),
        AgentEvent::HarnessStatusChanged { status_json } => {
            ("runtime.status_changed", status_json.clone())
        }
        AgentEvent::PlanUpdated { projection } => (
            "lifecycle.snapshot_changed",
            serde_json::to_value(projection).unwrap_or_else(|_| serde_json::json!({})),
        ),
        AgentEvent::WebDashboardStarted { startup_json } => {
            ("runtime.web_started", startup_json.clone())
        }
        AgentEvent::RuntimeQueueUpdated { snapshot_json } => {
            ("runtime.queue_changed", snapshot_json.clone())
        }
        AgentEvent::RuntimeTurnLifecycleUpdated { snapshot_json } => {
            ("runtime.turn_lifecycle_changed", snapshot_json.clone())
        }
        AgentEvent::ContextUpdated {
            tokens,
            context_window,
            context_class,
            thinking_level,
        } => (
            "runtime.context_changed",
            serde_json::json!({
                "tokens": tokens,
                "context_window": context_window,
                "context_class": context_class,
                "thinking_level": thinking_level,
            }),
        ),
        _ => return None,
    };
    Some(DaemonEventStreamEnvelope {
        schema_version: 1,
        event_type: event_type.to_string(),
        payload,
    })
}

/// GET /api/events/stream — Server-Sent Events stream for daemon/app dashboards.
pub async fn get_events_stream(
    State(state): State<WebState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.events_tx.subscribe();
    let stream = stream::unfold(rx, move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Some(envelope) = daemon_event_stream_envelope(&event) {
                        let data =
                            serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
                        let event = Event::default()
                            .event(envelope.event_type.clone())
                            .data(data);
                        return Some((Ok(event), rx));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let envelope = DaemonEventStreamEnvelope {
                        schema_version: 1,
                        event_type: "stream.lagged".to_string(),
                        payload: serde_json::json!({
                            "skipped_events": skipped,
                            "recovery": {
                                "action": "refetch_snapshot",
                                "href": "/api/events"
                            }
                        }),
                    };
                    let data =
                        serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
                    let event = Event::default().event("stream.lagged").data(data);
                    return Some((Ok(event), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// GET /api/events — read-only daemon event queue snapshot for web/console clients.
pub async fn get_events(
    State(state): State<WebState>,
) -> Result<Json<DaemonEventsResponse>, StatusCode> {
    let events = state
        .daemon_events
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .clone();
    let status = state
        .daemon_status
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .clone();
    Ok(Json(DaemonEventsResponse {
        schema_version: 1,
        queued_events: events.len(),
        processed_events: status.processed_events,
        events,
    }))
}

/// POST /api/events — authenticated local event ingress for daemon/runtime triggers.
pub async fn post_event(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(event): Json<DaemonEventEnvelope>,
) -> EventIngressOutcome {
    let principal = match super::rbac::principal_from_headers(&state, &headers) {
        Ok(principal) => principal,
        Err(error) => return EventIngressOutcome::Rbac(error.status(), error.response()),
    };
    if let Some(label) = event.caller_role.as_deref() {
        let asserted_role = match super::rbac::parse_control_role(label) {
            Ok(role) => role,
            Err(error) => return EventIngressOutcome::Rbac(error.status(), error.response()),
        };
        if asserted_role != principal.role {
            return EventIngressOutcome::Rbac(
                StatusCode::FORBIDDEN,
                super::rbac::RbacError::Forbidden {
                    role: principal.role,
                    operation: omegon_rbac::OmegonOperation::EventIngress,
                }
                .response(),
            );
        }
    } else {
        return EventIngressOutcome::Rbac(
            StatusCode::BAD_REQUEST,
            super::rbac::RbacError::InvalidRole {
                role: "missing".to_string(),
            }
            .response(),
        );
    }
    if let Err(error) = super::rbac::require_principal_operation(
        &principal,
        omegon_rbac::OmegonOperation::EventIngress,
        &super::rbac::RbacContext {
            route: "/api/events",
            daemon_event_id: Some(&event.event_id),
            trigger_kind: Some(&event.trigger_kind),
            ..super::rbac::RbacContext::default()
        },
    ) {
        return EventIngressOutcome::Rbac(error.status(), error.response());
    }

    let caller_role = super::rbac::role_to_control_role(principal.role);
    let required = crate::control_actions::classify_daemon_trigger(&event.trigger_kind).role;
    if !crate::control_actions::is_role_sufficient(caller_role, required) {
        return EventIngressOutcome::Rbac(
            StatusCode::FORBIDDEN,
            super::rbac::RbacError::Forbidden {
                role: principal.role,
                operation: omegon_rbac::OmegonOperation::EventIngress,
            }
            .response(),
        );
    }

    match state.daemon_events.lock() {
        Ok(mut queue) => {
            queue.push(event);
            let queued_events = queue.len();
            if let Ok(mut status) = state.daemon_status.lock() {
                status.queued_events = queued_events;
            }
            EventIngressOutcome::Accepted(
                StatusCode::ACCEPTED,
                EventAccepted {
                    accepted: true,
                    queued_events,
                },
            )
        }
        Err(_) => EventIngressOutcome::Accepted(
            StatusCode::INTERNAL_SERVER_ERROR,
            EventAccepted {
                accepted: false,
                queued_events: 0,
            },
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

fn node_brief(node: &crate::lifecycle::types::DesignNode) -> NodeBrief {
    NodeBrief {
        id: node.id.clone(),
        title: node.title.clone(),
        status: node.status.as_str().to_string(),
        parent: node.parent.clone(),
        open_questions: node.open_questions.len(),
        openspec_change: node.openspec_change.clone(),
        dependencies: node.dependencies.clone(),
        branches: node.branches.clone(),
        tags: node.tags.clone(),
    }
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

            let brief = node_brief(node);

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

    fn auth_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        headers
    }

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
                web_authority: crate::web::WebAuthorityConfig::default().status(),
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
            conversation_log: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::VecDeque::new(),
            )),
            plan_surface: std::sync::Arc::new(std::sync::Mutex::new(
                omegon_traits::PlanSurfaceProjection::default(),
            )),
            tool_runs: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::VecDeque::new()),
            ),
            web_role: styrene_rbac::Role::Admin,
            web_authority: crate::web::WebAuthorityConfig::default(),
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

    fn event_accepted_response(outcome: EventIngressOutcome) -> (StatusCode, EventAccepted) {
        match outcome {
            EventIngressOutcome::Accepted(status, payload) => (status, payload),
            EventIngressOutcome::Rbac(status, _error) => (
                status,
                EventAccepted {
                    accepted: false,
                    queued_events: 0,
                },
            ),
        }
    }

    fn event_rbac_response(
        outcome: EventIngressOutcome,
    ) -> (StatusCode, super::super::rbac::RbacErrorResponse) {
        match outcome {
            EventIngressOutcome::Rbac(status, payload) => (status, payload),
            EventIngressOutcome::Accepted(status, _) => {
                panic!("expected RBAC response, got {status}")
            }
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
        assert!(response.surface_stream);
        assert!(response.actions_api);
        assert!(response.legacy_ws);
        assert_eq!(response.acp_websocket_path, "/api/acp");
        assert!(response.supports_session_resume);
        assert!(response.supports_attachments);
    }

    #[tokio::test]
    async fn runtime_capabilities_describe_registered_runtime_contract() {
        let response = get_runtime_capabilities(axum::extract::State(test_state()))
            .await
            .0;

        assert_eq!(response.schema_version, 1);
        assert!(response.probes.healthz);
        assert!(response.probes.readyz);
        assert!(response.probes.startup);
        assert!(response.probes.state_snapshot);
        assert!(response.browser_web.surface_api);
        assert!(response.browser_web.surface_stream);
        assert!(response.browser_web.actions_api);
        assert!(response.acp_websocket);
        assert_eq!(response.acp_websocket_path, "/api/acp");
        assert_eq!(response.browser_web.acp_websocket_path, "/api/acp");
        assert!(response.daemon_event_ingress);
        assert_eq!(response.rbac.mode, "styrene-mapped");
        assert_eq!(response.rbac.role, "admin");
        assert!(
            response
                .rbac
                .operations
                .iter()
                .any(|operation| operation.id == "native_session.action" && operation.allowed)
        );
    }

    #[tokio::test]
    async fn runtime_status_reports_control_plane_and_daemon_state() {
        let state = test_state();
        let response = get_runtime_status(axum::extract::State(state))
            .await
            .unwrap()
            .0;

        assert_eq!(response.schema_version, 1);
        assert_eq!(response.state, ControlPlaneState::Ready);
        assert!(response.ready);
        assert_eq!(response.daemon.queued_events, 0);
        assert_eq!(
            response.startup.as_ref().unwrap().auth_mode,
            "ephemeral-bearer"
        );
    }

    #[tokio::test]
    async fn providers_status_reports_harness_provider_inventory() {
        let mut state = test_state();
        let mut harness = crate::status::HarnessStatus::default();
        harness.providers.push(crate::status::ProviderStatus {
            name: "OpenAI".into(),
            authenticated: true,
            auth_method: Some("api-key".into()),
            auth_state: None,
            model: Some("gpt-5".into()),
            runtime_status: None,
            recent_failure_count: None,
            last_failure_kind: None,
            last_failure_at: None,
        });
        state.handles.harness = Some(Arc::new(Mutex::new(harness)));

        let response = get_providers_status(axum::extract::State(state))
            .await
            .unwrap()
            .0;
        assert_eq!(response.schema_version, 1);
        assert_eq!(response.providers.len(), 1);
        assert_eq!(response.providers[0].name, "OpenAI");
        assert!(response.providers[0].authenticated);
    }

    #[tokio::test]
    async fn extensions_status_reports_extension_inventory_schema() {
        let response = get_extensions_status(axum::extract::State(test_state()))
            .await
            .unwrap()
            .0;
        assert_eq!(response.schema_version, 1);
        // Inventory is host-dependent; the endpoint contract is that it returns
        // a sorted metadata vector without requiring the UI to scrape files.
        let names: Vec<_> = response
            .extensions
            .iter()
            .map(|extension| extension.name.clone())
            .collect();
        let sorted = {
            let mut sorted = names.clone();
            sorted.sort();
            sorted
        };
        assert_eq!(names, sorted);
    }

    #[tokio::test]
    async fn workspace_leases_status_reports_active_instance_leases() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(dir.path()).await;
        let lease = crate::workspace::types::WorkspaceLease {
            project_id: "project".into(),
            workspace_id: crate::workspace::runtime::workspace_id_from_path(dir.path()),
            label: "main".into(),
            path: dir.path().display().to_string(),
            backend_kind: crate::workspace::types::WorkspaceBackendKind::LocalDir,
            vcs_ref: None,
            bindings: crate::workspace::types::WorkspaceBindings::default(),
            branch: "main".into(),
            role: crate::workspace::types::WorkspaceRole::Primary,
            workspace_kind: crate::workspace::types::WorkspaceKind::Code,
            mutability: crate::workspace::types::Mutability::Mutable,
            owner_session_id: Some("session-1".into()),
            owner_agent_id: Some("omegon-test".into()),
            created_at: crate::workspace::runtime::current_timestamp(),
            last_heartbeat: crate::workspace::runtime::current_timestamp(),
            archived: false,
            archived_at: None,
            archive_reason: None,
            parent_workspace_id: None,
            source: "test".into(),
        };
        crate::workspace::runtime::write_workspace_lease(dir.path(), "test-1", &lease).unwrap();

        let response = get_workspace_leases_status().await.unwrap().0;

        assert_eq!(response.schema_version, 1);
        assert_eq!(response.leases.len(), 1);
        assert_eq!(response.leases[0].instance_id, "test-1");
        assert_eq!(
            response.leases[0].lease.owner_session_id,
            Some("session-1".to_string())
        );
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
        let response = get_web_surfaces(axum::extract::State(test_state()), auth_headers())
            .await
            .unwrap()
            .0;

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
            response.surfaces.settings.auth_mode,
            Some("ephemeral-bearer".to_string())
        );
    }

    #[tokio::test]
    async fn web_attachments_stage_and_read_browser_payload() {
        let home = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(home.path()).await;

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
    }

    #[tokio::test]
    async fn resolve_attachment_paths_returns_named_file_for_images() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let (status, created) = post_web_attachment(Json(WebAttachmentCreateRequest {
            filename: "shot.png".to_string(),
            content_type: Some("image/png".to_string()),
            data_base64: base64::engine::general_purpose::STANDARD.encode(b"\x89PNGfake"),
        }))
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);

        let paths =
            resolve_web_attachment_paths(std::slice::from_ref(&created.id)).expect("resolves");
        assert_eq!(paths.len(), 1);
        // Resolves to the original-extension file so downstream image detection
        // (which keys off the extension) recognizes it — not the .bin blob.
        assert!(paths[0].ends_with("shot.png"), "path was {}", paths[0]);
        assert!(std::path::Path::new(&paths[0]).exists());
    }

    #[tokio::test]
    async fn resolve_attachment_paths_rejects_unknown_and_traversal_ids() {
        // Unknown id → descriptive error, not a panic.
        let err = resolve_web_attachment_paths(&["att-does-not-exist".to_string()]).unwrap_err();
        assert!(err.contains("unknown or expired"), "got {err}");
        // Traversal-shaped id is refused before any filesystem touch.
        let err = resolve_web_attachment_paths(&["../../etc/passwd".to_string()]).unwrap_err();
        assert!(err.contains("invalid attachment id"), "got {err}");
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
    async fn native_session_create_denies_monitor_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Monitor;

        let response = post_native_session(
            axum::extract::State(state),
            auth_headers(),
            Json(NativeSessionCreateRequest {
                assistant_profile_id: None,
                cwd: None,
            }),
        )
        .await;

        assert!(matches!(response, Err(StatusCode::FORBIDDEN)));
    }

    #[tokio::test]
    async fn native_session_create_allows_operator_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Operator;

        let response = post_native_session(
            axum::extract::State(state),
            auth_headers(),
            Json(NativeSessionCreateRequest {
                assistant_profile_id: None,
                cwd: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0, StatusCode::CREATED);
        assert_eq!(response.1.session.session_id, "default");
    }

    #[tokio::test]
    async fn native_session_create_returns_first_party_links() {
        let response = post_native_session(
            axum::extract::State(test_state()),
            auth_headers(),
            Json(NativeSessionCreateRequest {
                assistant_profile_id: None,
                cwd: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.0, StatusCode::CREATED);
        assert_eq!(response.1.schema_version, 1);
        assert_eq!(response.1.allocation_mode, "singleton-live");
        assert_eq!(response.1.session.session_id, "default");
        assert!(response.1.assistant_profile_id.is_none());
        assert!(response.1.assistant.is_none());
        assert_eq!(
            response.1.links.surfaces,
            Some("/api/sessions/default/surfaces".to_string())
        );
        assert_eq!(
            response.1.links.actions,
            Some("/api/sessions/default/actions".to_string())
        );
        assert_eq!(
            response.1.links.stream,
            Some("/api/sessions/default/surfaces/stream".to_string())
        );
    }

    #[tokio::test]
    async fn native_session_create_validates_assistant_profile_readiness() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        write_blocked_agent(home.path());
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = post_native_session(
            axum::extract::State(test_state()),
            auth_headers(),
            Json(NativeSessionCreateRequest {
                assistant_profile_id: Some("blocked-agent".to_string()),
                cwd: None,
            }),
        )
        .await
        .unwrap();

        restore_env("OMEGON_HOME", previous_home);

        assert_eq!(response.0, StatusCode::CREATED);
        assert_eq!(
            response.1.assistant_profile_id,
            Some("blocked-agent".to_string())
        );
        let assistant = response.1.assistant.as_ref().expect("assistant readiness");
        assert_eq!(assistant.id, "blocked-agent");
        assert_eq!(
            assistant.launch_readiness.status,
            crate::capabilities::profiles::AssistantLaunchStatus::Blocked
        );
        assert_eq!(assistant.blocker_count, 1);
    }

    #[tokio::test]
    async fn native_session_create_404s_unknown_assistant_profile() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = post_native_session(
            axum::extract::State(test_state()),
            auth_headers(),
            Json(NativeSessionCreateRequest {
                assistant_profile_id: Some("missing".to_string()),
                cwd: None,
            }),
        )
        .await;

        restore_env("OMEGON_HOME", previous_home);

        assert!(matches!(response, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn native_session_read_denies_blocked_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Blocked;

        let response = get_native_session(
            axum::extract::State(state),
            auth_headers(),
            axum::extract::Path("default".to_string()),
        )
        .await;

        assert!(matches!(response, Err(StatusCode::FORBIDDEN)));
    }

    #[tokio::test]
    async fn native_session_surfaces_deny_blocked_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Blocked;

        let response = get_native_session_surfaces(
            axum::extract::State(state),
            auth_headers(),
            axum::extract::Path("default".to_string()),
        )
        .await;

        assert!(matches!(response, Err(StatusCode::FORBIDDEN)));
    }

    #[tokio::test]
    async fn web_surfaces_deny_blocked_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Blocked;

        let response = get_web_surfaces(axum::extract::State(state), auth_headers()).await;

        assert!(matches!(response, Err(StatusCode::FORBIDDEN)));
    }

    #[tokio::test]
    async fn native_session_surfaces_reject_unknown_session() {
        let response = get_native_session_surfaces(
            axum::extract::State(test_state()),
            auth_headers(),
            axum::extract::Path("missing".to_string()),
        )
        .await;
        assert!(matches!(response, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn native_session_action_denies_monitor_role_at_endpoint() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Monitor;
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        state.command_tx = tx;

        let (status, response) = post_native_session_action(
            axum::extract::State(state),
            auth_headers(),
            axum::extract::Path("default".to_string()),
            Json(WebActionRequest {
                schema_version: 1,
                action_id: "rbac-denied".to_string(),
                client_id: "auspex".to_string(),
                session_id: "default".to_string(),
                action: WebActionPayload::SubmitPrompt {
                    text: "must not run".to_string(),
                    attachments: Vec::new(),
                },
            }),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            response.0.status,
            crate::ui_runtime::envelope::UiActionOutcomeStatus::Rejected
        );
        assert_eq!(response.0.error, Some("capability_not_granted".to_string()));
        assert!(
            rx.try_recv().is_err(),
            "denied action must not reach command queue"
        );
    }

    #[tokio::test]
    async fn native_session_action_rbac_denial_has_stable_failure_contract() {
        let error = super::super::rbac::require_operation(
            styrene_rbac::Role::Monitor,
            omegon_rbac::OmegonOperation::NativeSessionAction,
            &super::super::rbac::RbacContext {
                route: "/api/sessions/{session_id}/actions",
                session_id: Some("default"),
                action_id: Some("a-denied"),
                client_id: Some("auspex"),
                ..super::super::rbac::RbacContext::default()
            },
        )
        .unwrap_err();

        assert_eq!(error.status(), StatusCode::FORBIDDEN);
        let response = error.response();
        assert_eq!(response.error, "forbidden");
        assert_eq!(response.reason, "capability_not_granted");
        assert_eq!(response.operation, Some("native_session.action"));
        assert_eq!(
            response.capability,
            Some(omegon_rbac::OmegonCapability::SESSION_ACTION)
        );
    }

    #[tokio::test]
    async fn native_session_action_reuses_web_action_ingress() {
        let mut state = test_state();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        state.command_tx = tx;

        let (status, response) = post_native_session_action(
            axum::extract::State(state),
            auth_headers(),
            axum::extract::Path("default".to_string()),
            Json(WebActionRequest {
                schema_version: 1,
                action_id: "native-a1".to_string(),
                client_id: "auspex".to_string(),
                session_id: "ignored-client-value".to_string(),
                action: WebActionPayload::SubmitPrompt {
                    text: "hello native session".to_string(),
                    attachments: Vec::new(),
                },
            }),
        )
        .await;

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(response.session_id, "default");
        match rx.recv().await.unwrap() {
            super::super::WebCommand::UserPrompt { text, .. } => {
                assert_eq!(text, "hello native session");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn web_sessions_endpoint_lists_default_session() {
        let home = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(home.path()).await;

        let response = get_web_sessions().await.unwrap().0;

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
        assert_eq!(response.schema_version, 1);
        assert_eq!(
            response.links.surfaces,
            Some("/api/sessions/default/surfaces".to_string())
        );
        assert_eq!(
            response.links.actions,
            Some("/api/sessions/default/actions".to_string())
        );
        assert_eq!(
            response.links.stream,
            Some("/api/sessions/default/surfaces/stream".to_string())
        );
        assert_eq!(response.snapshot.session_id, "default");
    }

    #[tokio::test]
    async fn web_session_endpoint_404s_missing_session() {
        let home = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(home.path()).await;

        let response = get_web_session(
            axum::extract::State(test_state()),
            axum::extract::Path("missing".to_string()),
        )
        .await;

        assert!(matches!(response, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn web_action_denies_monitor_role_before_queueing_command() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Monitor;
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        state.command_tx = tx;

        let (status, response) = post_web_action(
            axum::extract::State(state),
            auth_headers(),
            Json(WebActionRequest {
                schema_version: 1,
                action_id: "web-denied".to_string(),
                client_id: "auspex".to_string(),
                session_id: "default".to_string(),
                action: WebActionPayload::SubmitPrompt {
                    text: "should not queue".to_string(),
                    attachments: Vec::new(),
                },
            }),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(response.0.error, Some("capability_not_granted".to_string()));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn web_action_submit_prompt_queues_web_command() {
        let mut state = test_state();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        state.command_tx = tx;

        let (status, response) = post_web_action(
            axum::extract::State(state),
            auth_headers(),
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
            super::super::WebCommand::UserPrompt { text, .. } => assert_eq!(text, "hello web"),
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
            auth_headers(),
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
    async fn web_action_permission_can_answer_always_allow() {
        let state = test_state();
        let (tx, rx) = std::sync::mpsc::channel();
        let respond = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
        let request_id = state.register_permission(&respond);

        let (status, response) = post_web_action(
            axum::extract::State(state),
            auth_headers(),
            Json(WebActionRequest {
                schema_version: 1,
                action_id: "perm-always".to_string(),
                client_id: "browser-tab".to_string(),
                session_id: "default".to_string(),
                action: WebActionPayload::RespondPermission {
                    request_id,
                    allow: true,
                    always: true,
                },
            }),
        )
        .await;

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(
            response.status,
            crate::ui_runtime::envelope::UiActionOutcomeStatus::Accepted
        );
        assert_eq!(
            response.message.as_deref(),
            Some("permission always allowed")
        );
        assert_eq!(
            rx.recv().expect("permission response"),
            omegon_traits::PermissionResponse::AlwaysAllow
        );
    }

    #[tokio::test]
    async fn web_action_rejects_unknown_session() {
        let (status, response) = post_web_action(
            axum::extract::State(test_state()),
            auth_headers(),
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
        assert_eq!(response.error, Some("unknown session_id".to_string()));
    }

    #[tokio::test]
    async fn assistant_runs_endpoint_returns_empty_runtime_projection() {
        let home = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(home.path()).await;
        let response = get_assistant_runs(axum::extract::State(test_state()))
            .await
            .unwrap()
            .0;
        assert!(response.runs.is_empty());
    }

    #[tokio::test]
    async fn assistant_run_endpoint_404s_missing_runtime_run() {
        let home = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(home.path()).await;
        let err = get_assistant_run(
            axum::extract::State(test_state()),
            axum::extract::Path("missing".into()),
        )
        .await
        .unwrap_err();
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
    async fn assistant_profile_detail_endpoint_returns_full_profile_and_readiness() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        write_blocked_agent(home.path());
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = get_assistant_profile(
            axum::extract::State(test_state()),
            axum::extract::Path("blocked-agent".to_string()),
        )
        .await
        .unwrap()
        .0;

        restore_env("OMEGON_HOME", previous_home);

        assert_eq!(response.schema_version, 1);
        assert_eq!(response.profile.id, "blocked-agent");
        assert_eq!(
            response.profile.required_secrets,
            vec!["MISSING_REQUIRED_TOKEN"]
        );
        assert_eq!(response.assistant.id, "blocked-agent");
        assert_eq!(
            response.assistant.launch_readiness.status,
            crate::capabilities::profiles::AssistantLaunchStatus::Blocked
        );
        assert_eq!(
            response.readiness_href,
            "/api/capabilities/assistants/blocked-agent/readiness"
        );
    }

    #[tokio::test]
    async fn assistant_profile_detail_endpoint_404s_missing_profile() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let response = get_assistant_profile(
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
            crate::capabilities::secrets::SecretReadinessStatus::Missing
        );
        assert_eq!(readiness.recipe_kind, Some("env".to_string()));
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
        let (status, payload) = event_accepted_response(
            post_event(axum::extract::State(test_state()), headers, Json(event)).await,
        );
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!payload.accepted);
    }

    #[tokio::test]
    async fn post_event_rejects_missing_caller_role() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let state = test_state();
        let event = DaemonEventEnvelope {
            event_id: "evt-missing-role".into(),
            source: "manual/test".into(),
            trigger_kind: "manual".into(),
            payload: serde_json::json!({"ok": true}),
            caller_role: None,
            source_user: None,
            source_channel: None,
            source_thread: None,
        };

        let (status, payload) = event_rbac_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload.error, "invalid_role");
        assert_eq!(payload.reason, "missing_role");
        assert!(state.daemon_events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn post_event_rejects_self_asserted_admin_role() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Monitor;
        let event = DaemonEventEnvelope {
            event_id: "evt-self-assert-admin".into(),
            source: "manual/test".into(),
            trigger_kind: "manual".into(),
            payload: serde_json::json!({}),
            caller_role: Some("admin".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };

        let (status, payload) = event_rbac_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(payload.error, "forbidden");
        assert_eq!(payload.reason, "capability_not_granted");
        assert_eq!(payload.operation, Some("event.ingress"));
        assert_eq!(payload.role, Some("monitor"));
        assert!(state.daemon_events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn post_event_accepts_trusted_proxy_principal() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        headers.insert(
            "omegon-principal-issuer",
            axum::http::HeaderValue::from_static("auspex"),
        );
        headers.insert(
            "omegon-principal-subject",
            axum::http::HeaderValue::from_static("user:alice"),
        );
        headers.insert(
            "omegon-principal-role",
            axum::http::HeaderValue::from_static("admin"),
        );
        let state = test_state();
        let event = DaemonEventEnvelope {
            event_id: "evt-proxy".into(),
            source: "auspex/test".into(),
            trigger_kind: "manual".into(),
            payload: serde_json::json!({"ok": true}),
            caller_role: Some("admin".into()),
            source_user: Some("user:alice".into()),
            source_channel: None,
            source_thread: None,
        };

        let (status, payload) = event_accepted_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );

        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(payload.accepted);
        assert_eq!(state.daemon_events.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn post_event_rejects_monitor_role_for_event_ingress() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let state = test_state();
        let event = DaemonEventEnvelope {
            event_id: "evt-monitor-denied".into(),
            source: "manual/test".into(),
            trigger_kind: "new_session".into(),
            payload: serde_json::json!({}),
            caller_role: Some("read".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };

        let (status, payload) = event_rbac_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(payload.error, "forbidden");
        assert_eq!(payload.reason, "capability_not_granted");
        assert_eq!(payload.operation, Some("event.ingress"));
        assert_eq!(
            payload.capability,
            Some(omegon_rbac::OmegonCapability::EVENT_INGRESS)
        );
        assert!(state.daemon_events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn post_event_rejects_invalid_caller_role() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        let event = DaemonEventEnvelope {
            event_id: "evt-invalid-role".into(),
            source: "manual/test".into(),
            trigger_kind: "new_session".into(),
            payload: serde_json::json!({}),
            caller_role: Some("superuser".into()),
            source_user: None,
            source_channel: None,
            source_thread: None,
        };

        let (status, payload) = event_rbac_response(
            post_event(axum::extract::State(test_state()), headers, Json(event)).await,
        );

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload.error, "invalid_role");
        assert_eq!(payload.reason, "unknown_role");
    }

    #[tokio::test]
    async fn get_events_reports_queued_daemon_event_snapshot() {
        let state = test_state();
        state
            .daemon_events
            .lock()
            .unwrap()
            .push(DaemonEventEnvelope {
                event_id: "evt-queued".into(),
                source: "manual/test".into(),
                trigger_kind: "new_session".into(),
                payload: serde_json::json!({"reason":"test"}),
                caller_role: Some("write".into()),
                source_user: None,
                source_channel: None,
                source_thread: None,
            });
        state.daemon_status.lock().unwrap().processed_events = 2;

        let response = get_events(axum::extract::State(state)).await.unwrap().0;
        assert_eq!(response.schema_version, 1);
        assert_eq!(response.queued_events, 1);
        assert_eq!(response.processed_events, 2);
        assert_eq!(response.events[0].event_id, "evt-queued");
    }

    #[test]
    fn daemon_event_stream_maps_runtime_and_provider_events() {
        let runtime = daemon_event_stream_envelope(&omegon_traits::AgentEvent::ContextUpdated {
            tokens: 512,
            context_window: 4096,
            context_class: "standard".into(),
            thinking_level: "medium".into(),
        })
        .expect("context event maps");
        assert_eq!(runtime.event_type, "runtime.context_changed");
        assert_eq!(runtime.payload["tokens"], 512);

        let provider = daemon_event_stream_envelope(&omegon_traits::AgentEvent::RouteChanged {
            state: "ready".into(),
            selected: Some("anthropic/claude".into()),
            serving: Some("anthropic".into()),
            warning: None,
            message: "provider ready".into(),
        })
        .expect("provider event maps");
        assert_eq!(provider.event_type, "provider.status_changed");
        assert_eq!(provider.payload["selected"], "anthropic/claude");
    }

    #[test]
    fn daemon_event_stream_ignores_conversation_chunks() {
        assert!(
            daemon_event_stream_envelope(&omegon_traits::AgentEvent::MessageChunk {
                text: "not daemon state".into(),
            })
            .is_none()
        );
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
        let (status, payload) = event_accepted_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );
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
        let (status, payload) = event_accepted_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );
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
        let (status, payload) = event_accepted_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );
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
        let (status, payload) = event_accepted_response(
            post_event(axum::extract::State(state.clone()), headers, Json(event)).await,
        );
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
            route_decision: None,
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
                    inventory_generation: None,
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
            runtime.model,
            Some("anthropic:claude-sonnet-4-6".to_string())
        );
        assert_eq!(runtime.context_class, Some("massive".to_string()));
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

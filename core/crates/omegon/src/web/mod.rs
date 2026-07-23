//! Embedded web server — localhost HTTP + WebSocket control-plane.
//!
//! Serves:
//! - `GET /` — embedded single-page dashboard
//! - `GET /api/startup` — machine-readable startup/discovery metadata
//! - `GET /api/healthz` — liveness probe for local supervisors
//! - `GET /api/readyz` — readiness probe for local supervisors
//! - `GET /api/state` — full agent state snapshot (JSON)
//! - `WS /ws` — bidirectional agent protocol (JSON-over-WebSocket)
//!
//! The WebSocket protocol is the **full agent interface** — any web UI can
//! connect and drive the agent as a black box.

pub mod acp_ws;
pub mod api;
pub mod auth;
pub mod rbac;
pub mod surface_stream;
pub mod surfaces;
pub mod ws;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use tokio::sync::{broadcast, mpsc};

use crate::tui::dashboard::DashboardHandles;
pub use auth::{WEB_AUTH_SECRET_NAME, WebAuthSource, WebAuthState};
use omegon_traits::{
    DaemonEventEnvelope, IpcHarnessSnapshot, IpcHealthSnapshot, IpcHealthState, IpcMemorySnapshot,
    OmegonTransportSecurity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlPlaneState {
    Starting,
    Ready,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DaemonChildRuntimeStatus {
    pub label: String,
    pub status: String,
    pub supervision_mode: Option<String>,
    pub pid: Option<u32>,
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

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WebLoopSchedulerStatus {
    pub configured_jobs: usize,
    pub enabled_jobs: usize,
    pub disabled_jobs: usize,
    pub last_outcome: Option<String>,
    pub next_due_at: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WebDaemonStatus {
    pub queued_events: usize,
    pub processed_events: usize,
    pub worker_running: bool,
    pub transport_warnings: Vec<String>,
    pub active_child_runtimes: Vec<DaemonChildRuntimeStatus>,
    pub loop_scheduler: WebLoopSchedulerStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebTrustedProxyIdentity {
    pub schema_version: u8,
    pub subject: String,
    pub fingerprint: String,
    #[serde(default)]
    pub strict_daemon_identity: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WebAuthorityConfig {
    pub trusted_proxy: Option<WebTrustedProxyIdentity>,
    pub require_proxy_identity: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebAuthorityStatus {
    pub mode: String,
    pub trusted_proxy_configured: bool,
    pub trusted_proxy_subject: Option<String>,
    pub trusted_proxy_fingerprint: Option<String>,
    pub strict_proxy_identity: bool,
}

impl WebAuthorityConfig {
    pub fn status(&self) -> WebAuthorityStatus {
        let configured = self.trusted_proxy.as_ref();
        WebAuthorityStatus {
            mode: if self.require_proxy_identity {
                "trusted_proxy_strict".to_string()
            } else if configured.is_some() {
                "trusted_proxy".to_string()
            } else {
                "bearer".to_string()
            },
            trusted_proxy_configured: configured.is_some(),
            trusted_proxy_subject: configured.map(|identity| identity.subject.clone()),
            trusted_proxy_fingerprint: configured.map(|identity| identity.fingerprint.clone()),
            strict_proxy_identity: self.require_proxy_identity,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebStartupInfo {
    pub schema_version: u32,
    pub addr: String,
    pub http_base: String,
    pub state_url: String,
    pub startup_url: String,
    pub health_url: String,
    pub ready_url: String,
    pub ws_url: String,
    pub acp_url: Option<String>,
    pub token: String,
    pub auth_mode: String,
    pub auth_source: String,
    pub web_authority: WebAuthorityStatus,
    pub control_plane_state: ControlPlaneState,
    pub daemon_status: WebDaemonStatus,
    pub instance_descriptor: Option<omegon_traits::OmegonInstanceDescriptor>,
}

fn project_web_instance(
    handles: &DashboardHandles,
    startup: &WebStartupInfo,
) -> omegon_traits::OmegonInstanceDescriptor {
    let harness_projection = handles
        .harness
        .as_ref()
        .and_then(|lock| lock.lock().ok())
        .map(|h| IpcHarnessSnapshot {
            context_class: h.context_class.clone(),
            thinking_level: h.thinking_level.clone(),
            capability_tier: h.capability_grade.clone(),
            runtime_profile: h.runtime_profile.as_str().to_string(),
            autonomy_mode: match h.autonomy_mode {
                omegon_traits::OmegonAutonomyMode::OperatorDriven => "operator-driven".into(),
                omegon_traits::OmegonAutonomyMode::GuardedAutonomous => "guarded-autonomous".into(),
                omegon_traits::OmegonAutonomyMode::Autonomous => "autonomous".into(),
            },
            dispatcher: omegon_traits::IpcDispatcherSnapshot {
                available_options: h.dispatcher.available_options.clone(),
                switch_state: h.dispatcher.switch_state.clone(),
                request_id: h.dispatcher.request_id.clone(),
                expected_profile: h.dispatcher.expected_profile.clone(),
                expected_model: h.dispatcher.expected_model.clone(),
                active_profile: h.dispatcher.active_profile.clone(),
                active_model: h.dispatcher.active_model.clone(),
                failure_code: h.dispatcher.failure_code.clone(),
                note: h.dispatcher.note.clone(),
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
            providers: vec![],
            mcp_server_count: h.mcp_servers.iter().filter(|s| s.connected).count(),
            mcp_tool_count: h.mcp_tool_count(),
            active_persona: h.active_persona.as_ref().map(|p| p.name.clone()),
            active_tone: h.active_tone.as_ref().map(|t| t.name.clone()),
            active_delegate_count: h.active_delegates.len(),
            execution_substrate: Some(h.execution_substrate.clone()),
        })
        .unwrap_or(IpcHarnessSnapshot {
            context_class: "Compact".into(),
            thinking_level: "Medium".into(),
            capability_tier: "B".into(),
            runtime_profile: "primary-interactive".into(),
            autonomy_mode: "operator-driven".into(),
            dispatcher: omegon_traits::IpcDispatcherSnapshot {
                available_options: vec![
                    "F".into(),
                    "D".into(),
                    "C".into(),
                    "B".into(),
                    "A".into(),
                    "S".into(),
                ],
                switch_state: "idle".into(),
                request_id: None,
                expected_profile: None,
                expected_model: None,
                active_profile: Some("B".into()),
                active_model: None,
                failure_code: None,
                note: None,
            },
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
            execution_substrate: Some(crate::execution_substrate::detect()),
        });

    let (git_branch, git_detached) = handles
        .harness
        .as_ref()
        .and_then(|lock| lock.lock().ok())
        .map(|h| (h.git_branch.clone(), h.git_detached))
        .unwrap_or((None, false));

    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    let health = IpcHealthSnapshot {
        state: match startup.control_plane_state {
            ControlPlaneState::Ready => IpcHealthState::Ready,
            ControlPlaneState::Degraded => IpcHealthState::Degraded,
            ControlPlaneState::Starting => IpcHealthState::Starting,
            ControlPlaneState::Failed => IpcHealthState::Failed,
        },
        memory_ok: harness_projection.memory_available
            || harness_projection.memory_warning.is_none(),
        provider_ok: handles
            .harness
            .as_ref()
            .and_then(|lock| lock.lock().ok())
            .is_some_and(|h| h.providers.iter().any(|p| p.authenticated)),
        checked_at: chrono::Utc::now().to_rfc3339(),
    };

    let session = omegon_traits::IpcSessionSnapshot {
        cwd: cwd.clone(),
        pid: std::process::id(),
        started_at: chrono::Utc::now().to_rfc3339(),
        turns: 0,
        tool_calls: 0,
        compactions: 0,
        busy: handles.session.lock().map(|s| s.busy).unwrap_or(false),
        git_branch,
        git_detached,
        session_id: None,
    };

    let mut instance = crate::ipc::snapshot::project_instance_descriptor(
        handles,
        &cwd,
        &session,
        &harness_projection,
        &health,
        env!("CARGO_PKG_VERSION"),
        startup
            .instance_descriptor
            .as_ref()
            .map(|instance| instance.identity.instance_id.as_str())
            .unwrap_or("web-compat"),
    );
    instance.control_plane.http_base = Some(startup.http_base.clone());
    instance.control_plane.startup_url = Some(startup.startup_url.clone());
    instance.control_plane.state_url = Some(startup.state_url.clone());
    instance.control_plane.ws_url = Some(startup.ws_url.clone());
    instance.control_plane.auth_mode = Some(startup.auth_mode.clone());
    instance.control_plane.auth_source = Some(startup.auth_source.clone());
    let transport_security =
        if startup.http_base.starts_with("https://") && startup.ws_url.starts_with("wss://") {
            OmegonTransportSecurity::Secure
        } else {
            OmegonTransportSecurity::InsecureBootstrap
        };
    instance.control_plane.http_transport_security = Some(transport_security.clone());
    instance.control_plane.ws_transport_security = Some(transport_security);
    instance
}

/// Shared state accessible to all web handlers.
#[derive(Clone)]
pub struct WebState {
    /// Dashboard data handles (same Arc<Mutex<>> the TUI reads).
    pub handles: DashboardHandles,
    /// Broadcast channel for AgentEvents → WebSocket push.
    pub events_tx: broadcast::Sender<omegon_traits::AgentEvent>,
    /// Channel for WebSocket commands → main loop.
    pub command_tx: mpsc::Sender<WebCommand>,
    /// Web auth state for dashboard and WebSocket attachment.
    pub web_auth: Arc<WebAuthState>,
    /// Machine-readable startup/discovery payload once the server is bound.
    pub startup_info: Arc<Mutex<Option<WebStartupInfo>>>,
    /// Control-plane lifecycle state for machine health/readiness probes.
    pub control_plane_state: Arc<Mutex<ControlPlaneState>>,
    /// Shared secrets manager for metadata-only readiness projections.
    pub secrets: Option<Arc<omegon_secrets::SecretsManager>>,
    /// Project-local assistant run ledger path.
    pub assistant_runs_db_path: Arc<std::path::PathBuf>,
    /// Received daemon/event-ingress envelopes (v1 in-memory queue).
    pub daemon_events: Arc<Mutex<Vec<DaemonEventEnvelope>>>,
    /// Shared queue/worker status for daemon event ingress.
    pub daemon_status: Arc<Mutex<WebDaemonStatus>>,
    /// Permission responders captured from broadcast `PermissionRequest`
    /// events, keyed by a stable `Arc`-identity id, so the web client can
    /// answer tool-approval prompts via `POST /api/web/actions`. Populated by
    /// the surface stream as it forwards the event to the browser.
    pub pending_permissions: Arc<
        Mutex<
            std::collections::HashMap<
                String,
                std::sync::mpsc::Sender<omegon_traits::PermissionResponse>,
            >,
        >,
    >,
    /// Operator-wait responders captured from broadcast `OperatorWaitRequest`
    /// events, keyed by a stable `Arc`-identity id. The surface stream sends
    /// `acknowledge` immediately on capture (the producer abandons the wait if
    /// no surface acknowledges within ~2s), then the browser delivers the
    /// Completed/Cancelled decision via `POST /api/web/actions`.
    pub pending_operator_waits: Arc<
        Mutex<
            std::collections::HashMap<
                String,
                std::sync::mpsc::Sender<omegon_traits::OperatorWaitResponse>,
            >,
        >,
    >,
    /// Rolling conversation transcript accumulated by a single-writer task
    /// subscribed to the agent event bus, plus user prompts recorded at
    /// submission. Served by `GET /api/web/surfaces` so a browser reload
    /// replays prior turns instead of starting blank. Bounded to the most
    /// recent [`CONVERSATION_LOG_CAP`] segments.
    pub conversation_log: Arc<Mutex<std::collections::VecDeque<surfaces::WebConversationSegment>>>,
    /// Latest renderer-neutral plan projection observed from the agent event
    /// bus. Served by `GET /api/web/surfaces` so the browser's Plan rail can
    /// survive reloads instead of relying only on live `plan_updated` pushes.
    pub plan_surface: Arc<Mutex<omegon_traits::PlanSurfaceProjection>>,
    /// Recent tool runs accumulated from AgentEvent::ToolStart/Update/End so
    /// the browser Instruments rail can recover active/recent tool state after reload.
    pub tool_runs: Arc<Mutex<std::collections::VecDeque<surfaces::WebToolRunSurface>>>,
    /// Effective Styrene role for browser/native web requests. Defaults to Admin
    /// for local ephemeral bearer mode until profile settings thread a stricter role.
    pub web_role: styrene_rbac::Role,
    /// Optional local authority proxy identity contract for Auspex-mediated web access.
    pub web_authority: WebAuthorityConfig,
}

/// Maximum conversation segments retained for reload replay. Older segments are
/// evicted; the live stream remains the source of truth for the active turn.
pub(crate) const CONVERSATION_LOG_CAP: usize = 400;
pub(crate) const TOOL_RUN_LOG_CAP: usize = 100;

impl WebState {
    /// Create a new WebState. Generates a random auth token.
    pub fn new(
        handles: DashboardHandles,
        events_tx: broadcast::Sender<omegon_traits::AgentEvent>,
    ) -> Self {
        Self::with_auth_state(
            handles,
            events_tx,
            WebAuthState::ephemeral_generated(generate_token()),
        )
    }

    pub fn with_auth_state(
        handles: DashboardHandles,
        events_tx: broadcast::Sender<omegon_traits::AgentEvent>,
        auth_state: WebAuthState,
    ) -> Self {
        Self::with_auth_state_and_secrets(handles, events_tx, auth_state, None)
    }

    pub fn with_web_role(mut self, web_role: styrene_rbac::Role) -> Self {
        self.web_role = web_role;
        self
    }

    pub fn with_web_authority(mut self, web_authority: WebAuthorityConfig) -> Self {
        self.web_authority = web_authority;
        self
    }

    pub fn with_auth_state_and_secrets(
        handles: DashboardHandles,
        events_tx: broadcast::Sender<omegon_traits::AgentEvent>,
        auth_state: WebAuthState,
        secrets: Option<Arc<omegon_secrets::SecretsManager>>,
    ) -> Self {
        let (command_tx, _) = mpsc::channel(32); // receiver returned by start_server
        Self {
            handles,
            events_tx,
            command_tx,
            web_auth: Arc::new(auth_state),
            startup_info: Arc::new(Mutex::new(None)),
            control_plane_state: Arc::new(Mutex::new(ControlPlaneState::Starting)),
            secrets,
            assistant_runs_db_path: Arc::new(crate::paths::assistant_runs_db(
                &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )),
            daemon_events: Arc::new(Mutex::new(Vec::new())),
            daemon_status: Arc::new(Mutex::new(WebDaemonStatus {
                transport_warnings: default_transport_warnings(),
                ..WebDaemonStatus::default()
            })),
            pending_permissions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            pending_operator_waits: Arc::new(Mutex::new(std::collections::HashMap::new())),
            conversation_log: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            plan_surface: Arc::new(Mutex::new(omegon_traits::PlanSurfaceProjection::default())),
            tool_runs: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            web_role: styrene_rbac::Role::Admin,
            web_authority: WebAuthorityConfig::default(),
        }
    }
}

/// Derive a stable, coordination-free id for a permission responder from its
/// `Arc` identity. Every clone of the same broadcast `PermissionRequest` event
/// shares the `Arc`, so the surface stream (which emits this id to the browser)
/// and the later action lookup agree without a shared counter.
pub(crate) fn permission_request_id(
    respond: &Arc<Mutex<Option<std::sync::mpsc::Sender<omegon_traits::PermissionResponse>>>>,
) -> String {
    format!("perm-{:x}", Arc::as_ptr(respond) as usize)
}

impl WebState {
    /// Capture a permission responder so the web client can answer it later,
    /// returning its stable id. Takes the sender out of the shared event slot
    /// (first consumer wins — in a daemon-hosted web deployment there is no TUI
    /// contending for it). Idempotent: if the sender was already taken, the id
    /// is still returned so the browser can reference the in-flight request.
    pub(crate) fn register_permission(
        &self,
        respond: &Arc<Mutex<Option<std::sync::mpsc::Sender<omegon_traits::PermissionResponse>>>>,
    ) -> String {
        let id = permission_request_id(respond);
        if let Some(sender) = respond.lock().ok().and_then(|mut slot| slot.take())
            && let Ok(mut map) = self.pending_permissions.lock()
        {
            map.insert(id.clone(), sender);
        }
        id
    }

    /// Resolve a captured permission responder by id, sending the decision.
    /// Returns `Err` with a reason if the id is unknown/already answered or the
    /// agent is no longer waiting.
    pub(crate) fn answer_permission(
        &self,
        request_id: &str,
        decision: omegon_traits::PermissionResponse,
    ) -> Result<(), &'static str> {
        let sender = self
            .pending_permissions
            .lock()
            .ok()
            .and_then(|mut map| map.remove(request_id))
            .ok_or("unknown or already-answered permission request")?;
        sender
            .send(decision)
            .map_err(|_| "permission request is no longer awaiting a response")
    }

    /// Capture an operator-wait responder so the web client can answer it, and
    /// send `acknowledge` immediately. The producer abandons the wait if no
    /// surface acknowledges within ~2s of emitting the event, so acknowledgement
    /// must happen synchronously on capture — well before the browser round-trip
    /// that delivers the eventual Completed/Cancelled decision. Returns the id.
    pub(crate) fn register_operator_wait(
        &self,
        acknowledge: &Arc<Mutex<Option<std::sync::mpsc::Sender<()>>>>,
        respond: &Arc<Mutex<Option<std::sync::mpsc::Sender<omegon_traits::OperatorWaitResponse>>>>,
    ) -> String {
        let id = format!("wait-{:x}", Arc::as_ptr(respond) as usize);
        // Acknowledge first: a present web surface is handling this wait.
        if let Some(ack) = acknowledge.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = ack.send(());
        }
        if let Some(sender) = respond.lock().ok().and_then(|mut slot| slot.take())
            && let Ok(mut map) = self.pending_operator_waits.lock()
        {
            map.insert(id.clone(), sender);
        }
        id
    }

    /// Resolve a captured operator-wait responder by id with the operator's
    /// decision. Returns `Err` if the id is unknown/already answered or the
    /// agent is no longer waiting.
    pub(crate) fn answer_operator_wait(
        &self,
        request_id: &str,
        completed: bool,
    ) -> Result<(), &'static str> {
        let decision = if completed {
            omegon_traits::OperatorWaitResponse::Completed
        } else {
            omegon_traits::OperatorWaitResponse::Cancelled
        };
        let sender = self
            .pending_operator_waits
            .lock()
            .ok()
            .and_then(|mut map| map.remove(request_id))
            .ok_or("unknown or already-answered operator-wait request")?;
        sender
            .send(decision)
            .map_err(|_| "operator-wait request is no longer awaiting a response")
    }

    /// Append a completed user-prompt segment to the transcript. Called at
    /// submission time (user prompts are not re-broadcast on the agent bus),
    /// preserving correct ordering: the user turn lands before the agent's
    /// `TurnStart` opens the assistant reply.
    pub(crate) fn record_user_segment(&self, text: &str) {
        if let Ok(mut log) = self.conversation_log.lock() {
            let index = log.len();
            log.push_back(surfaces::WebConversationSegment {
                index,
                role: "user".to_string(),
                title: None,
                summary: None,
                body: Some(text.to_string()),
                complete: true,
                copyable: true,
                selectable: true,
            });
            while log.len() > CONVERSATION_LOG_CAP {
                log.pop_front();
            }
        }
    }

    /// Fold one agent event into the rolling transcript. Single-writer: only the
    /// accumulator task started by [`start_conversation_accumulator`] calls this.
    /// `TurnStart` opens an assistant segment, `MessageChunk` appends to it
    /// (opening one if needed), `TurnEnd` marks it complete. Non-conversation
    /// events are ignored.
    pub(crate) fn fold_conversation_event(&self, event: &omegon_traits::AgentEvent) {
        use omegon_traits::AgentEvent;
        if let AgentEvent::PlanUpdated { projection } = event
            && let Ok(mut plan) = self.plan_surface.lock()
        {
            *plan = projection.clone();
        }
        self.fold_tool_event(event);
        let Ok(mut log) = self.conversation_log.lock() else {
            return;
        };
        let open_assistant = |log: &std::collections::VecDeque<
            surfaces::WebConversationSegment,
        >| {
            matches!(log.back(), Some(seg) if seg.role == "assistant" && !seg.complete)
        };
        match event {
            AgentEvent::TurnStart { .. } => {
                let index = log.len();
                log.push_back(surfaces::WebConversationSegment {
                    index,
                    role: "assistant".to_string(),
                    title: None,
                    summary: None,
                    body: Some(String::new()),
                    complete: false,
                    copyable: true,
                    selectable: true,
                });
            }
            AgentEvent::MessageChunk { text } => {
                if !open_assistant(&log) {
                    let index = log.len();
                    log.push_back(surfaces::WebConversationSegment {
                        index,
                        role: "assistant".to_string(),
                        title: None,
                        summary: None,
                        body: Some(String::new()),
                        complete: false,
                        copyable: true,
                        selectable: true,
                    });
                }
                if let Some(seg) = log.back_mut() {
                    seg.body.get_or_insert_with(String::new).push_str(text);
                }
            }
            AgentEvent::TurnEnd { .. } => {
                if let Some(seg) = log.back_mut()
                    && seg.role == "assistant"
                    && !seg.complete
                {
                    seg.complete = true;
                    // Drop an assistant turn that produced no visible text
                    // (e.g. a tool-only turn) so reload replay stays clean.
                    if seg.body.as_deref().unwrap_or("").is_empty() {
                        log.pop_back();
                    }
                }
            }
            _ => {}
        }
        while log.len() > CONVERSATION_LOG_CAP {
            log.pop_front();
        }
    }

    pub(crate) fn redact_web_value(&self, value: &serde_json::Value) -> serde_json::Value {
        let Some(secrets) = &self.secrets else {
            return value.clone();
        };
        let serialized = value.to_string();
        let redacted = secrets.redact(&serialized);
        serde_json::from_str(&redacted).unwrap_or(serde_json::Value::String(redacted))
    }

    pub(crate) fn redact_web_text(&self, text: &str) -> String {
        self.secrets
            .as_ref()
            .map(|secrets| secrets.redact(text))
            .unwrap_or_else(|| text.to_string())
    }

    fn fold_tool_event(&self, event: &omegon_traits::AgentEvent) {
        use omegon_traits::{AgentEvent, ContentBlock};
        let Ok(mut tools) = self.tool_runs.lock() else {
            return;
        };
        match event {
            AgentEvent::ToolStart {
                id,
                name,
                args,
                provenance,
            } => {
                let redacted_args = self.redact_web_value(args);
                if let Some(existing) = tools.iter_mut().find(|tool| tool.id == *id) {
                    existing.name = name.clone();
                    existing.provenance = provenance.clone();
                    existing.status = "running".to_string();
                    existing.args = redacted_args;
                    existing.output_tail = None;
                    existing.result_summary = None;
                    existing.is_error = false;
                    existing.elapsed_ms = None;
                    existing.phase = None;
                } else {
                    tools.push_back(surfaces::WebToolRunSurface {
                        id: id.clone(),
                        name: name.clone(),
                        provenance: provenance.clone(),
                        status: "running".to_string(),
                        args: redacted_args,
                        output_tail: None,
                        result_summary: None,
                        is_error: false,
                        elapsed_ms: None,
                        phase: None,
                    });
                }
            }
            AgentEvent::ToolUpdate { id, partial } => {
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.id == *id) {
                    if !partial.tail.is_empty() {
                        tool.output_tail = Some(self.redact_web_text(&partial.tail));
                    }
                    tool.elapsed_ms = Some(partial.progress.elapsed_ms);
                    tool.phase = partial.progress.phase.clone();
                }
            }
            AgentEvent::ToolEnd {
                id,
                name,
                result,
                is_error,
                provenance,
            } => {
                let summary = result
                    .content
                    .iter()
                    .filter_map(ContentBlock::as_text)
                    .find(|text| !text.trim().is_empty())
                    .map(|text| self.redact_web_text(&text.chars().take(240).collect::<String>()));
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.id == *id) {
                    tool.name = name.clone();
                    tool.provenance = provenance.clone();
                    tool.status = if *is_error { "failed" } else { "completed" }.to_string();
                    tool.result_summary = summary;
                    tool.is_error = *is_error;
                } else {
                    tools.push_back(surfaces::WebToolRunSurface {
                        id: id.clone(),
                        name: name.clone(),
                        provenance: provenance.clone(),
                        status: if *is_error { "failed" } else { "completed" }.to_string(),
                        args: serde_json::Value::Null,
                        output_tail: None,
                        result_summary: summary,
                        is_error: *is_error,
                        elapsed_ms: None,
                        phase: None,
                    });
                }
            }
            _ => {}
        }
        while tools.len() > TOOL_RUN_LOG_CAP {
            tools.pop_front();
        }
    }

    /// Snapshot the current transcript for `GET /api/web/surfaces`, re-indexing
    /// from zero so the browser sees a contiguous list after any eviction.
    pub(crate) fn conversation_segments(&self) -> Vec<surfaces::WebConversationSegment> {
        self.conversation_log
            .lock()
            .map(|log| {
                log.iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, mut seg)| {
                        seg.index = index;
                        seg
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Spawn the single-writer conversation accumulator: subscribes to the agent
/// event bus and folds turn/message events into `state.conversation_log` so a
/// browser reload can replay the transcript. A broadcast lag (slow consumer)
/// can drop events and leave a gap; the live stream remains authoritative for
/// the active turn, and the next `TurnStart` reopens a clean segment.
fn start_conversation_accumulator(state: &WebState) {
    let mut rx = state.events_tx.subscribe();
    let state = state.clone();
    crate::task_spawn::spawn_best_effort_result("web-conversation-accumulator", async move {
        loop {
            match rx.recv().await {
                Ok(event) => state.fold_conversation_event(&event),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
        Ok(())
    });
}

/// Commands received from WebSocket clients, forwarded to the main loop.
#[derive(Debug)]
pub enum WebCommand {
    UserPrompt {
        text: String,
        /// Resolved on-disk attachment paths (images render inline; other types
        /// are summarized downstream). Empty for a plain text prompt.
        image_paths: Vec<String>,
    },
    SlashCommand {
        name: String,
        args: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::SlashCommandResponse>>,
    },
    ExecuteControl {
        request: crate::control_runtime::ControlRequest,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    ManagedDelegateControl {
        method: String,
        payload: serde_json::Value,
        respond_to: tokio::sync::oneshot::Sender<serde_json::Value>,
    },
    Cancel,
    Shutdown,
    CancelCleaveChild {
        label: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::SlashCommandResponse>>,
    },
}

/// Start the embedded web server. Returns the bound address and a receiver
/// for web commands that should be processed by the main agent loop.
pub async fn start_server(
    state: WebState,
    preferred_port: u16,
) -> anyhow::Result<(WebStartupInfo, mpsc::Receiver<WebCommand>)> {
    let acp_state = acp_ws::AcpWebState {
        web_auth: state.web_auth.clone(),
        web_authority: state.web_authority.clone(),
        model: String::new(),
        cwd: std::env::current_dir().unwrap_or_default(),
        agent_id: None,
        dangerously_bypass_permissions: false,
        active_connections: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        shutdown: tokio_util::sync::CancellationToken::new(),
    };
    start_server_with_options(state, preferred_port, false, acp_state, None).await
}

pub async fn start_server_with_options(
    mut state: WebState,
    preferred_port: u16,
    strict_port: bool,
    acp_state: acp_ws::AcpWebState,
    tls: Option<crate::control_tls::ControlTlsConfig>,
) -> anyhow::Result<(WebStartupInfo, mpsc::Receiver<WebCommand>)> {
    // Create the command channel — caller gets the receiver
    let (cmd_tx, cmd_rx) = mpsc::channel(32);
    state.command_tx = cmd_tx;

    let token = state.web_auth.issue_query_token();
    let auth_mode = state.web_auth.mode_name();
    let auth_source = state.web_auth.source_name().to_string();
    let startup_info = state.startup_info.clone();
    let control_plane_state = state.control_plane_state.clone();
    let daemon_status = state.daemon_status.clone();
    let app_state_handles = state.handles.clone();

    let app = Router::new()
        .route(
            "/api/sessions/{session_id}/surfaces/stream",
            axum::routing::get(surface_stream::native_session_surface_stream_handler),
        )
        .route(
            "/api/sessions/{session_id}/surfaces",
            axum::routing::get(api::get_native_session_surfaces),
        )
        .route(
            "/api/sessions/{session_id}/actions",
            axum::routing::post(api::post_native_session_action),
        )
        .route(
            "/api/sessions/{session_id}",
            axum::routing::get(api::get_native_session),
        )
        .route(
            "/api/sessions",
            axum::routing::post(api::post_native_session),
        )
        .route(
            "/api/assistant-profiles/{id}",
            axum::routing::get(api::get_assistant_profile),
        )
        .route(
            "/api/assistant-runs/{run_id}",
            axum::routing::get(api::get_assistant_run),
        )
        .route(
            "/api/assistant-runs",
            axum::routing::get(api::get_assistant_runs),
        )
        .route(
            "/api/capabilities/assistants/{id}/readiness",
            axum::routing::get(api::get_capability_assistant_readiness),
        )
        .route(
            "/api/capabilities/assistants",
            axum::routing::get(api::get_capability_assistants),
        )
        .route(
            "/api/capabilities",
            axum::routing::get(api::get_capabilities),
        )
        .route("/api/state", axum::routing::get(api::get_state))
        .route(
            "/api/web/attachments/{id}",
            axum::routing::get(api::get_web_attachment),
        )
        .route(
            "/api/web/attachments",
            axum::routing::post(api::post_web_attachment),
        )
        .route(
            "/api/web/sessions/{session_id}",
            axum::routing::get(api::get_web_session),
        )
        .route(
            "/api/web/sessions",
            axum::routing::get(api::get_web_sessions),
        )
        .route(
            "/api/web/surfaces/stream",
            axum::routing::get(surface_stream::web_surface_stream_handler),
        )
        .route(
            "/api/web/actions",
            axum::routing::post(api::post_web_action),
        )
        .route(
            "/api/web/surfaces",
            axum::routing::get(api::get_web_surfaces),
        )
        .route(
            "/api/web/capabilities",
            axum::routing::get(api::get_web_capabilities),
        )
        .route(
            "/api/web/launch-context",
            axum::routing::get(api::get_web_launch_context),
        )
        .route(
            "/api/lifecycle/design/frontier",
            axum::routing::get(api::get_lifecycle_design_frontier),
        )
        .route(
            "/api/lifecycle/design/blocked",
            axum::routing::get(api::get_lifecycle_design_blocked),
        )
        .route(
            "/api/lifecycle/design/ready",
            axum::routing::get(api::get_lifecycle_design_ready),
        )
        .route(
            "/api/lifecycle/design/{id}",
            axum::routing::get(api::get_lifecycle_design_node),
        )
        .route(
            "/api/lifecycle/design",
            axum::routing::get(api::get_lifecycle_design),
        )
        .route(
            "/api/lifecycle/snapshot",
            axum::routing::get(api::get_lifecycle_snapshot),
        )
        .route(
            "/api/workspaces/leases",
            axum::routing::get(api::get_workspace_leases_status),
        )
        .route(
            "/api/providers/status",
            axum::routing::get(api::get_providers_status),
        )
        .route(
            "/api/extensions",
            axum::routing::get(api::get_extensions_status),
        )
        .route(
            "/api/runtime/capabilities",
            axum::routing::get(api::get_runtime_capabilities),
        )
        .route(
            "/api/runtime/status",
            axum::routing::get(api::get_runtime_status),
        )
        .route("/api/startup", axum::routing::get(api::get_startup))
        .route("/api/healthz", axum::routing::get(api::get_health))
        .route("/api/readyz", axum::routing::get(api::get_ready))
        .route("/api/graph", axum::routing::get(api::get_graph))
        .route(
            "/api/events/stream",
            axum::routing::get(api::get_events_stream),
        )
        .route(
            "/api/events",
            axum::routing::get(api::get_events).post(api::post_event),
        )
        .route("/api/evals", axum::routing::get(api::get_evals))
        .route(
            "/api/evals/compare",
            axum::routing::get(api::get_eval_compare),
        )
        .route("/api/evals/{*id}", axum::routing::get(api::get_eval))
        .route("/ws", axum::routing::get(ws::ws_handler))
        .route("/evals", axum::routing::get(serve_eval_dashboard))
        .route("/web", axum::routing::get(serve_omegon_web))
        .route("/", axum::routing::get(serve_dashboard))
        .merge(
            Router::new()
                .route("/acp", axum::routing::get(acp_ws::acp_ws_handler))
                .route("/api/acp", axum::routing::get(acp_ws::acp_ws_handler))
                .with_state(acp_state.clone()),
        )
        .layer(
            tower_http::cors::CorsLayer::new()
                // Allow any origin — the server is localhost-only (bound to 127.0.0.1)
                // and protected by auth token. Strict origin matching breaks WebSocket
                // upgrades because the browser sends Origin with the port
                // (http://127.0.0.1:7842) which doesn't match portless origins.
                .allow_origin(tower_http::cors::Any)
                .allow_methods([axum::http::Method::GET])
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(state.clone());

    // Bind directly — no TOCTOU race
    let listener = if strict_port {
        bind_strict(preferred_port).await?
    } else {
        bind_with_fallback(preferred_port).await?
    };
    let bound = listener.local_addr()?;
    let (http_scheme, ws_scheme) = crate::control_tls::schemes(tls.as_ref());
    if tls.is_some()
        && let Ok(mut status) = daemon_status.lock()
    {
        status.transport_warnings.clear();
    }

    let mut startup = WebStartupInfo {
        schema_version: 2,
        addr: bound.to_string(),
        http_base: format!("{http_scheme}://{bound}"),
        state_url: format!("{http_scheme}://{bound}/api/state"),
        startup_url: format!("{http_scheme}://{bound}/api/startup"),
        health_url: format!("{http_scheme}://{bound}/api/healthz"),
        ready_url: format!("{http_scheme}://{bound}/api/readyz"),
        ws_url: format!("{ws_scheme}://{bound}/ws?token={token}"),
        acp_url: Some(format!("{ws_scheme}://{bound}/api/acp?token={token}")),
        token,
        auth_mode: auth_mode.to_string(),
        auth_source,
        web_authority: state.web_authority.status(),
        control_plane_state: ControlPlaneState::Ready,
        daemon_status: daemon_status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default(),
        instance_descriptor: None,
    };
    startup.instance_descriptor = Some(project_web_instance(&app_state_handles, &startup));
    if let Ok(mut slot) = startup_info.lock() {
        *slot = Some(startup.clone());
    }
    if let Ok(mut status) = control_plane_state.lock() {
        *status = ControlPlaneState::Ready;
    }

    tracing::debug!(
        port = bound.port(),
        auth_mode = startup.auth_mode,
        auth_source = startup.auth_source,
        "web dashboard at {}/?token={} · interactive SPA at {}/web?token={}",
        startup.http_base,
        startup.token,
        startup.http_base,
        startup.token
    );

    crate::task_spawn::spawn_infra("web-server", async move {
        crate::control_tls::serve_router(listener, app, tls).await
    });

    start_daemon_event_worker(&state);
    start_conversation_accumulator(&state);
    if let Some(project_root) =
        std::env::var_os("OMEGON_PROJECT_ROOT").map(std::path::PathBuf::from)
    {
        start_loop_job_scheduler(&state, project_root);
    }

    Ok((startup, cmd_rx))
}

fn default_transport_warnings() -> Vec<String> {
    vec![
        "HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost."
            .into(),
    ]
}

fn refresh_startup_daemon_status(state: &WebState) {
    let mut daemon_status = match state.daemon_status.lock() {
        Ok(status) => status.clone(),
        Err(_) => return,
    };
    daemon_status.active_child_runtimes = state
        .handles
        .cleave
        .as_ref()
        .and_then(|lock| lock.lock().ok())
        .map(|cleave| {
            cleave
                .children
                .iter()
                .filter_map(|child| {
                    let runtime = child.runtime.as_ref()?;
                    Some(DaemonChildRuntimeStatus {
                        label: child.label.clone(),
                        status: child.status.clone(),
                        supervision_mode: child.supervision_mode.map(|mode| match mode {
                            crate::features::cleave::ChildSupervisionMode::Attached => {
                                "attached".to_string()
                            }
                            crate::features::cleave::ChildSupervisionMode::RecoveredDegraded => {
                                "recovered_degraded".to_string()
                            }
                            crate::features::cleave::ChildSupervisionMode::Lost => {
                                "lost".to_string()
                            }
                        }),
                        pid: child.pid,
                        model: runtime.model.clone(),
                        thinking_level: runtime.thinking_level.clone(),
                        context_class: runtime.context_class.clone(),
                        enabled_tools: runtime.enabled_tools.clone(),
                        disabled_tools: runtime.disabled_tools.clone(),
                        skills: runtime.skills.clone(),
                        enabled_extensions: runtime.enabled_extensions.clone(),
                        disabled_extensions: runtime.disabled_extensions.clone(),
                        preloaded_files: runtime.preloaded_files.clone(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    daemon_status.loop_scheduler = loop_scheduler_status();
    if let Ok(mut startup) = state.startup_info.lock()
        && let Some(startup) = startup.as_mut()
    {
        startup.daemon_status = daemon_status;
    }
}

fn loop_scheduler_status() -> WebLoopSchedulerStatus {
    let Some(project_root) = std::env::var_os("OMEGON_PROJECT_ROOT").map(std::path::PathBuf::from)
    else {
        return WebLoopSchedulerStatus::default();
    };
    let jobs =
        crate::features::loop_jobs::load_jobs_from_project(&project_root).unwrap_or_default();
    let configured_jobs = jobs.len();
    let enabled_jobs = jobs.iter().filter(|job| job.enabled).count();
    let disabled_jobs = configured_jobs.saturating_sub(enabled_jobs);
    let next_due_at = jobs
        .iter()
        .filter(|job| job.enabled)
        .filter_map(|job| crate::features::loop_jobs::next_due_at(&project_root, job))
        .min()
        .map(|due| due.to_rfc3339());
    let last_outcome = jobs
        .iter()
        .filter_map(|job| crate::features::loop_jobs::last_run_record(&project_root, &job.id))
        .filter_map(|record| {
            let ts = chrono::DateTime::parse_from_rfc3339(&record.fired_at).ok()?;
            Some((ts, format!("{}:{}", record.job_id, record.outcome)))
        })
        .max_by_key(|(ts, _)| *ts)
        .map(|(_, outcome)| outcome);

    WebLoopSchedulerStatus {
        configured_jobs,
        enabled_jobs,
        disabled_jobs,
        last_outcome,
        next_due_at,
    }
}

fn start_daemon_event_worker(state: &WebState) {
    if let Ok(mut status) = state.daemon_status.lock() {
        status.worker_running = true;
    }
    refresh_startup_daemon_status(state);

    let state = state.clone();
    crate::task_spawn::spawn_best_effort_result("web-daemon-event-worker", async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            ticker.tick().await;
            if let Err(err) = process_next_daemon_event(&state).await {
                tracing::warn!(?err, "daemon event worker failed to dispatch event");
            }
        }
        #[allow(unreachable_code)]
        Ok(())
    });
}

fn start_loop_job_scheduler(state: &WebState, project_root: std::path::PathBuf) {
    let state = state.clone();
    crate::task_spawn::spawn_best_effort_result("web-loop-job-scheduler", async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(err) = process_due_loop_jobs(&state, &project_root).await {
                tracing::warn!(?err, "loop job scheduler failed");
            }
        }
        #[allow(unreachable_code)]
        Ok(())
    });
}

async fn process_due_loop_jobs(
    state: &WebState,
    project_root: &std::path::Path,
) -> anyhow::Result<()> {
    let now = chrono::Utc::now();
    let mut jobs = crate::features::loop_jobs::load_jobs_from_project(project_root)?;
    let mut changed = false;

    for job in jobs.iter_mut().filter(|job| job.enabled) {
        let crate::features::loop_jobs::LoopTrigger::Every { duration } = &job.trigger else {
            continue;
        };
        let Some(interval) = crate::features::loop_jobs::parse_loop_duration(duration) else {
            crate::features::loop_jobs::append_run_record(
                project_root,
                &crate::features::loop_jobs::LoopRunRecord {
                    job_id: job.id.clone(),
                    fired_at: now.to_rfc3339(),
                    outcome: "invalid_duration".into(),
                    message: format!("unsupported interval '{duration}'"),
                },
            )?;
            job.enabled = false;
            changed = true;
            continue;
        };
        if let Some(last) = crate::features::loop_jobs::last_run_at(project_root, &job.id)
            && now - last < interval
        {
            continue;
        }

        let prompt_body = match std::fs::read_to_string(&job.prompt_path) {
            Ok(body) => body,
            Err(err) => {
                crate::features::loop_jobs::append_run_record(
                    project_root,
                    &crate::features::loop_jobs::LoopRunRecord {
                        job_id: job.id.clone(),
                        fired_at: now.to_rfc3339(),
                        outcome: "prompt_missing".into(),
                        message: err.to_string(),
                    },
                )?;
                job.enabled = false;
                changed = true;
                continue;
            }
        };
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest as _;
        hasher.update(prompt_body.as_bytes());
        let current_hash = format!("{:x}", hasher.finalize());
        if current_hash != job.prompt_sha256 {
            crate::features::loop_jobs::append_run_record(
                project_root,
                &crate::features::loop_jobs::LoopRunRecord {
                    job_id: job.id.clone(),
                    fired_at: now.to_rfc3339(),
                    outcome: "prompt_hash_changed".into(),
                    message: "prompt file changed; loop paused pending operator review".into(),
                },
            )?;
            job.enabled = false;
            changed = true;
            continue;
        }

        if let crate::features::loop_jobs::LoopStop::MaxRuns { max_runs } = job.stop
            && crate::features::loop_jobs::run_count(project_root, &job.id) >= max_runs as usize
        {
            crate::features::loop_jobs::append_run_record(
                project_root,
                &crate::features::loop_jobs::LoopRunRecord {
                    job_id: job.id.clone(),
                    fired_at: now.to_rfc3339(),
                    outcome: "max_runs_reached".into(),
                    message: format!("disabled after reaching {max_runs} runs"),
                },
            )?;
            job.enabled = false;
            changed = true;
            continue;
        }

        let prompt = format!(
            "Recurring loop job `{}` invoking prompt `{}`.\n\n{}",
            job.id, job.prompt, prompt_body
        );
        state
            .command_tx
            .send(WebCommand::UserPrompt {
                text: prompt,
                image_paths: Vec::new(),
            })
            .await?;
        crate::features::loop_jobs::append_run_record(
            project_root,
            &crate::features::loop_jobs::LoopRunRecord {
                job_id: job.id.clone(),
                fired_at: now.to_rfc3339(),
                outcome: "dispatched".into(),
                message: "queued user prompt from loop scheduler".into(),
            },
        )?;
    }

    if changed {
        crate::features::loop_jobs::save_jobs_to_project(project_root, &jobs)?;
    }
    Ok(())
}

pub(crate) async fn process_next_daemon_event(state: &WebState) -> anyhow::Result<bool> {
    let event = {
        let mut queue = state
            .daemon_events
            .lock()
            .map_err(|_| anyhow::anyhow!("daemon event queue unavailable"))?;
        if queue.is_empty() {
            return Ok(false);
        }
        let event = queue.remove(0);
        let remaining = queue.len();
        if let Ok(mut status) = state.daemon_status.lock() {
            status.queued_events = remaining;
        }
        event
    };

    let dispatch_result = match event.trigger_kind.as_str() {
        "prompt" => event
            .payload
            .get("text")
            .and_then(|value| value.as_str())
            .map(|text| WebCommand::UserPrompt {
                text: text.to_string(),
                image_paths: Vec::new(),
            }),
        "slash-command" => event
            .payload
            .get("name")
            .and_then(|value| value.as_str())
            .map(|name| WebCommand::SlashCommand {
                name: name.to_string(),
                args: event
                    .payload
                    .get("args")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                respond_to: None,
            }),
        "cancel" => Some(WebCommand::Cancel),
        "new-session" => Some(WebCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::NewSession,
            respond_to: None,
        }),
        "context-status" => Some(WebCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ContextStatus,
            respond_to: None,
        }),
        "context-compact" => Some(WebCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ContextCompact,
            respond_to: None,
        }),
        "context-clear" => Some(WebCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ContextClear,
            respond_to: None,
        }),
        "auth-status" => Some(WebCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::AuthStatus,
            respond_to: None,
        }),
        "model-view" => Some(WebCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ModelView,
            respond_to: None,
        }),
        "model-list" => Some(WebCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ModelList,
            respond_to: None,
        }),
        "set-model" => event
            .payload
            .get("model")
            .and_then(|value| value.as_str())
            .map(|model| WebCommand::ExecuteControl {
                request: crate::control_runtime::ControlRequest::SetModel {
                    requested_model: model.to_string(),
                },
                respond_to: None,
            }),
        "set-thinking" => event
            .payload
            .get("level")
            .and_then(|value| value.as_str())
            .and_then(crate::settings::ThinkingLevel::parse)
            .map(|level| WebCommand::ExecuteControl {
                request: crate::control_runtime::ControlRequest::SetThinking { level },
                respond_to: None,
            }),
        "shutdown" => Some(WebCommand::Shutdown),
        "cancel-cleave-child" => event
            .payload
            .get("label")
            .and_then(|value| value.as_str())
            .map(|label| WebCommand::CancelCleaveChild {
                label: label.to_string(),
                respond_to: None,
            }),
        _ => None,
    };

    match dispatch_result {
        Some(command) => {
            state.command_tx.send(command).await?;
            if let Ok(mut status) = state.daemon_status.lock() {
                status.processed_events += 1;
            }
        }
        None => {
            if let Ok(mut status) = state.daemon_status.lock() {
                status.transport_warnings.push(format!(
                    "Unsupported daemon event trigger '{}' from {}.",
                    event.trigger_kind, event.source
                ));
            }
            let _ = state
                .events_tx
                .send(omegon_traits::AgentEvent::SystemNotification {
                    message: format!(
                        "⚠ Daemon event ingress is degraded: unsupported trigger '{}' from {}",
                        event.trigger_kind, event.source
                    ),
                });
        }
    }

    refresh_startup_daemon_status(state);
    Ok(true)
}

/// Serve the embedded dashboard HTML.
async fn serve_dashboard() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("assets/dashboard.html"))
}

/// Serve the embedded Omegon Web single-agent SPA.
///
/// Consumes the `/api/web/*` surface contract: an initial `GET /api/web/surfaces`
/// snapshot, the `/api/web/surfaces/stream` WebSocket for live deltas, and
/// `POST /api/web/actions` for prompt/cancel/slash submission.
async fn serve_omegon_web() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("assets/omegon-web.html"))
}

async fn serve_eval_dashboard() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("assets/eval-dashboard.html"))
}

/// Bind to a port with auto-increment fallback. Returns the listener directly
/// to avoid TOCTOU races.
/// Resolve the bind IP. Container workloads set OMEGON_BIND_ADDR=0.0.0.0
/// so the control plane is reachable via port-forward.
fn bind_ip() -> [u8; 4] {
    match std::env::var("OMEGON_BIND_ADDR").ok().as_deref() {
        Some("0.0.0.0") => [0, 0, 0, 0],
        _ => [127, 0, 0, 1],
    }
}

async fn bind_strict(port: u16) -> anyhow::Result<tokio::net::TcpListener> {
    let addr: SocketAddr = (bind_ip(), port).into();
    tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to bind control port {port}: {e}\n  \
             Hint: another process may be using this port. Check with: lsof -i :{port}\n  \
             Use --strict-port=false to auto-fallback to the next available port."
        )
    })
}

async fn bind_with_fallback(preferred: u16) -> anyhow::Result<tokio::net::TcpListener> {
    for offset in 0..10 {
        let port = preferred + offset;
        let addr: SocketAddr = (bind_ip(), port).into();
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(_) => continue,
        }
    }
    anyhow::bail!(
        "No available port found in range {preferred}-{}",
        preferred + 9
    )
}

/// Generate a random auth token for the web server.
pub(crate) fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Simple token from timestamp + pid — not cryptographic, just prevents
    // casual cross-origin access and local process snooping.
    format!("{:x}{:x}", seed, std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bind_with_fallback_succeeds() {
        let listener = bind_with_fallback(18000).await.unwrap();
        assert!(listener.local_addr().unwrap().port() >= 18000);
    }

    #[test]
    fn generate_token_is_nonempty() {
        let token = generate_token();
        assert!(!token.is_empty());
        assert!(token.len() >= 8);
    }

    #[tokio::test]
    async fn serves_omegon_web_spa_referencing_the_contract() {
        // The embedded SPA must be present and wired to the /api/web/* contract
        // it consumes; a silent rename of an endpoint would break the UI without
        // a compile error, so assert the references here.
        let html = serve_omegon_web().await.0;
        assert!(html.contains("Omegon Web"));
        assert!(html.contains("/api/web/surfaces"));
        assert!(html.contains("/api/web/surfaces/stream"));
        assert!(html.contains("/api/web/actions"));
        // Action discriminators the backend accepts.
        assert!(html.contains("submit_prompt"));
        assert!(html.contains("cancel_active_turn"));
        assert!(html.contains("run_slash_command"));
        // Token is taken from the query string (WS auth).
        assert!(html.contains("token"));
    }

    #[test]
    fn web_state_issues_attach_token_for_query_use() {
        let state = WebState::new(
            DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
        );
        let token = state.web_auth.issue_query_token();

        assert!(!token.is_empty());
        assert!(state.web_auth.verify_query_token(Some(&token)));
    }

    #[test]
    fn startup_info_carries_auth_metadata() {
        let state = WebState::with_auth_state(
            DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
            WebAuthState::ephemeral_generated("token-123".into()),
        );
        let startup = WebStartupInfo {
            schema_version: 2,
            addr: "127.0.0.1:7842".into(),
            http_base: "http://127.0.0.1:7842".into(),
            state_url: "http://127.0.0.1:7842/api/state".into(),
            startup_url: "http://127.0.0.1:7842/api/startup".into(),
            health_url: "http://127.0.0.1:7842/api/healthz".into(),
            ready_url: "http://127.0.0.1:7842/api/readyz".into(),
            ws_url: "ws://127.0.0.1:7842/ws?token=token-123".into(),
            acp_url: None,
            token: state.web_auth.issue_query_token(),
            auth_mode: state.web_auth.mode_name().into(),
            auth_source: state.web_auth.source_name().into(),
            web_authority: WebAuthorityConfig::default().status(),
            control_plane_state: ControlPlaneState::Ready,
            daemon_status: WebDaemonStatus {
                transport_warnings: default_transport_warnings(),
                ..WebDaemonStatus::default()
            },
            instance_descriptor: None,
        };

        assert_eq!(startup.token, "token-123");
        assert_eq!(startup.auth_mode, "ephemeral-bearer");
        assert_eq!(startup.auth_source, "generated");
        assert_eq!(startup.state_url, "http://127.0.0.1:7842/api/state");
        assert_eq!(startup.startup_url, "http://127.0.0.1:7842/api/startup");
        assert_eq!(startup.health_url, "http://127.0.0.1:7842/api/healthz");
        assert_eq!(startup.ready_url, "http://127.0.0.1:7842/api/readyz");
        assert_eq!(startup.ws_url, "ws://127.0.0.1:7842/ws?token=token-123");
        assert_eq!(startup.control_plane_state, ControlPlaneState::Ready);
        assert_eq!(startup.daemon_status.queued_events, 0);
        assert!(
            startup
                .daemon_status
                .transport_warnings
                .iter()
                .any(|warning| warning.contains("insecure bootstrap"))
        );
        let descriptor = project_web_instance(&state.handles, &startup);
        assert_eq!(
            descriptor.control_plane.http_transport_security,
            Some(OmegonTransportSecurity::InsecureBootstrap)
        );
        assert_eq!(
            descriptor.control_plane.ws_transport_security,
            Some(OmegonTransportSecurity::InsecureBootstrap)
        );
    }

    #[test]
    fn project_descriptor_marks_tls_control_plane_secure() {
        let state = WebState::new(
            DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
        );
        let startup = WebStartupInfo {
            schema_version: 2,
            addr: "127.0.0.1:7842".into(),
            http_base: "https://127.0.0.1:7842".into(),
            state_url: "https://127.0.0.1:7842/api/state".into(),
            startup_url: "https://127.0.0.1:7842/api/startup".into(),
            health_url: "https://127.0.0.1:7842/api/healthz".into(),
            ready_url: "https://127.0.0.1:7842/api/readyz".into(),
            ws_url: "wss://127.0.0.1:7842/ws?token=test".into(),
            acp_url: Some("wss://127.0.0.1:7842/acp?token=test".into()),
            token: "test".into(),
            auth_mode: "ephemeral-bearer".into(),
            auth_source: "generated".into(),
            web_authority: WebAuthorityConfig::default().status(),
            control_plane_state: ControlPlaneState::Ready,
            daemon_status: WebDaemonStatus::default(),
            instance_descriptor: None,
        };

        let descriptor = project_web_instance(&state.handles, &startup);
        assert_eq!(
            descriptor.control_plane.http_transport_security,
            Some(OmegonTransportSecurity::Secure)
        );
        assert_eq!(
            descriptor.control_plane.ws_transport_security,
            Some(OmegonTransportSecurity::Secure)
        );
    }

    #[tokio::test]
    async fn bind_strict_fails_when_port_is_taken() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();

        let err = bind_strict(port).await.unwrap_err();
        assert!(err.to_string().contains("Failed to bind control port"));
    }

    #[test]
    fn generate_token_is_unique() {
        let t1 = generate_token();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = generate_token();
        // Not guaranteed unique from timestamps alone, but in practice different
        assert_ne!(t1, t2);
    }

    #[tokio::test]
    async fn daemon_event_worker_dispatches_prompt_and_updates_status() {
        let state = WebState::new(
            DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
        );
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let startup = WebStartupInfo {
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
            web_authority: WebAuthorityConfig::default().status(),
            control_plane_state: ControlPlaneState::Ready,
            daemon_status: WebDaemonStatus::default(),
            instance_descriptor: None,
        };
        let state = WebState {
            command_tx,
            startup_info: Arc::new(Mutex::new(Some(startup))),
            ..state
        };
        state
            .daemon_events
            .lock()
            .unwrap()
            .push(DaemonEventEnvelope {
                event_id: "evt-1".into(),
                source: "manual/test".into(),
                trigger_kind: "prompt".into(),
                payload: serde_json::json!({"text": "hello from queue"}),
                caller_role: Some("admin".into()),
                source_user: None,
                source_channel: None,
                source_thread: None,
            });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::UserPrompt { text, .. } => assert_eq!(text, "hello from queue"),
            other => panic!("wrong command: {other:?}"),
        }
        let status = state.daemon_status.lock().unwrap().clone();
        assert_eq!(status.queued_events, 0);
        assert_eq!(status.processed_events, 1);
        let startup_status = state
            .startup_info
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .daemon_status
            .clone();
        assert_eq!(startup_status.processed_events, 1);
    }

    fn test_turn_end() -> omegon_traits::AgentEvent {
        omegon_traits::AgentEvent::TurnEnd(Box::new(omegon_traits::AgentEventTurnEnd {
            turn: 1,
            turn_end_reason: omegon_traits::TurnEndReason::AssistantCompleted,
            model: None,
            provider: None,
            estimated_tokens: 0,
            context_window: 0,
            context_composition: omegon_traits::ContextComposition::default(),
            actual_input_tokens: 0,
            actual_output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            provider_telemetry: None,
            dominant_phase: None,
            drift_kind: None,
            progress_nudge_reason: None,
            intent_task: None,
            intent_phase: None,
            files_read_count: 0,
            files_modified_count: 0,
            stats_tool_calls: 0,
            streaks: omegon_traits::ControllerStreaks::default(),
        }))
    }

    #[test]
    fn conversation_fold_records_user_and_assistant_turns() {
        use omegon_traits::AgentEvent;
        let (events_tx, _rx) = tokio::sync::broadcast::channel(8);
        let state = WebState::new(DashboardHandles::default(), events_tx);

        // User prompt is recorded at submission; agent turn folds from the bus.
        state.record_user_segment("hello");
        state.fold_conversation_event(&AgentEvent::TurnStart { turn: 1 });
        state.fold_conversation_event(&AgentEvent::MessageChunk { text: "hi ".into() });
        state.fold_conversation_event(&AgentEvent::MessageChunk {
            text: "there".into(),
        });
        state.fold_conversation_event(&test_turn_end());

        let segs = state.conversation_segments();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].role, "user");
        assert_eq!(segs[0].body.as_deref(), Some("hello"));
        assert!(segs[0].complete);
        assert_eq!(segs[1].role, "assistant");
        assert_eq!(segs[1].body.as_deref(), Some("hi there"));
        assert!(segs[1].complete);
        // Contiguous indices after re-indexing.
        assert_eq!(segs[0].index, 0);
        assert_eq!(segs[1].index, 1);
    }

    #[test]
    fn conversation_fold_drops_text_free_assistant_turn() {
        use omegon_traits::AgentEvent;
        let (events_tx, _rx) = tokio::sync::broadcast::channel(8);
        let state = WebState::new(DashboardHandles::default(), events_tx);
        // A tool-only turn produces no MessageChunk; it must not leave an empty
        // assistant bubble in the reload transcript.
        state.fold_conversation_event(&AgentEvent::TurnStart { turn: 1 });
        state.fold_conversation_event(&test_turn_end());
        assert!(state.conversation_segments().is_empty());
    }

    #[tokio::test]
    async fn daemon_event_worker_dispatches_new_session_trigger() {
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(DashboardHandles::default(), events_tx);
        let state = WebState {
            command_tx,
            ..state
        };
        state
            .daemon_events
            .lock()
            .unwrap()
            .push(DaemonEventEnvelope {
                event_id: "evt-new-session".into(),
                source: "manual/test".into(),
                trigger_kind: "new-session".into(),
                payload: serde_json::json!({}),
                caller_role: Some("admin".into()),
                source_user: None,
                source_channel: None,
                source_thread: None,
            });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::ExecuteControl {
                request: crate::control_runtime::ControlRequest::NewSession,
                respond_to: None,
            } => {}
            other => panic!("wrong command: {other:?}"),
        }
        let status = state.daemon_status.lock().unwrap().clone();
        assert_eq!(status.queued_events, 0);
        assert_eq!(status.processed_events, 1);
    }

    #[tokio::test]
    async fn daemon_event_worker_dispatches_shutdown_trigger() {
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(DashboardHandles::default(), events_tx);
        let state = WebState {
            command_tx,
            ..state
        };
        state
            .daemon_events
            .lock()
            .unwrap()
            .push(DaemonEventEnvelope {
                event_id: "evt-shutdown".into(),
                source: "manual/test".into(),
                trigger_kind: "shutdown".into(),
                payload: serde_json::json!({}),
                caller_role: Some("admin".into()),
                source_user: None,
                source_channel: None,
                source_thread: None,
            });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::Shutdown => {}
            other => panic!("wrong command: {other:?}"),
        }
        let status = state.daemon_status.lock().unwrap().clone();
        assert_eq!(status.queued_events, 0);
        assert_eq!(status.processed_events, 1);
    }

    #[tokio::test]
    async fn daemon_event_worker_dispatches_cancel_cleave_child_trigger() {
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(DashboardHandles::default(), events_tx);
        let state = WebState {
            command_tx,
            ..state
        };
        state
            .daemon_events
            .lock()
            .unwrap()
            .push(DaemonEventEnvelope {
                event_id: "evt-cancel-child".into(),
                source: "manual/test".into(),
                trigger_kind: "cancel-cleave-child".into(),
                payload: serde_json::json!({"label": "alpha"}),
                caller_role: Some("admin".into()),
                source_user: None,
                source_channel: None,
                source_thread: None,
            });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::CancelCleaveChild { label, .. } => assert_eq!(label, "alpha"),
            other => panic!("wrong command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn daemon_event_worker_preserves_child_runtime_metadata_in_startup_status() {
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::with_auth_state(
            DashboardHandles {
                cleave: Some(Arc::new(Mutex::new(
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
                            supervision_mode: Some(
                                crate::features::cleave::ChildSupervisionMode::RecoveredDegraded,
                            ),
                            duration_secs: None,
                            pid: Some(4242),
                            last_tool: None,
                            last_tool_activity: None,
                            last_turn: None,
                            tasks: Vec::new(),
                            tasks_done: 0,
                            started_at: None,
                            last_activity_at: None,
                            tokens_in: 0,
                            tokens_out: 0,
                            runtime: Some(crate::features::cleave::ChildRuntimeSummary {
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
                            }),
                        }],
                        total_tokens_in: 0,
                        total_tokens_out: 0,
                    },
                ))),
                ..DashboardHandles::default()
            },
            events_tx,
            WebAuthState::ephemeral_generated("test".into()),
        );
        let startup = WebStartupInfo {
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
            web_authority: WebAuthorityConfig::default().status(),
            control_plane_state: ControlPlaneState::Ready,
            daemon_status: WebDaemonStatus::default(),
            instance_descriptor: None,
        };
        let state = WebState {
            command_tx,
            startup_info: Arc::new(Mutex::new(Some(startup))),
            ..state
        };
        refresh_startup_daemon_status(&state);
        state
            .daemon_events
            .lock()
            .unwrap()
            .push(DaemonEventEnvelope {
                event_id: "evt-rt-1".into(),
                source: "manual/test".into(),
                trigger_kind: "prompt".into(),
                payload: serde_json::json!({"text": "runtime check"}),
                caller_role: Some("admin".into()),
                source_user: None,
                source_channel: None,
                source_thread: None,
            });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::UserPrompt { text, .. } => assert_eq!(text, "runtime check"),
            other => panic!("wrong command: {other:?}"),
        }

        let startup_status = state
            .startup_info
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .daemon_status
            .clone();
        assert_eq!(startup_status.processed_events, 1);
        assert_eq!(startup_status.active_child_runtimes.len(), 1);
        let child = &startup_status.active_child_runtimes[0];
        assert_eq!(child.label, "child-1");
        assert_eq!(child.pid, Some(4242));
        assert_eq!(
            child.supervision_mode.as_deref(),
            Some("recovered_degraded")
        );
        assert_eq!(child.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(child.thinking_level.as_deref(), Some("high"));
        assert_eq!(child.context_class.as_deref(), Some("massive"));
        assert_eq!(child.disabled_tools, vec!["bash"]);
        assert_eq!(child.enabled_extensions, vec!["alpha"]);
        assert_eq!(child.preloaded_files, vec!["docs/runtime-preload.md"]);
    }

    #[tokio::test]
    async fn daemon_event_worker_marks_unsupported_trigger_as_degraded() {
        let (events_tx, mut events_rx) = tokio::sync::broadcast::channel(4);
        let state = WebState::new(DashboardHandles::default(), events_tx);
        state
            .daemon_events
            .lock()
            .unwrap()
            .push(DaemonEventEnvelope {
                event_id: "evt-2".into(),
                source: "manual/test".into(),
                trigger_kind: "mystery".into(),
                payload: serde_json::json!({"ignored": true}),
                caller_role: Some("admin".into()),
                source_user: None,
                source_channel: None,
                source_thread: None,
            });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let status = state.daemon_status.lock().unwrap().clone();
        assert_eq!(status.queued_events, 0);
        assert!(
            status
                .transport_warnings
                .iter()
                .any(|warning| warning.contains("Unsupported daemon event trigger 'mystery'"))
        );
        let event = events_rx.recv().await.unwrap();
        match event {
            omegon_traits::AgentEvent::SystemNotification { message } => {
                assert!(message.contains("degraded"), "got: {message}");
                assert!(message.contains("mystery"), "got: {message}");
            }
            other => panic!("wrong event: {other:?}"),
        }
    }
    #[tokio::test]
    async fn loop_scheduler_dispatches_due_job_and_records_run() {
        let dir = tempfile::tempdir().unwrap();
        let prompt_path = dir.path().join("prompt.md");
        std::fs::write(&prompt_path, "loop body").unwrap();
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest as _;
        hasher.update("loop body".as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        crate::features::loop_jobs::save_jobs_to_project(
            dir.path(),
            &[crate::features::loop_jobs::LoopJob {
                id: "loop-test".into(),
                prompt: "test-prompt".into(),
                trigger: crate::features::loop_jobs::LoopTrigger::Every {
                    duration: "1s".into(),
                },
                stop: crate::features::loop_jobs::LoopStop::OperatorStop,
                concurrency: crate::features::loop_jobs::LoopConcurrencyPolicy::SkipIfRunning,
                enabled: true,
                prompt_path: prompt_path.display().to_string(),
                prompt_sha256: hash,
                created_at: chrono::Utc::now().to_rfc3339(),
            }],
        )
        .unwrap();

        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState {
            command_tx,
            ..WebState::new(
                DashboardHandles::default(),
                tokio::sync::broadcast::channel(16).0,
            )
        };

        process_due_loop_jobs(&state, dir.path()).await.unwrap();
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::UserPrompt { text, .. } => {
                assert!(text.contains("Recurring loop job `loop-test`"));
                assert!(text.contains("loop body"));
            }
            other => panic!("wrong command: {other:?}"),
        }
        assert_eq!(
            crate::features::loop_jobs::run_count(dir.path(), "loop-test"),
            1
        );
    }

    #[tokio::test]
    async fn loop_scheduler_pauses_job_when_prompt_hash_changes() {
        let dir = tempfile::tempdir().unwrap();
        let prompt_path = dir.path().join("prompt.md");
        std::fs::write(&prompt_path, "changed body").unwrap();
        crate::features::loop_jobs::save_jobs_to_project(
            dir.path(),
            &[crate::features::loop_jobs::LoopJob {
                id: "loop-drift".into(),
                prompt: "test-prompt".into(),
                trigger: crate::features::loop_jobs::LoopTrigger::Every {
                    duration: "1s".into(),
                },
                stop: crate::features::loop_jobs::LoopStop::OperatorStop,
                concurrency: crate::features::loop_jobs::LoopConcurrencyPolicy::SkipIfRunning,
                enabled: true,
                prompt_path: prompt_path.display().to_string(),
                prompt_sha256: "stale".into(),
                created_at: chrono::Utc::now().to_rfc3339(),
            }],
        )
        .unwrap();

        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState {
            command_tx,
            ..WebState::new(
                DashboardHandles::default(),
                tokio::sync::broadcast::channel(16).0,
            )
        };

        process_due_loop_jobs(&state, dir.path()).await.unwrap();
        assert!(command_rx.try_recv().is_err());
        let jobs = crate::features::loop_jobs::load_jobs_from_project(dir.path()).unwrap();
        assert!(!jobs[0].enabled);
        let last = crate::features::loop_jobs::last_run_record(dir.path(), "loop-drift").unwrap();
        assert_eq!(last.outcome, "prompt_hash_changed");
    }

    #[tokio::test]
    async fn loop_scheduler_disables_after_max_runs() {
        let dir = tempfile::tempdir().unwrap();
        let prompt_path = dir.path().join("prompt.md");
        std::fs::write(&prompt_path, "loop body").unwrap();
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest as _;
        hasher.update("loop body".as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        crate::features::loop_jobs::append_run_record(
            dir.path(),
            &crate::features::loop_jobs::LoopRunRecord {
                job_id: "loop-max".into(),
                fired_at: (chrono::Utc::now() - chrono::Duration::seconds(5)).to_rfc3339(),
                outcome: "dispatched".into(),
                message: "previous".into(),
            },
        )
        .unwrap();
        crate::features::loop_jobs::save_jobs_to_project(
            dir.path(),
            &[crate::features::loop_jobs::LoopJob {
                id: "loop-max".into(),
                prompt: "test-prompt".into(),
                trigger: crate::features::loop_jobs::LoopTrigger::Every {
                    duration: "1s".into(),
                },
                stop: crate::features::loop_jobs::LoopStop::MaxRuns { max_runs: 1 },
                concurrency: crate::features::loop_jobs::LoopConcurrencyPolicy::SkipIfRunning,
                enabled: true,
                prompt_path: prompt_path.display().to_string(),
                prompt_sha256: hash,
                created_at: chrono::Utc::now().to_rfc3339(),
            }],
        )
        .unwrap();

        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState {
            command_tx,
            ..WebState::new(
                DashboardHandles::default(),
                tokio::sync::broadcast::channel(16).0,
            )
        };

        process_due_loop_jobs(&state, dir.path()).await.unwrap();
        assert!(command_rx.try_recv().is_err());
        let jobs = crate::features::loop_jobs::load_jobs_from_project(dir.path()).unwrap();
        assert!(!jobs[0].enabled);
        let last = crate::features::loop_jobs::last_run_record(dir.path(), "loop-max").unwrap();
        assert_eq!(last.outcome, "max_runs_reached");
    }

    #[test]
    fn web_state_accumulates_tool_runs_for_instrument_snapshot() {
        let state = WebState::new(
            DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
        );
        state.fold_conversation_event(&omegon_traits::AgentEvent::ToolStart {
            id: "tool-1".into(),
            name: "bash".into(),
            provenance: omegon_traits::ToolProvenance::BuiltIn,
            args: serde_json::json!({"command":"pwd"}),
        });
        state.fold_conversation_event(&omegon_traits::AgentEvent::ToolUpdate {
            id: "tool-1".into(),
            partial: omegon_traits::PartialToolResult {
                tail: "workspace".into(),
                progress: omegon_traits::ToolProgress {
                    elapsed_ms: 42,
                    phase: Some("reading cwd".into()),
                    ..omegon_traits::ToolProgress::default()
                },
                details: serde_json::Value::Null,
            },
        });
        state.fold_conversation_event(&omegon_traits::AgentEvent::ToolEnd {
            id: "tool-1".into(),
            name: "bash".into(),
            provenance: omegon_traits::ToolProvenance::BuiltIn,
            result: omegon_traits::ToolResult {
                content: vec![omegon_traits::ContentBlock::Text {
                    text: "done".into(),
                }],
                details: serde_json::Value::Null,
            },
            is_error: false,
        });

        let tools = state.tool_runs.lock().unwrap();
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(tool.id, "tool-1");
        assert_eq!(tool.name, "bash");
        assert_eq!(tool.status, "completed");
        assert_eq!(tool.output_tail.as_deref(), Some("workspace"));
        assert_eq!(tool.result_summary.as_deref(), Some("done"));
        assert_eq!(tool.elapsed_ms, Some(42));
        assert_eq!(tool.phase.as_deref(), Some("reading cwd"));
    }

    #[test]
    fn web_state_redacts_tool_run_snapshot_fields() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = std::sync::Arc::new(omegon_secrets::SecretsManager::new(dir.path()).unwrap());
        secrets.register_redaction_secret("TEST_WEB_TOKEN", "super-secret-token");
        let state = WebState::with_auth_state_and_secrets(
            DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
            WebAuthState::ephemeral_generated("test-token".to_string()),
            Some(secrets),
        );
        state.fold_conversation_event(&omegon_traits::AgentEvent::ToolStart {
            id: "tool-secret".into(),
            name: "bash".into(),
            provenance: omegon_traits::ToolProvenance::BuiltIn,
            args: serde_json::json!({"command":"curl -H 'Authorization: Bearer super-secret-token'"}),
        });
        state.fold_conversation_event(&omegon_traits::AgentEvent::ToolUpdate {
            id: "tool-secret".into(),
            partial: omegon_traits::PartialToolResult {
                tail: "using super-secret-token".into(),
                progress: omegon_traits::ToolProgress::default(),
                details: serde_json::Value::Null,
            },
        });
        state.fold_conversation_event(&omegon_traits::AgentEvent::ToolEnd {
            id: "tool-secret".into(),
            name: "bash".into(),
            provenance: omegon_traits::ToolProvenance::BuiltIn,
            result: omegon_traits::ToolResult {
                content: vec![omegon_traits::ContentBlock::Text {
                    text: "result contained super-secret-token".into(),
                }],
                details: serde_json::Value::Null,
            },
            is_error: false,
        });

        let tools = state.tool_runs.lock().unwrap();
        let tool = tools.front().unwrap();
        let serialized = serde_json::to_string(tool).unwrap();
        assert!(!serialized.contains("super-secret-token"));
        assert!(serialized.contains("[REDACTED"));
    }

    #[test]
    fn web_state_bounds_tool_run_history() {
        let state = WebState::new(
            DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
        );
        for idx in 0..(TOOL_RUN_LOG_CAP + 3) {
            state.fold_conversation_event(&omegon_traits::AgentEvent::ToolStart {
                id: format!("tool-{idx}"),
                name: "bash".into(),
                provenance: omegon_traits::ToolProvenance::BuiltIn,
                args: serde_json::json!({}),
            });
        }
        let tools = state.tool_runs.lock().unwrap();
        assert_eq!(tools.len(), TOOL_RUN_LOG_CAP);
        assert_eq!(tools.front().unwrap().id, "tool-3");
    }
}

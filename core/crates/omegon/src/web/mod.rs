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

pub mod api;
pub mod auth;
pub mod ws;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use tokio::sync::{broadcast, mpsc};

use crate::tui::dashboard::DashboardHandles;
pub use auth::{WEB_AUTH_SECRET_NAME, WebAuthSource, WebAuthState};
use omegon_traits::{
    DaemonEventEnvelope, IpcHarnessSnapshot, IpcHealthSnapshot, IpcHealthState,
    IpcMemorySnapshot, OmegonTransportSecurity,
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
pub struct WebDaemonStatus {
    pub queued_events: usize,
    pub processed_events: usize,
    pub worker_running: bool,
    pub transport_warnings: Vec<String>,
    pub active_child_runtimes: Vec<DaemonChildRuntimeStatus>,
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
    pub token: String,
    pub auth_mode: String,
    pub auth_source: String,
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
            capability_tier: h.capability_tier.clone(),
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
        })
        .unwrap_or(IpcHarnessSnapshot {
            context_class: "Squad".into(),
            thinking_level: "Medium".into(),
            capability_tier: "victory".into(),
            runtime_profile: "primary-interactive".into(),
            autonomy_mode: "operator-driven".into(),
            dispatcher: omegon_traits::IpcDispatcherSnapshot {
                available_options: vec!["retribution".into(), "victory".into(), "gloriana".into()],
                switch_state: "idle".into(),
                request_id: None,
                expected_profile: None,
                expected_model: None,
                active_profile: Some("victory".into()),
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
        busy: handles
            .session
            .lock()
            .map(|s| s.busy)
            .unwrap_or(false),
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
    instance.control_plane.http_transport_security = Some(OmegonTransportSecurity::InsecureBootstrap);
    instance.control_plane.ws_transport_security = Some(OmegonTransportSecurity::InsecureBootstrap);
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
    /// Received daemon/event-ingress envelopes (v1 in-memory queue).
    pub daemon_events: Arc<Mutex<Vec<DaemonEventEnvelope>>>,
    /// Shared queue/worker status for daemon event ingress.
    pub daemon_status: Arc<Mutex<WebDaemonStatus>>,
}

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
        let (command_tx, _) = mpsc::channel(32); // receiver returned by start_server
        Self {
            handles,
            events_tx,
            command_tx,
            web_auth: Arc::new(auth_state),
            startup_info: Arc::new(Mutex::new(None)),
            control_plane_state: Arc::new(Mutex::new(ControlPlaneState::Starting)),
            daemon_events: Arc::new(Mutex::new(Vec::new())),
            daemon_status: Arc::new(Mutex::new(WebDaemonStatus {
                transport_warnings: default_transport_warnings(),
                ..WebDaemonStatus::default()
            })),
        }
    }
}

/// Commands received from WebSocket clients, forwarded to the main loop.
#[derive(Debug)]
pub enum WebCommand {
    UserPrompt(String),
    SlashCommand {
        name: String,
        args: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::SlashCommandResponse>>,
    },
    ExecuteControl {
        request: crate::control_runtime::ControlRequest,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
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
    start_server_with_options(state, preferred_port, false).await
}

pub async fn start_server_with_options(
    mut state: WebState,
    preferred_port: u16,
    strict_port: bool,
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
        .route("/api/state", axum::routing::get(api::get_state))
        .route("/api/startup", axum::routing::get(api::get_startup))
        .route("/api/healthz", axum::routing::get(api::get_health))
        .route("/api/readyz", axum::routing::get(api::get_ready))
        .route("/api/graph", axum::routing::get(api::get_graph))
        .route("/api/events", axum::routing::post(api::post_event))
        .route("/ws", axum::routing::get(ws::ws_handler))
        .route("/", axum::routing::get(serve_dashboard))
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

    let mut startup = WebStartupInfo {
        schema_version: 2,
        addr: bound.to_string(),
        http_base: format!("http://{bound}"),
        state_url: format!("http://{bound}/api/state"),
        startup_url: format!("http://{bound}/api/startup"),
        health_url: format!("http://{bound}/api/healthz"),
        ready_url: format!("http://{bound}/api/readyz"),
        ws_url: format!("ws://{bound}/ws?token={token}"),
        token,
        auth_mode: auth_mode.to_string(),
        auth_source,
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
        "web dashboard at {}/?token={}",
        startup.http_base,
        startup.token
    );

    crate::task_spawn::spawn_infra("web-server", async move {
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    });

    start_daemon_event_worker(&state);

    Ok((startup, cmd_rx))
}

fn default_transport_warnings() -> Vec<String> {
    vec![
        "HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into(),
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
                            crate::features::cleave::ChildSupervisionMode::Attached => "attached".to_string(),
                            crate::features::cleave::ChildSupervisionMode::RecoveredDegraded => "recovered_degraded".to_string(),
                            crate::features::cleave::ChildSupervisionMode::Lost => "lost".to_string(),
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
    if let Ok(mut startup) = state.startup_info.lock()
        && let Some(startup) = startup.as_mut()
    {
        startup.daemon_status = daemon_status;
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
            .map(|text| WebCommand::UserPrompt(text.to_string())),
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
            let _ = state.events_tx.send(omegon_traits::AgentEvent::SystemNotification {
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

/// Bind to a port with auto-increment fallback. Returns the listener directly
/// to avoid TOCTOU races.
async fn bind_strict(port: u16) -> anyhow::Result<tokio::net::TcpListener> {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind strict control port {port}: {e}"))
}

async fn bind_with_fallback(preferred: u16) -> anyhow::Result<tokio::net::TcpListener> {
    for offset in 0..10 {
        let port = preferred + offset;
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
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
fn generate_token() -> String {
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
            token: state.web_auth.issue_query_token(),
            auth_mode: state.web_auth.mode_name().into(),
            auth_source: state.web_auth.source_name().into(),
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
        assert!(startup.daemon_status.transport_warnings.iter().any(|warning| warning.contains("insecure bootstrap")));
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

    #[tokio::test]
    async fn bind_strict_fails_when_port_is_taken() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();

        let err = bind_strict(port).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to bind strict control port")
        );
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
            token: "test".into(),
            auth_mode: "ephemeral-bearer".into(),
            auth_source: "generated".into(),
            control_plane_state: ControlPlaneState::Ready,
            daemon_status: WebDaemonStatus::default(),
            instance_descriptor: None,
        };
        let state = WebState {
            command_tx,
            startup_info: Arc::new(Mutex::new(Some(startup))),
            ..state
        };
        state.daemon_events.lock().unwrap().push(DaemonEventEnvelope {
            event_id: "evt-1".into(),
            source: "manual/test".into(),
            trigger_kind: "prompt".into(),
            payload: serde_json::json!({"text": "hello from queue"}),
            caller_role: Some("admin".into()),
        });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::UserPrompt(text) => assert_eq!(text, "hello from queue"),
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

    #[tokio::test]
    async fn daemon_event_worker_dispatches_new_session_trigger() {
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(DashboardHandles::default(), events_tx);
        let state = WebState {
            command_tx,
            ..state
        };
        state.daemon_events.lock().unwrap().push(DaemonEventEnvelope {
            event_id: "evt-new-session".into(),
            source: "manual/test".into(),
            trigger_kind: "new-session".into(),
            payload: serde_json::json!({}),
            caller_role: Some("admin".into()),
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
        state.daemon_events.lock().unwrap().push(DaemonEventEnvelope {
            event_id: "evt-shutdown".into(),
            source: "manual/test".into(),
            trigger_kind: "shutdown".into(),
            payload: serde_json::json!({}),
            caller_role: Some("admin".into()),
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
        state.daemon_events.lock().unwrap().push(DaemonEventEnvelope {
            event_id: "evt-cancel-child".into(),
            source: "manual/test".into(),
            trigger_kind: "cancel-cleave-child".into(),
            payload: serde_json::json!({"label": "alpha"}),
            caller_role: Some("admin".into()),
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
                cleave: Some(Arc::new(Mutex::new(crate::features::cleave::CleaveProgress {
                    active: true,
                    run_id: "run-1".into(),
                    total_children: 1,
                    completed: 0,
                    failed: 0,
                    children: vec![crate::features::cleave::ChildProgress {
                        label: "child-1".into(),
                        status: "running".into(),
                        supervision_mode: Some(crate::features::cleave::ChildSupervisionMode::RecoveredDegraded),
                        duration_secs: None,
                        pid: Some(4242),
                        last_tool: None,
                        last_turn: None,
                        started_at: None,
                        last_activity_at: None,
                        tokens_in: 0,
                        tokens_out: 0,
                        runtime: Some(crate::features::cleave::ChildRuntimeSummary {
                            model: Some("anthropic:claude-sonnet-4-6".into()),
                            thinking_level: Some("high".into()),
                            context_class: Some("legion".into()),
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
                }))),
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
            token: "test".into(),
            auth_mode: "ephemeral-bearer".into(),
            auth_source: "generated".into(),
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
        state.daemon_events.lock().unwrap().push(DaemonEventEnvelope {
            event_id: "evt-rt-1".into(),
            source: "manual/test".into(),
            trigger_kind: "prompt".into(),
            payload: serde_json::json!({"text": "runtime check"}),
            caller_role: Some("admin".into()),
        });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let command = command_rx.recv().await.unwrap();
        match command {
            WebCommand::UserPrompt(text) => assert_eq!(text, "runtime check"),
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
        assert_eq!(child.supervision_mode.as_deref(), Some("recovered_degraded"));
        assert_eq!(child.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(child.thinking_level.as_deref(), Some("high"));
        assert_eq!(child.context_class.as_deref(), Some("legion"));
        assert_eq!(child.disabled_tools, vec!["bash"]);
        assert_eq!(child.enabled_extensions, vec!["alpha"]);
        assert_eq!(child.preloaded_files, vec!["docs/runtime-preload.md"]);
    }

    #[tokio::test]
    async fn daemon_event_worker_marks_unsupported_trigger_as_degraded() {
        let (events_tx, mut events_rx) = tokio::sync::broadcast::channel(4);
        let state = WebState::new(DashboardHandles::default(), events_tx);
        state.daemon_events.lock().unwrap().push(DaemonEventEnvelope {
            event_id: "evt-2".into(),
            source: "manual/test".into(),
            trigger_kind: "mystery".into(),
            payload: serde_json::json!({"ignored": true}),
            caller_role: Some("admin".into()),
        });
        state.daemon_status.lock().unwrap().queued_events = 1;

        let processed = process_next_daemon_event(&state).await.unwrap();
        assert!(processed);
        let status = state.daemon_status.lock().unwrap().clone();
        assert_eq!(status.queued_events, 0);
        assert!(status.transport_warnings.iter().any(|warning| warning.contains("Unsupported daemon event trigger 'mystery'")));
        let event = events_rx.recv().await.unwrap();
        match event {
            omegon_traits::AgentEvent::SystemNotification { message } => {
                assert!(message.contains("degraded"), "got: {message}");
                assert!(message.contains("mystery"), "got: {message}");
            }
            other => panic!("wrong event: {other:?}"),
        }
    }
}

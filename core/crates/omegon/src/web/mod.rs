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
pub use auth::{
    WEB_AUTH_SECRET_NAME, WebAuthSource, WebAuthState,
};
use omegon_traits::{
    IpcHealthSnapshot, IpcHealthState, IpcHarnessSnapshot, IpcMemorySnapshot,
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
        memory_ok: harness_projection.memory_available || harness_projection.memory_warning.is_none(),
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
        busy: false,
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
        }
    }
}

/// Commands received from WebSocket clients, forwarded to the main loop.
#[derive(Debug, Clone)]
pub enum WebCommand {
    UserPrompt(String),
    SlashCommand { name: String, args: String },
    Cancel,
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
    let app_state_handles = state.handles.clone();

    let app = Router::new()
        .route("/api/state", axum::routing::get(api::get_state))
        .route("/api/startup", axum::routing::get(api::get_startup))
        .route("/api/healthz", axum::routing::get(api::get_health))
        .route("/api/readyz", axum::routing::get(api::get_ready))
        .route("/api/graph", axum::routing::get(api::get_graph))
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
        .with_state(state);

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
        "web dashboard at {}/?token={}"
        ,startup.http_base, startup.token
    );

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("web server error: {e}");
        }
    });

    Ok((startup, cmd_rx))
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
        let state = WebState::new(DashboardHandles::default(), tokio::sync::broadcast::channel(16).0);
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
    }

    #[tokio::test]
    async fn bind_strict_fails_when_port_is_taken() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let err = bind_strict(port).await.unwrap_err();
        assert!(err.to_string().contains("Failed to bind strict control port"));
    }

    #[test]
    fn generate_token_is_unique() {
        let t1 = generate_token();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = generate_token();
        // Not guaranteed unique from timestamps alone, but in practice different
        assert_ne!(t1, t2);
    }
}

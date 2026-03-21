//! Embedded web server — localhost HTTP + WebSocket for the agent dashboard.
//!
//! Started on demand by `/dash open`. Serves:
//! - `GET /` — embedded single-page dashboard
//! - `GET /api/state` — full agent state snapshot (JSON)
//! - `WS /ws` — bidirectional agent protocol (JSON-over-WebSocket)
//!
//! The WebSocket protocol is the **full agent interface** — any web UI can
//! connect and drive the agent as a black box.

pub mod api;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::sync::{broadcast, mpsc};

use crate::tui::dashboard::DashboardHandles;

/// Shared state accessible to all web handlers.
#[derive(Clone)]
pub struct WebState {
    /// Dashboard data handles (same Arc<Mutex<>> the TUI reads).
    pub handles: DashboardHandles,
    /// Broadcast channel for AgentEvents → WebSocket push.
    pub events_tx: broadcast::Sender<omegon_traits::AgentEvent>,
    /// Channel for WebSocket commands → main loop.
    pub command_tx: mpsc::Sender<WebCommand>,
    /// Auth token — required for WebSocket connections.
    pub auth_token: Arc<String>,
}

impl WebState {
    /// Create a new WebState. Generates a random auth token.
    pub fn new(
        handles: DashboardHandles,
        events_tx: broadcast::Sender<omegon_traits::AgentEvent>,
    ) -> Self {
        let token = generate_token();
        let (command_tx, _) = mpsc::channel(32); // receiver returned by start_server
        Self {
            handles,
            events_tx,
            command_tx,
            auth_token: Arc::new(token),
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
    mut state: WebState,
    preferred_port: u16,
) -> anyhow::Result<(SocketAddr, mpsc::Receiver<WebCommand>)> {
    // Create the command channel — caller gets the receiver
    let (cmd_tx, cmd_rx) = mpsc::channel(32);
    state.command_tx = cmd_tx;

    let token_for_query = state.auth_token.clone();

    let app = Router::new()
        .route("/api/state", axum::routing::get(api::get_state))
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
    let listener = bind_with_fallback(preferred_port).await?;
    let bound = listener.local_addr()?;

    let token = token_for_query.to_string();
    tracing::debug!(port = bound.port(), "web dashboard at http://{bound}/?token={token}");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("web server error: {e}");
        }
    });

    Ok((bound, cmd_rx))
}

/// Serve the embedded dashboard HTML.
async fn serve_dashboard() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("assets/dashboard.html"))
}

/// Bind to a port with auto-increment fallback. Returns the listener directly
/// to avoid TOCTOU races.
async fn bind_with_fallback(preferred: u16) -> anyhow::Result<tokio::net::TcpListener> {
    for offset in 0..10 {
        let port = preferred + offset;
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(_) => continue,
        }
    }
    anyhow::bail!("No available port found in range {preferred}-{}", preferred + 9)
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
    fn generate_token_is_unique() {
        let t1 = generate_token();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = generate_token();
        // Not guaranteed unique from timestamps alone, but in practice different
        assert_ne!(t1, t2);
    }
}

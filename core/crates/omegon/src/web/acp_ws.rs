//! WebSocket ACP transport — enables network-accessible agent sessions.
//!
//! Each WebSocket connection to `/acp` gets its own `OmegonAcpAgent` running
//! on a dedicated OS thread with a `LocalSet` (same isolation pattern as
//! `acp_worker::spawn_worker`). The WebSocket frames are bridged to the
//! ACP thread via `mpsc` channels.
//!
//! This enables:
//! - Remote agent access (k8s pods, cloud servers)
//! - Multiple concurrent editor connections
//! - The same ACP protocol as stdin/stdout mode

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// State for the ACP WebSocket endpoint.
#[derive(Clone)]
pub struct AcpWebState {
    pub web_auth: Arc<super::auth::WebAuthState>,
    pub model: String,
    pub cwd: PathBuf,
    pub agent_id: Option<String>,
    pub connection_counter: Arc<AtomicU64>,
    pub shutdown: CancellationToken,
}

#[derive(Deserialize)]
pub struct AcpQuery {
    token: Option<String>,
}

/// Axum handler — authenticates, upgrades to WebSocket, spawns ACP session.
pub async fn acp_ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<AcpQuery>,
    State(state): State<AcpWebState>,
) -> impl IntoResponse {
    if !state.web_auth.verify_query_token(query.token.as_deref()) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| handle_acp_socket(socket, state))
        .into_response()
}

async fn handle_acp_socket(socket: WebSocket, state: AcpWebState) {
    let conn_id = state.connection_counter.fetch_add(1, Ordering::Relaxed);
    tracing::info!(conn_id, "ACP WebSocket client connected");

    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channels bridging the axum runtime ↔ ACP thread.
    // Inbound: WebSocket frames → ACP agent
    // Outbound: ACP agent → WebSocket frames
    let (inbound_tx, inbound_rx) = mpsc::channel::<String>(64);
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<String>(64);

    let shutdown = state.shutdown.clone();

    // Pump: read WS text frames → inbound channel
    let recv_shutdown = shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                frame = ws_stream.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            if inbound_tx.send(text.to_string()).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            tracing::debug!(error = %e, "ACP WS recv error");
                            break;
                        }
                        _ => continue, // skip binary/ping/pong
                    }
                }
                _ = recv_shutdown.cancelled() => break,
            }
        }
        drop(inbound_tx);
    });

    // Pump: outbound channel → WS text frames
    let send_shutdown = shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = outbound_rx.recv() => {
                    match msg {
                        Some(text) => {
                            if ws_sink.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = send_shutdown.cancelled() => {
                    let _ = ws_sink.send(Message::Close(None)).await;
                    break;
                }
            }
        }
    });

    // Spawn the ACP agent on a dedicated thread
    let model = state.model.clone();
    let cwd = state.cwd.clone();
    let agent_id = state.agent_id.clone();

    let handle = std::thread::Builder::new()
        .name(format!("acp-ws-{conn_id}"))
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("ACP WS runtime");

            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, run_acp_session(model, cwd, agent_id, inbound_rx, outbound_tx));
        });

    match handle {
        Ok(h) => {
            // Wait for the thread to finish (connection closed or shutdown)
            let _ = tokio::task::spawn_blocking(move || { let _ = h.join(); }).await;
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to spawn ACP WS thread");
        }
    }

    tracing::info!(conn_id, "ACP WebSocket client disconnected");
}

/// Run an ACP session on the dedicated thread's LocalSet.
async fn run_acp_session(
    model: String,
    cwd: PathBuf,
    agent_id: Option<String>,
    mut inbound_rx: mpsc::Receiver<String>,
    outbound_tx: mpsc::Sender<String>,
) {
    use std::rc::Rc;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    // Apply agent manifest if specified
    if let Some(ref id) = agent_id {
        let shared_settings = crate::settings::shared(&model);
        if let Err(e) = crate::apply_agent_manifest_pre_setup(id, &cwd, &shared_settings) {
            tracing::error!(error = %e, "failed to apply agent manifest");
        }
    }

    let agent = Rc::new(crate::acp::OmegonAcpAgent::new(&model));

    // Create DuplexStream pair for the ACP connection.
    // Read side: inbound_rx → DuplexStream → AgentSideConnection
    // Write side: AgentSideConnection → DuplexStream → outbound_tx
    let (read_client, mut read_server) = tokio::io::duplex(64 * 1024);
    let (write_client, write_server) = tokio::io::duplex(64 * 1024);

    // Pump: inbound channel → DuplexStream (read side for ACP)
    tokio::task::spawn_local(async move {
        while let Some(msg) = inbound_rx.recv().await {
            let mut line = msg;
            if !line.ends_with('\n') {
                line.push('\n');
            }
            if read_server.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
        drop(read_server);
    });

    // Pump: DuplexStream (write side from ACP) → outbound channel
    tokio::task::spawn_local(async move {
        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(write_client);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end().to_string();
                    if !trimmed.is_empty() && outbound_tx.send(trimmed).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Create the ACP connection using the DuplexStream halves
    let agent_clone = agent.clone();
    let (conn, io_task) = agent_client_protocol::AgentSideConnection::new(
        agent_clone,
        write_server.compat_write(),
        read_client.compat(),
        |fut| { tokio::task::spawn_local(fut); },
    );
    agent.set_client(conn);

    // Run until the connection closes
    if let Err(e) = io_task.await {
        tracing::debug!(error = %e, "ACP WS session ended");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_web_state_is_clone() {
        // Ensure AcpWebState can be cloned (required by axum State extractor)
        fn assert_clone<T: Clone>() {}
        assert_clone::<AcpWebState>();
    }
}

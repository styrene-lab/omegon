//! WebSocket ACP transport — enables network-accessible agent sessions.
//!
//! Each WebSocket connection to `/acp` gets its own `OmegonAcpAgent` running
//! on a dedicated OS thread with a `LocalSet` (same isolation pattern as
//! `acp_worker::spawn_worker`). The WebSocket frames are bridged to the
//! ACP thread via unbounded `mpsc` channels.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const MAX_WS_MESSAGE_BYTES: usize = 2 * 1024 * 1024; // 2MB per frame
const MAX_CONCURRENT_CONNECTIONS: u64 = 64;
const DUPLEX_BUFFER_BYTES: usize = 1024 * 1024; // 1MB
const THREAD_JOIN_TIMEOUT_SECS: u64 = 30;

/// State for the ACP WebSocket endpoint.
#[derive(Clone)]
pub struct AcpWebState {
    pub web_auth: Arc<super::auth::WebAuthState>,
    pub web_authority: super::WebAuthorityConfig,
    pub model: String,
    pub cwd: PathBuf,
    pub agent_id: Option<String>,
    pub dangerously_bypass_permissions: bool,
    pub active_connections: Arc<AtomicU64>,
    pub shutdown: CancellationToken,
}

#[derive(Deserialize)]
pub struct AcpQuery {
    token: Option<String>,
}

/// Axum handler — authenticates, enforces connection limit, upgrades to WebSocket.
pub async fn acp_ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Query(query): Query<AcpQuery>,
    State(state): State<AcpWebState>,
) -> impl IntoResponse {
    if !state.web_auth.verify_query_token(query.token.as_deref()) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(error) = super::rbac::validate_proxy_identity_headers_for_config(
        &state.web_authority,
        &headers,
    ) {
        return error.status().into_response();
    }

    let active = state.active_connections.load(Ordering::Relaxed);
    if active >= MAX_CONCURRENT_CONNECTIONS {
        tracing::warn!(
            active,
            max = MAX_CONCURRENT_CONNECTIONS,
            "ACP connection limit reached"
        );
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    ws.max_message_size(MAX_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_acp_socket(socket, state))
        .into_response()
}

async fn handle_acp_socket(socket: WebSocket, state: AcpWebState) {
    let active = state.active_connections.fetch_add(1, Ordering::Relaxed) + 1;
    let conn_id = active;
    tracing::info!(conn_id, active, "ACP WebSocket client connected");

    // Guard: decrement active connections on drop (disconnect, panic, any exit path)
    struct ConnGuard(Arc<AtomicU64>);
    impl Drop for ConnGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }
    let _conn_guard = ConnGuard(state.active_connections.clone());

    let (mut ws_sink, mut ws_stream) = socket.split();

    // Unbounded channels to prevent deadlock from slow consumers.
    // Backpressure is handled by the WebSocket itself (TCP flow control)
    // and the max_message_size limit above.
    let (inbound_tx, inbound_rx) = mpsc::unbounded_channel::<String>();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<String>();

    let shutdown = state.shutdown.clone();

    // Pump: read WS text frames → inbound channel
    let recv_shutdown = shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                frame = ws_stream.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            if inbound_tx.send(text.to_string()).is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            tracing::debug!(error = %e, "ACP WS recv error");
                            break;
                        }
                        _ => continue,
                    }
                }
                _ = recv_shutdown.cancelled() => break,
            }
        }
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

    let model = state.model.clone();
    let cwd = state.cwd.clone();
    let agent_id = state.agent_id.clone();

    let handle = std::thread::Builder::new()
        .name(format!("acp-ws-{conn_id}"))
        .spawn(move || {
            // Catch panics so the handler thread doesn't silently die
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::error!(error = %e, "failed to create ACP WS runtime");
                        return;
                    }
                };
                let local = tokio::task::LocalSet::new();
                local.block_on(
                    &rt,
                    run_acp_session(
                        model,
                        cwd,
                        agent_id,
                        state.dangerously_bypass_permissions,
                        inbound_rx,
                        outbound_tx,
                    ),
                );
            }));
            if let Err(e) = result {
                let msg = e
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                tracing::error!(error = msg, "ACP WS thread panicked");
            }
        });

    match handle {
        Ok(h) => {
            let join_result = tokio::task::spawn_blocking(move || h.join());
            match tokio::time::timeout(
                std::time::Duration::from_secs(THREAD_JOIN_TIMEOUT_SECS),
                join_result,
            )
            .await
            {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(_panic))) => {
                    tracing::error!(conn_id, "ACP WS thread panicked during join");
                }
                Ok(Err(e)) => {
                    tracing::error!(conn_id, error = %e, "ACP WS join task failed");
                }
                Err(_) => {
                    tracing::error!(
                        conn_id,
                        "ACP WS thread join timed out after {THREAD_JOIN_TIMEOUT_SECS}s"
                    );
                }
            }
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
    dangerously_bypass_permissions: bool,
    mut inbound_rx: mpsc::UnboundedReceiver<String>,
    outbound_tx: mpsc::UnboundedSender<String>,
) {
    use std::rc::Rc;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    if let Some(ref id) = agent_id {
        let shared_settings = crate::settings::shared(&model);
        if let Err(e) = crate::apply_agent_manifest_pre_setup(id, &cwd, &shared_settings) {
            tracing::error!(error = %e, "failed to apply agent manifest");
        }
    }

    let agent = Rc::new(crate::acp::OmegonAcpAgent::new_with_safety(
        &model,
        dangerously_bypass_permissions,
    ));

    let (read_client, mut read_server) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let (write_client, write_server) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);

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
                    if !trimmed.is_empty() && outbound_tx.send(trimmed).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let io_task = crate::acp::connect_acp_agent(
        agent.clone(),
        write_server.compat_write(),
        read_client.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );

    if let Err(e) = io_task.await {
        tracing::debug!(error = %e, "ACP WS session ended");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_web_state_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<AcpWebState>();
    }

    #[test]
    fn max_message_size_is_reasonable() {
        const {
            assert!(MAX_WS_MESSAGE_BYTES >= 1024 * 1024); // at least 1MB
            assert!(MAX_WS_MESSAGE_BYTES <= 16 * 1024 * 1024); // at most 16MB
        }
    }

    #[test]
    fn duplex_buffer_larger_than_typical_message() {
        const {
            assert!(DUPLEX_BUFFER_BYTES >= 256 * 1024);
        }
    }
}

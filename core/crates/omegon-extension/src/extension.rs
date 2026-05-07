//! Extension trait and serving infrastructure.
//!
//! v1: `ExtensionServe` — simple request/response loop (backward compat).
//! v2: `MessageRouter` — bidirectional communication with `HostProxy`.

use crate::rpc::{RpcIncoming, RpcMessage, RpcNotification, RpcRequest, RpcResponse};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, mpsc, oneshot};

// ─── Extension Trait ──────────────────────────────────────────────────────

/// Extension trait — implement this to create an extension.
///
/// The extension SDK will handle all RPC protocol details. You only need
/// to implement method dispatch and return JSON results.
#[async_trait]
pub trait Extension: Send + Sync {
    /// Extension name (must match manifest).
    fn name(&self) -> &str;

    /// Extension version (should match manifest).
    fn version(&self) -> &str;

    /// Handle an RPC method call.
    ///
    /// Return `Ok(Value)` on success, or an `Err(Error)` with a typed error code.
    /// Unknown methods should return `Error::method_not_found(method)`.
    ///
    /// # Safety
    ///
    /// - Parameter validation happens here. Return `Error::invalid_params()` if args don't make sense.
    /// - Panics in this method will crash the extension (not omegon).
    /// - Timeouts are enforced by the parent process — don't block indefinitely.
    async fn handle_rpc(&self, method: &str, params: Value) -> crate::Result<Value>;

    /// Handle a notification from the host. Default is no-op.
    ///
    /// Called for messages with no `id` (fire-and-forget). The return value
    /// is ignored — notifications never produce a response.
    async fn handle_notification(&self, _method: &str, _params: Value) {
        // Default: ignore notifications
    }

    /// Called after the initialize handshake completes. Default is no-op.
    ///
    /// The `host` proxy is available for sending notifications or requests
    /// back to the host. Store it if you need it during tool execution.
    async fn on_initialized(&self, _host: HostProxy) {
        // Default: no-op
    }

    /// Called when the host delivers configuration values declared in the
    /// manifest's `[config]` section. Extensions should store these for use
    /// during tool execution.
    ///
    /// Called after `on_initialized`, before any tool invocations. May be
    /// called again if the user updates config at runtime (hot-reload).
    async fn on_config(&self, _config: std::collections::HashMap<String, serde_json::Value>) {
        // Default: ignore config
    }
}

// ─── HostProxy ────────────────────────────────────────────────────────────

/// Proxy for sending messages from an extension back to the host.
///
/// Extensions receive this via `on_initialized()`. It can be cloned and
/// stored for later use during tool execution.
#[derive(Clone)]
pub struct HostProxy {
    writer_tx: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<RpcResponse>>>>,
    next_id: Arc<AtomicU64>,
}

impl HostProxy {
    pub(crate) fn new(
        writer_tx: mpsc::Sender<String>,
        pending: Arc<Mutex<HashMap<String, oneshot::Sender<RpcResponse>>>>,
    ) -> Self {
        Self {
            writer_tx,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Send a notification to the host (fire-and-forget, no response).
    pub async fn notify(&self, method: &str, params: Value) -> crate::Result<()> {
        let notif = RpcNotification::new(method, params);
        let json = serde_json::to_string(&notif)?;
        self.writer_tx
            .send(json)
            .await
            .map_err(|_| crate::Error::internal_error("host connection closed"))?;
        Ok(())
    }

    /// Send a request to the host and await the response.
    ///
    /// Used for sampling, elicitation, and other ext→host calls.
    ///
    /// Includes a 30-second timeout to prevent indefinite hangs.
    pub async fn request(&self, method: &str, params: Value) -> crate::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let id_str = format!("ext-{}", id);

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id_str.clone(), tx);

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id_str,
            "method": method,
            "params": params,
        });
        let json = serde_json::to_string(&request)?;
        self.writer_tx
            .send(json)
            .await
            .map_err(|_| crate::Error::internal_error("host connection closed"))?;

        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(response)) => response.into_result(),
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id_str);
                Err(crate::Error::internal_error(format!(
                    "host dropped response channel for {method}"
                )))
            }
            Err(_) => {
                self.pending.lock().await.remove(&id_str);
                Err(crate::Error::new(
                    crate::ErrorCode::Timeout,
                    format!("ext→host request '{method}' timed out after 30s"),
                ))
            }
        }
    }
}

// ─── v2 Message Router ───────────────────────────────────────────────────

/// Bidirectional message router for v2 extensions.
///
/// Spawns two tasks:
/// - Reader: parses incoming messages, routes responses to pending table,
///   dispatches requests/notifications to the Extension trait
/// - Writer: serializes outgoing messages from the mpsc channel to stdout
pub(crate) struct MessageRouter<E: Extension> {
    ext: Arc<E>,
}

impl<E: Extension + 'static> MessageRouter<E> {
    pub(crate) fn new(ext: E) -> Self {
        Self { ext: Arc::new(ext) }
    }

    /// Run the bidirectional message router until EOF.
    pub(crate) async fn run(self) -> crate::Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let reader = tokio::io::BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);

        // Channel for outgoing messages (responses + ext-initiated notifications/requests).
        let (writer_tx, mut writer_rx) = mpsc::channel::<String>(256);

        // Pending response table for ext→host requests.
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<RpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Host proxy for the extension to use.
        let host = HostProxy::new(writer_tx.clone(), pending.clone());

        // Notify the extension that it can now send messages.
        // This happens asynchronously — don't block the router startup.
        let ext_init = self.ext.clone();
        let host_init = host.clone();
        tokio::spawn(async move {
            ext_init.on_initialized(host_init).await;
        });

        // Writer task: drain outgoing messages to stdout.
        let writer_handle = tokio::spawn(async move {
            while let Some(msg) = writer_rx.recv().await {
                if writer.write_all(msg.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        // Reader loop: process incoming messages on the main task.
        self.read_loop(reader, writer_tx.clone(), pending).await?;

        // Shut down writer gracefully — drop the sender so the writer task
        // drains remaining messages and exits naturally.
        drop(writer_tx);
        // Give the writer a moment to flush, then abort if it's stuck.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), writer_handle).await;
        Ok(())
    }

    async fn read_loop(
        &self,
        mut reader: tokio::io::BufReader<tokio::io::Stdin>,
        writer_tx: mpsc::Sender<String>,
        pending: Arc<Mutex<HashMap<String, oneshot::Sender<RpcResponse>>>>,
    ) -> crate::Result<()> {
        let mut buf = Vec::with_capacity(4096);

        loop {
            buf.clear();
            let line = match read_bounded_line(&mut reader, &mut buf).await {
                Ok(Some(line)) => line,
                Ok(None) => return Ok(()), // EOF
                Err(msg) => {
                    let error_response =
                        RpcResponse::error(None, crate::ErrorCode::ParseError, msg);
                    let json = serde_json::to_string(&error_response)?;
                    let _ = writer_tx.send(json).await;
                    continue;
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse and route.
            match RpcIncoming::parse(trimmed) {
                Ok(RpcIncoming::Request(req)) => {
                    // Incoming request from host — dispatch and respond.
                    let ext = self.ext.clone();
                    let tx = writer_tx.clone();
                    tokio::spawn(async move {
                        let response = dispatch_request(&*ext, &req).await;
                        let json = serde_json::to_string(&response).unwrap_or_default();
                        let _ = tx.send(json).await;
                    });
                }
                Ok(RpcIncoming::Response(resp)) => {
                    // Response to one of our pending ext→host requests.
                    if let Some(id) = resp.id.as_ref().and_then(|v| v.as_str()) {
                        if let Some(tx) = pending.lock().await.remove(id) {
                            let _ = tx.send(resp);
                        }
                    }
                }
                Ok(RpcIncoming::Notification(notif)) => {
                    // Notification from host — dispatch, no response.
                    let ext = self.ext.clone();
                    tokio::spawn(async move {
                        ext.handle_notification(&notif.method, notif.params).await;
                    });
                }
                Err(e) => {
                    // Parse error — send error response.
                    let error_response =
                        RpcResponse::error(None, crate::ErrorCode::ParseError, e.to_string());
                    let json = serde_json::to_string(&error_response)?;
                    let _ = writer_tx.send(json).await;
                }
            }
        }
    }
}

// ─── Bounded line reader ──────────────────────────────────────────────

/// 16 MiB cap to prevent OOM from unbounded reads.
const MAX_LINE_SIZE: usize = 16 * 1024 * 1024;

/// Read a single newline-terminated line with a bounded size limit.
///
/// Returns:
/// - `Ok(Some(line))` — a valid line within the size limit
/// - `Ok(None)` — EOF (pipe closed)
/// - `Err(message)` — line exceeded MAX_LINE_SIZE (discarded from pipe)
///
/// Uses `fill_buf()` + `consume()` to check size *during* the read,
/// preventing OOM from malicious oversized messages. If the limit is
/// exceeded, the remainder of the line is consumed and discarded.
async fn read_bounded_line<'a, R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &'a mut Vec<u8>,
) -> Result<Option<&'a str>, String> {
    buf.clear();

    loop {
        let available = reader.fill_buf().await.map_err(|e| e.to_string())?;

        if available.is_empty() {
            // EOF
            return if buf.is_empty() {
                Ok(None)
            } else {
                // Partial line at EOF — process it.
                let line = std::str::from_utf8(buf).map_err(|e| format!("invalid UTF-8: {e}"))?;
                Ok(Some(line))
            };
        }

        // Find newline in available data.
        let (chunk, found_newline) = match available.iter().position(|&b| b == b'\n') {
            Some(pos) => (&available[..=pos], true),
            None => (available, false),
        };

        // Check size limit *before* copying.
        if buf.len() + chunk.len() > MAX_LINE_SIZE {
            // Oversized. Consume this chunk and drain the rest of the line.
            let chunk_len = chunk.len();
            reader.consume(chunk_len);
            if !found_newline {
                // Drain until newline or EOF.
                let mut drain = Vec::new();
                let _ = reader.read_until(b'\n', &mut drain).await;
            }
            return Err(format!(
                "message too large (>{} bytes, limit {})",
                MAX_LINE_SIZE, MAX_LINE_SIZE,
            ));
        }

        buf.extend_from_slice(chunk);
        let chunk_len = chunk.len();
        reader.consume(chunk_len);

        if found_newline {
            let line = std::str::from_utf8(buf).map_err(|e| format!("invalid UTF-8: {e}"))?;
            return Ok(Some(line));
        }
    }
}

/// Dispatch an incoming request to the extension's handle_rpc method.
async fn dispatch_request<E: Extension>(ext: &E, req: &RpcRequest) -> RpcResponse {
    let result = ext.handle_rpc(&req.method, req.params.clone()).await;

    match result {
        Ok(value) => RpcResponse::success(req.id.clone(), value),
        Err(e) => RpcResponse::error(req.id.clone(), e.code(), e.message()),
    }
}

// ─── v1 Extension Serve (backward compat) ────────────────────────────────

/// Extension serving loop (v1). Created by [`crate::serve()`], runs until shutdown.
///
/// This is the simple request/response loop without bidirectional support.
/// Use `serve_v2()` for new extensions that need to send notifications or
/// requests back to the host.
pub(crate) struct ExtensionServe<E: Extension> {
    ext: E,
}

impl<E: Extension> ExtensionServe<E> {
    pub(crate) fn new(ext: E) -> Self {
        Self { ext }
    }

    /// Run the extension serving loop.
    pub(crate) async fn run(self) -> crate::Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let mut reader = tokio::io::BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);

        let mut buf = Vec::with_capacity(4096);

        loop {
            buf.clear();
            let line = match read_bounded_line(&mut reader, &mut buf).await {
                Ok(Some(line)) => line,
                Ok(None) => return Ok(()), // EOF
                Err(msg) => {
                    let error_response =
                        RpcResponse::error(None, crate::ErrorCode::ParseError, msg);
                    let response_json = serde_json::to_string(&error_response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                    continue;
                }
            };

            // Parse incoming RPC message
            let msg: RpcMessage = match serde_json::from_str(line.trim()) {
                Ok(msg) => msg,
                Err(e) => {
                    let error_response =
                        RpcResponse::error(None, crate::ErrorCode::ParseError, e.to_string());
                    let response_json = serde_json::to_string(&error_response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                    continue;
                }
            };

            match msg {
                RpcMessage::Request(req) => {
                    let response = self.handle_request(&req).await;
                    let response_json = serde_json::to_string(&response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                }
                RpcMessage::Notification(_notif) => {
                    // Ignore notifications — v1 extension doesn't process them
                }
            }
        }
    }

    async fn handle_request(&self, req: &RpcRequest) -> RpcResponse {
        let result = self.ext.handle_rpc(&req.method, req.params.clone()).await;

        match result {
            Ok(value) => RpcResponse::success(req.id.clone(), value),
            Err(e) => RpcResponse::error(req.id.clone(), e.code(), e.message()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestExtension;

    #[async_trait]
    impl Extension for TestExtension {
        fn name(&self) -> &str {
            "test-extension"
        }

        fn version(&self) -> &str {
            "0.1.0"
        }

        async fn handle_rpc(&self, method: &str, _params: Value) -> crate::Result<Value> {
            match method {
                "echo" => Ok(serde_json::json!({"status": "ok"})),
                "get_tools" => Ok(serde_json::json!([])),
                _ => Err(crate::Error::method_not_found(method)),
            }
        }
    }

    #[tokio::test]
    async fn test_extension_dispatch() {
        let ext = TestExtension;

        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::String("1".to_string())),
            method: "echo".to_string(),
            params: serde_json::json!({}),
        };

        let response = dispatch_request(&ext, &req).await;

        assert_eq!(
            response.id,
            Some(serde_json::Value::String("1".to_string()))
        );
        assert!(response.result.is_some());
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let ext = TestExtension;

        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::String("1".to_string())),
            method: "unknown".to_string(),
            params: serde_json::json!({}),
        };

        let response = dispatch_request(&ext, &req).await;

        assert_eq!(
            response.id,
            Some(serde_json::Value::String("1".to_string()))
        );
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, -32601);
        assert_eq!(error.label, "MethodNotFound");
    }

    #[tokio::test]
    async fn test_host_proxy_notification() {
        let (tx, mut rx) = mpsc::channel(16);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let proxy = HostProxy::new(tx, pending);

        proxy
            .notify("notifications/tools/list_changed", serde_json::json!({}))
            .await
            .unwrap();

        let msg = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(parsed["method"], "notifications/tools/list_changed");
        assert!(parsed.get("id").is_none());
    }

    #[tokio::test]
    async fn test_host_proxy_request_response() {
        let (tx, mut rx) = mpsc::channel(16);
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<RpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let proxy = HostProxy::new(tx, pending.clone());

        // Spawn request in background.
        let proxy_clone = proxy.clone();
        let handle = tokio::spawn(async move {
            proxy_clone
                .request("sampling/create_message", serde_json::json!({"test": true}))
                .await
        });

        // Read the outgoing request.
        let msg = rx.recv().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&msg).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();
        assert!(id.starts_with("ext-"));
        assert_eq!(parsed["method"], "sampling/create_message");

        // Simulate host response.
        let response = RpcResponse::success(
            Some(Value::String(id.clone())),
            serde_json::json!({"role": "assistant", "content": "hello"}),
        );
        if let Some(tx) = pending.lock().await.remove(&id) {
            tx.send(response).unwrap();
        }

        // Check the extension got the result.
        let result = handle.await.unwrap().unwrap();
        assert_eq!(result["role"], "assistant");
    }

    // ─── Bounded read tests ──────────────────────────────────────────

    #[tokio::test]
    async fn test_bounded_read_normal_line() {
        let data = b"hello world\n";
        let mut reader = tokio::io::BufReader::new(&data[..]);
        let mut buf = Vec::new();

        let result = read_bounded_line(&mut reader, &mut buf).await;
        let line = result.unwrap().unwrap();
        assert_eq!(line.trim(), "hello world");
    }

    #[tokio::test]
    async fn test_bounded_read_eof() {
        let data = b"";
        let mut reader = tokio::io::BufReader::new(&data[..]);
        let mut buf = Vec::new();

        let result = read_bounded_line(&mut reader, &mut buf).await;
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_bounded_read_partial_line_at_eof() {
        let data = b"no newline";
        let mut reader = tokio::io::BufReader::new(&data[..]);
        let mut buf = Vec::new();

        let result = read_bounded_line(&mut reader, &mut buf).await;
        let line = result.unwrap().unwrap();
        assert_eq!(line, "no newline");
    }

    #[tokio::test]
    async fn test_bounded_read_multiple_lines() {
        let data = b"line one\nline two\n";
        let mut reader = tokio::io::BufReader::new(&data[..]);
        let mut buf = Vec::new();

        let line1 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line1.trim(), "line one");

        buf.clear();
        let line2 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line2.trim(), "line two");
    }

    #[tokio::test]
    async fn test_bounded_read_rejects_oversized() {
        // Create a line larger than MAX_LINE_SIZE
        let oversized = vec![b'x'; MAX_LINE_SIZE + 100];
        let mut data = oversized;
        data.push(b'\n');
        let mut reader = tokio::io::BufReader::new(&data[..]);
        let mut buf = Vec::new();

        let result = read_bounded_line(&mut reader, &mut buf).await;
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("too large"));
    }

    #[tokio::test]
    async fn test_bounded_read_exactly_at_limit() {
        // Line exactly at MAX_LINE_SIZE (including newline)
        let mut data = vec![b'x'; MAX_LINE_SIZE - 1];
        data.push(b'\n');
        let mut reader = tokio::io::BufReader::new(&data[..]);
        let mut buf = Vec::new();

        let result = read_bounded_line(&mut reader, &mut buf).await;
        assert!(result.is_ok());
        let line = result.unwrap().unwrap();
        assert_eq!(line.len(), MAX_LINE_SIZE);
    }

    #[tokio::test]
    async fn test_bounded_read_empty_lines() {
        let data = b"\n\nhello\n";
        let mut reader = tokio::io::BufReader::new(&data[..]);
        let mut buf = Vec::new();

        // First line is just "\n"
        let line1 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line1.trim(), "");

        buf.clear();
        let line2 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line2.trim(), "");

        buf.clear();
        let line3 = read_bounded_line(&mut reader, &mut buf)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(line3.trim(), "hello");
    }
}

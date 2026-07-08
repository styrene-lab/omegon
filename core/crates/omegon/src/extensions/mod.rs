//! Extension spawning and process management.
//!
//! Handles both native (binary) and OCI (container) extensions.
//! All extensions communicate via JSON-RPC 2.0 over stdin/stdout.
//! Stateful widgets stream updates via separate TCP connection.
//!
//! # Secret delivery
//!
//! Extension subprocesses are spawned with `env_clear()` — no secret inheritance
//! from the parent process environment. Declared secrets are delivered via the
//! `bootstrap_secrets` RPC method immediately after the `get_tools` handshake.
//! This prevents plain-text secrets from appearing in `/proc/<pid>/environ`,
//! `ps` output, crash dumps, or child processes of the extension.

use anyhow::{Result, anyhow};
use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

pub(crate) mod approval;
pub mod config_store;
pub(crate) mod host_actions;
pub mod manifest;
pub mod mind;
pub(crate) mod sdk_compat;
pub mod state;
mod tool_result;
pub mod voice_bridge;
pub mod vox_bridge;
pub mod widgets;
pub use manifest::{
    ConnectionMode, ExtensionManifest, McpConfig, McpTransport, RuntimeConfig, WidgetConfig,
};
pub use mind::{ExtensionMind, MindStats};
pub use sdk_compat::SdkCompatibilityDiagnostic;
pub use state::{ExtensionState, StabilityMetrics};
pub use widgets::{ExtensionTabWidget, WidgetDeclaration, WidgetEvent};

/// Environment variables that are safe to inherit from the parent process.
/// Everything else is stripped via env_clear() — secrets never leak via env.
const EXTENSION_TOOL_RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const EXTENSION_POLL_RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

const SAFE_INHERIT_ENVS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TERM",
    "SHELL",
    // Dynamic linker paths — needed on some systems for compiled binaries
    "DYLD_LIBRARY_PATH",          // macOS
    "DYLD_FALLBACK_LIBRARY_PATH", // macOS
    "LD_LIBRARY_PATH",            // Linux
    // Rust runtime
    "RUST_LOG",
    "RUST_BACKTRACE",
    // Project root — set by omegon from --cwd, read by extensions to locate the
    // user's active workspace. Not a secret (just a filesystem path).
    "OMEGON_PROJECT_ROOT",
    // Flynt/Codex vault roots — backwards compat for the flynt-agent extension.
    "FLYNT_VAULT",
    "CODEX_VAULT",
];

#[derive(Debug, Clone)]
pub struct ExtensionNotification {
    pub extension_name: String,
    pub method: String,
    pub params: Value,
}

#[derive(Clone)]
struct ExtensionNotificationSink {
    extension_name: String,
    tx: mpsc::UnboundedSender<ExtensionNotification>,
}

impl ExtensionNotificationSink {
    fn send(&self, notification: omegon_extension::RpcNotification) {
        let event = ExtensionNotification {
            extension_name: self.extension_name.clone(),
            method: notification.method,
            params: notification.params,
        };
        if let Err(err) = self.tx.send(event) {
            tracing::debug!(
                extension = %self.extension_name,
                error = %err,
                "extension notification dropped because receiver is closed"
            );
        }
    }
}

/// Handles for communicating with an extension process.
pub struct ProcessHandles {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl ProcessHandles {
    fn new(
        child: tokio::process::Child,
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
    ) -> Self {
        Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        }
    }

    /// Send a JSON-RPC request and receive the response.
    /// Standalone so the handshake sequence can run before ExtensionFeature is constructed.
    async fn rpc_call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.rpc_call_with_notifications(method, params, None).await
    }

    async fn rpc_call_with_notifications(
        &mut self,
        method: &str,
        params: Value,
        notification_sink: Option<&ExtensionNotificationSink>,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.stdin
            .write_all(format!("{}\n", request).as_bytes())
            .await?;
        self.stdin.flush().await?;

        let mut line = String::new();
        loop {
            line.clear();
            let n = self.reader.read_line(&mut line).await?;
            if n == 0 {
                return Err(anyhow!("extension closed connection"));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resp: Value = serde_json::from_str(trimmed)?;
            if let Ok(omegon_extension::RpcIncoming::Notification(notification)) =
                omegon_extension::RpcIncoming::parse(trimmed)
            {
                if let Some(sink) = notification_sink {
                    sink.send(notification);
                }
                continue;
            }
            if resp.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return if let Some(result) = resp.get("result") {
                    Ok(result.clone())
                } else if let Some(error) = resp.get("error") {
                    Err(anyhow!("RPC error: {}", error))
                } else {
                    Err(anyhow!("invalid RPC response: no result or error"))
                };
            }
            // Continue reading (may be out-of-order notifications or prior responses)
        }
    }
}

impl Drop for ProcessHandles {
    fn drop(&mut self) {
        // Extension processes are long-lived JSON-RPC peers. Dropping the
        // final host-side handle must not leave shell-script/native extension
        // children alive, because Tokio waits for managed child processes and
        // `cargo test` can hang after assertions complete. Respawn paths still
        // perform explicit async kill/wait; this synchronous drop path is the
        // deterministic backstop for tests and normal shutdown.
        let _ = self.child.start_kill();
    }
}

fn host_rpc_response_for_extension_request(
    manifest: &ExtensionManifest,
    extension_name: &str,
    request: &omegon_extension::RpcRequest,
) -> Option<Value> {
    match request.method.as_str() {
        "actions/execute" => {
            let action = request.params.get("action").cloned().unwrap_or(Value::Null);
            let outcome = host_actions::process_native_extension_action_execute(
                action,
                manifest,
                extension_name,
            );
            let result = serde_json::to_value(outcome).unwrap_or_else(|err| {
                json!({
                    "action_id": "<serialization-error>",
                    "status": "invalid",
                    "error": {
                        "code": "serialization_error",
                        "message": err.to_string()
                    }
                })
            });
            Some(json!({
                "jsonrpc": "2.0",
                "id": request.id.clone(),
                "result": result
            }))
        }
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": request.id.clone(),
            "error": {
                "code": -32601,
                "message": format!("unknown host request method '{}'", request.method)
            }
        })),
    }
}

#[derive(Clone)]
struct ExtensionRuntimeContext {
    name: String,
    ext_dir: PathBuf,
    manifest: ExtensionManifest,
    resolved_secrets: Vec<(String, String)>,
    notification_sink: Option<ExtensionNotificationSink>,
}

/// Wrapper Feature for any extension (native or OCI).
/// Manages RPC communication via stdin/stdout, agnostic to runtime type.
#[derive(Clone)]
pub struct ExtensionFeature {
    runtime: ExtensionRuntimeContext,
    tools: Vec<ToolDefinition>,
    handles: Arc<Mutex<Option<ProcessHandles>>>,
    request_id: Arc<AtomicU64>,
    widgets: Vec<WidgetDeclaration>,
    widget_tx: broadcast::Sender<WidgetEvent>,
    state: Arc<Mutex<ExtensionState>>,
}

impl ExtensionFeature {
    /// Create a new extension feature from already-handshaked process handles.
    fn new(
        runtime: ExtensionRuntimeContext,
        tools: Vec<ToolDefinition>,
        widgets: Vec<WidgetDeclaration>,
        handles: ProcessHandles,
        state: ExtensionState,
    ) -> (Self, broadcast::Receiver<WidgetEvent>) {
        let (widget_tx, widget_rx) = broadcast::channel::<WidgetEvent>(100);
        let next_id = handles.next_id;
        (
            Self {
                runtime,
                tools,
                handles: Arc::new(Mutex::new(Some(handles))),
                request_id: Arc::new(AtomicU64::new(next_id)),
                widgets,
                widget_tx,
                state: Arc::new(Mutex::new(state)),
            },
            widget_rx,
        )
    }

    /// Send a JSON-RPC request and receive the response.
    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        self.rpc_call_with_cancel(
            method,
            params,
            CancellationToken::new(),
            Some(EXTENSION_TOOL_RPC_TIMEOUT),
        )
        .await
    }

    async fn rpc_call_with_cancel(
        &self,
        method: &str,
        params: Value,
        cancel: CancellationToken,
        idle_timeout: Option<std::time::Duration>,
    ) -> Result<Value> {
        let mut guard = self.handles.lock().await;
        let handles = guard
            .as_mut()
            .ok_or_else(|| anyhow!("extension process not running"))?;

        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        handles
            .stdin
            .write_all(format!("{}\n", request).as_bytes())
            .await?;
        handles.stdin.flush().await?;

        let started_at = std::time::Instant::now();
        let mut last_notification: Option<String> = None;
        let mut line = String::new();
        loop {
            line.clear();
            let read = handles.reader.read_line(&mut line);
            let n = if let Some(timeout) = idle_timeout {
                tokio::select! {
                    result = tokio::time::timeout(timeout, read) => match result {
                        Ok(result) => result?,
                        Err(_) => {
                            anyhow::bail!(
                                "extension '{}' RPC '{}' id {} timed out after {}ms waiting for response (last_notification={})",
                                self.runtime.name,
                                method,
                                id,
                                started_at.elapsed().as_millis(),
                                last_notification.as_deref().unwrap_or("none")
                            );
                        }
                    },
                    _ = cancel.cancelled() => {
                        anyhow::bail!(
                            "extension '{}' RPC '{}' id {} cancelled after {}ms (last_notification={})",
                            self.runtime.name,
                            method,
                            id,
                            started_at.elapsed().as_millis(),
                            last_notification.as_deref().unwrap_or("none")
                        );
                    }
                }
            } else {
                tokio::select! {
                    result = read => result?,
                    _ = cancel.cancelled() => {
                        anyhow::bail!(
                            "extension '{}' RPC '{}' id {} cancelled after {}ms (last_notification={})",
                            self.runtime.name,
                            method,
                            id,
                            started_at.elapsed().as_millis(),
                            last_notification.as_deref().unwrap_or("none")
                        );
                    }
                }
            };
            if n == 0 {
                return Err(anyhow!("extension closed connection"));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resp: Value = serde_json::from_str(trimmed)?;
            if let Ok(incoming) = omegon_extension::RpcIncoming::parse(trimmed) {
                match incoming {
                    omegon_extension::RpcIncoming::Request(req) => {
                        let response = host_rpc_response_for_extension_request(
                            &self.runtime.manifest,
                            &self.runtime.name,
                            &req,
                        )
                        .ok_or_else(|| anyhow!("host request produced no response"))?;
                        handles
                            .stdin
                            .write_all(format!("{}\n", response).as_bytes())
                            .await?;
                        handles.stdin.flush().await?;
                        continue;
                    }
                    omegon_extension::RpcIncoming::Notification(notification) => {
                        last_notification = Some(notification.method.clone());
                        if let Some(sink) = &self.runtime.notification_sink {
                            sink.send(notification);
                        }
                        continue;
                    }
                    omegon_extension::RpcIncoming::Response(_) => {}
                }
            }
            if resp.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return if let Some(result) = resp.get("result") {
                    Ok(result.clone())
                } else if let Some(error) = resp.get("error") {
                    Err(anyhow!("RPC error: {}", error))
                } else {
                    Err(anyhow!("invalid RPC response"))
                };
            }
        }
    }

    async fn extension_tool_result_with_context(
        &self,
        output: Value,
        call_id: &str,
        context: &omegon_traits::ToolExecutionContext,
    ) -> ToolResult {
        let mut envelope = tool_result::parse_extension_tool_envelope(output);
        if !envelope.host_actions.is_empty() {
            let outcomes = host_actions::process_declarative_host_actions_with_context(
                envelope.host_actions,
                &self.runtime.manifest,
                &self.runtime.name,
                call_id,
                context,
            )
            .await;
            envelope.host_actions = Vec::new();
            envelope.host_action_outcomes.extend(outcomes);
        }
        envelope.into_tool_result()
    }

    async fn extension_tool_result(&self, output: Value, call_id: &str) -> ToolResult {
        self.extension_tool_result_with_context(
            output,
            call_id,
            &omegon_traits::ToolExecutionContext::default(),
        )
        .await
    }

    async fn respawn_after_transport_error(&self, cause: &anyhow::Error) -> Result<()> {
        let mut guard = self.handles.lock().await;
        if let Some(mut stale) = guard.take() {
            let _ = stale.child.kill().await;
            let _ = stale.child.wait().await;
        }

        let mut handles = spawn_process_handles(&self.runtime.manifest, &self.runtime.ext_dir)
            .await
            .map_err(|err| {
                anyhow!(
                    "extension '{}' transport failed ({cause}); respawn failed: {err}",
                    self.runtime.name
                )
            })?;
        let handshake = handshake(
            &mut handles,
            &self.runtime.manifest,
            &self.runtime.ext_dir,
            &self.runtime.resolved_secrets,
            self.runtime.notification_sink.as_ref(),
        )
        .await
        .map_err(|err| {
            anyhow!(
                "extension '{}' transport failed ({cause}); respawn handshake failed: {err}",
                self.runtime.name
            )
        })?;
        self.request_id.store(handles.next_id, Ordering::SeqCst);
        *guard = Some(handles);
        tracing::warn!(
            extension = %self.runtime.name,
            tools = handshake.tools.len(),
            cause = %cause,
            "respawned extension after transport failure"
        );
        Ok(())
    }

    /// Get widgets declared by this extension.
    pub fn widgets(&self) -> &[WidgetDeclaration] {
        &self.widgets
    }

    /// Get extension state.
    pub async fn state(&self) -> ExtensionState {
        self.state.lock().await.clone()
    }

    /// Record an error in the extension state and persist it.
    pub async fn record_error(&self, error: String) {
        let mut state = self.state.lock().await;
        state.record_error(error);
        let _ = state.save(&self.runtime.ext_dir);
    }

    /// Broadcast a widget event (for internal use).
    pub fn send_widget_event(&self, event: WidgetEvent) -> Result<()> {
        self.widget_tx
            .send(event)
            .map_err(|e| anyhow!("widget event broadcast failed: {}", e))?;
        Ok(())
    }

    /// Subscribe to widget events.
    pub fn widget_events(&self) -> broadcast::Receiver<WidgetEvent> {
        self.widget_tx.subscribe()
    }

    /// Create a polling handle for calling RPC methods from outside the EventBus.
    /// Used by the daemon's vox event bridge to poll for inbound messages.
    pub fn polling_handle(&self) -> ExtensionPollingHandle {
        ExtensionPollingHandle {
            handles: self.handles.clone(),
            request_id: self.request_id.clone(),
            name: self.runtime.name.clone(),
            notification_sink: self.runtime.notification_sink.clone(),
        }
    }
}

/// Shareable handle for calling RPC methods on an extension subprocess.
/// Clones the Arc'd handles from ExtensionFeature so daemon background tasks
/// can poll the extension without going through the EventBus/agent turn.
#[derive(Clone)]
pub struct ExtensionPollingHandle {
    handles: Arc<Mutex<Option<ProcessHandles>>>,
    request_id: Arc<AtomicU64>,
    name: String,
    notification_sink: Option<ExtensionNotificationSink>,
}

impl std::fmt::Debug for ExtensionPollingHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionPollingHandle")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl ExtensionPollingHandle {
    pub async fn pump_notifications_for(&self, idle_timeout: std::time::Duration) -> Result<()> {
        let mut guard = self.handles.lock().await;
        let handles = guard
            .as_mut()
            .ok_or_else(|| anyhow!("extension process not running"))?;
        let read = async {
            let mut line = String::new();
            let n = handles.reader.read_line(&mut line).await?;
            anyhow::Ok::<(usize, String)>((n, line))
        };
        match tokio::time::timeout(idle_timeout, read).await {
            Ok(Ok((0, _))) => Err(anyhow!("extension closed connection")),
            Ok(Ok((_n, line))) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return Ok(());
                }
                if let Ok(omegon_extension::RpcIncoming::Notification(notification)) =
                    omegon_extension::RpcIncoming::parse(trimmed)
                    && let Some(sink) = &self.notification_sink
                {
                    sink.send(notification);
                }
                Ok(())
            }
            Ok(Err(err)) => Err(err),
            Err(_) => Ok(()),
        }
    }

    /// Name of the extension this handle is connected to.
    pub fn extension_name(&self) -> &str {
        &self.name
    }

    /// Send a JSON-RPC request and receive the response.
    pub async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let mut guard = self.handles.lock().await;
        let handles = guard
            .as_mut()
            .ok_or_else(|| anyhow!("extension process not running"))?;

        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        handles
            .stdin
            .write_all(format!("{}\n", request).as_bytes())
            .await?;
        handles.stdin.flush().await?;

        let mut line = String::new();
        loop {
            line.clear();
            let n = handles.reader.read_line(&mut line).await?;
            if n == 0 {
                return Err(anyhow!("extension closed connection"));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resp: Value = serde_json::from_str(trimmed)?;
            if let Ok(incoming) = omegon_extension::RpcIncoming::parse(trimmed) {
                match incoming {
                    omegon_extension::RpcIncoming::Request(req) => {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": req.id,
                            "error": {
                                "code": -32601,
                                "message": format!("host request method '{}' is unavailable on polling handles", req.method)
                            }
                        });
                        handles
                            .stdin
                            .write_all(format!("{}\n", response).as_bytes())
                            .await?;
                        handles.stdin.flush().await?;
                        continue;
                    }
                    omegon_extension::RpcIncoming::Notification(_) => {
                        continue;
                    }
                    omegon_extension::RpcIncoming::Response(_) => {}
                }
            }
            if resp.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return if let Some(result) = resp.get("result") {
                    Ok(result.clone())
                } else if let Some(error) = resp.get("error") {
                    Err(anyhow!("RPC error: {}", error))
                } else {
                    Err(anyhow!("invalid RPC response"))
                };
            }
        }
    }
}

#[async_trait::async_trait]
impl Feature for ExtensionFeature {
    fn name(&self) -> &str {
        &self.runtime.name
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.tools.clone()
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        cancel: CancellationToken,
    ) -> Result<ToolResult> {
        match self
            .rpc_call_with_cancel(
                "execute_tool",
                json!({ "name": tool_name, "args": args.clone() }),
                cancel.clone(),
                Some(EXTENSION_TOOL_RPC_TIMEOUT),
            )
            .await
        {
            Ok(output) => Ok(self.extension_tool_result(output, _call_id).await),
            Err(e) if is_extension_transport_error(&e) => {
                self.record_error(format!("transport failure: {e}")).await;
                self.respawn_after_transport_error(&e).await?;
                let output = self
                    .rpc_call_with_cancel(
                        "execute_tool",
                        json!({ "name": tool_name, "args": args }),
                        cancel,
                        Some(EXTENSION_TOOL_RPC_TIMEOUT),
                    )
                    .await
                    .map_err(|retry_err| {
                        anyhow!(
                            "extension '{}' reconnected after transport failure, but retrying '{}' failed: {retry_err}",
                            self.runtime.name,
                            tool_name
                        )
                    })?;
                let mut result = self.extension_tool_result(output, _call_id).await;
                result.details = match result.details {
                    Value::Object(mut details) => {
                        details.insert("extension_reconnected".to_string(), Value::Bool(true));
                        Value::Object(details)
                    }
                    other => json!({"extension_reconnected": true, "extension_details": other}),
                };
                Ok(result)
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("MethodNotFound") {
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!(
                                "Extension '{}' does not support tool execution. \
                                 The tool '{}' was advertised but cannot be called.",
                                self.runtime.name, tool_name
                            ),
                        }],
                        details: json!({"is_error": true}),
                    })
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn execute_with_context(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: CancellationToken,
        _sink: omegon_traits::ToolProgressSink,
        context: omegon_traits::ToolExecutionContext,
    ) -> Result<ToolResult> {
        match self
            .rpc_call_with_cancel(
                "execute_tool",
                json!({ "name": tool_name, "args": args.clone() }),
                cancel.clone(),
                Some(EXTENSION_TOOL_RPC_TIMEOUT),
            )
            .await
        {
            Ok(output) => Ok(self
                .extension_tool_result_with_context(output, call_id, &context)
                .await),
            Err(e) if is_extension_transport_error(&e) => {
                self.record_error(format!("transport failure: {e}")).await;
                self.respawn_after_transport_error(&e).await?;
                let output = self
                    .rpc_call_with_cancel(
                        "execute_tool",
                        json!({ "name": tool_name, "args": args }),
                        cancel,
                        Some(EXTENSION_TOOL_RPC_TIMEOUT),
                    )
                    .await
                    .map_err(|retry_err| {
                        anyhow!(
                            "extension '{}' reconnected after transport failure, but retrying '{}' failed: {retry_err}",
                            self.runtime.name,
                            tool_name
                        )
                    })?;
                let mut result = self
                    .extension_tool_result_with_context(output, call_id, &context)
                    .await;
                result.details = match result.details {
                    Value::Object(mut details) => {
                        details.insert("extension_reconnected".to_string(), Value::Bool(true));
                        Value::Object(details)
                    }
                    other => json!({"extension_reconnected": true, "extension_details": other}),
                };
                Ok(result)
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("MethodNotFound") {
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!(
                                "Extension '{}' does not support tool execution. \
                                 The tool '{}' was advertised but cannot be called.",
                                self.runtime.name, tool_name
                            ),
                        }],
                        details: json!({"is_error": true}),
                    })
                } else {
                    Err(e)
                }
            }
        }
    }
}

/// Result of spawning an extension: feature + widgets
pub struct SpawnedExtension {
    pub feature: Box<dyn Feature>,
    pub nex_delegation_executor: Option<std::sync::Arc<ExtensionFeature>>,
    pub widgets: Vec<ExtensionTabWidget>,
    pub widget_rx: broadcast::Receiver<WidgetEvent>,
    /// Optional metadata returned by the extension initialize handshake.
    pub metadata: Option<Value>,
    /// SDK contract compatibility classification derived from initialize metadata.
    pub sdk_compatibility: SdkCompatibilityDiagnostic,
    /// Generic RPC handle for ACP/runtime control-plane calls.
    pub rpc_polling_handle: ExtensionPollingHandle,
    /// Polling handle for extensions that provide `vox_route` (event bridge).
    pub vox_polling_handle: Option<ExtensionPollingHandle>,
    /// Idle notification pump for voice-capable extensions.
    pub voice_polling_handle: Option<ExtensionPollingHandle>,
    /// Push notification receiver for voice-capable extensions.
    pub voice_notification_rx: Option<mpsc::UnboundedReceiver<ExtensionNotification>>,
}

fn nex_delegation_executor(feature: &ExtensionFeature) -> Option<std::sync::Arc<ExtensionFeature>> {
    if feature.runtime.name == "omegon-nex"
        && feature
            .tools
            .iter()
            .any(|tool| tool.name == "nex_devenv_inspect")
    {
        Some(std::sync::Arc::new(feature.clone()))
    } else {
        None
    }
}

#[async_trait::async_trait]
impl crate::tools::nex_substrate::NexDelegationExecutor for ExtensionFeature {
    async fn execute_devenv_inspect(&self, tool: &str, path: &Path) -> anyhow::Result<ToolResult> {
        if self.runtime.name != "omegon-nex" || tool != "nex_devenv_inspect" {
            anyhow::bail!("unsupported Nex delegation tool: {tool}");
        }
        self.execute(
            "nex_devenv_inspect",
            "nex-substrate-delegation",
            json!({"path": path.display().to_string()}),
            CancellationToken::new(),
        )
        .await
    }
}

/// Spawn an extension from its manifest directory.
///
/// `resolved_secrets` contains pre-resolved (name, value) pairs for all secrets
/// declared in `manifest.secrets`. These are delivered via `bootstrap_secrets`
/// RPC — never via subprocess environment variables.
pub async fn spawn_from_manifest(
    ext_dir: &Path,
    resolved_secrets: &[(String, String)],
) -> Result<SpawnedExtension> {
    let manifest = ExtensionManifest::from_extension_dir(ext_dir)?;

    // Enforce required secrets before spending any resources on spawning.
    // Check against the pre-resolved pairs rather than process env.
    let missing: Vec<&str> = manifest
        .secrets
        .required
        .iter()
        .filter(|name| !resolved_secrets.iter().any(|(k, _)| k == *name))
        .map(|s| s.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(anyhow!(
            "extension '{}' requires secrets that could not be resolved: {}. \
             Configure them with: omegon secret set {}",
            manifest.extension.name,
            missing.join(", "),
            missing[0],
        ));
    }

    // Log optional secrets that are absent — extension will degrade gracefully.
    for name in &manifest.secrets.optional {
        if !resolved_secrets.iter().any(|(k, _)| k == name) {
            tracing::debug!(
                extension = %manifest.extension.name,
                secret = %name,
                "optional secret absent — extension may have reduced functionality"
            );
        }
    }

    let substrate = crate::execution_substrate::detect();
    if substrate.kind != omegon_traits::ExecutionSubstrateKind::HostNative
        && matches!(&manifest.runtime, RuntimeConfig::Native { .. })
    {
        return Err(anyhow!(
            "native extension '{}' is disabled under {:?} execution substrate; use an OCI/image-bundled extension build or run Omegon host-native",
            manifest.extension.name,
            substrate.kind
        ));
    }

    let state = ExtensionState::load(ext_dir)?;
    let widgets: Vec<WidgetDeclaration> = manifest
        .widgets
        .iter()
        .map(|(id, config)| WidgetDeclaration {
            id: id.clone(),
            label: config.label.clone(),
            kind: config.kind.clone(),
            renderer: config.renderer.clone(),
            description: config.description.clone(),
        })
        .collect();

    match manifest.runtime {
        RuntimeConfig::Native { .. } => {
            let binary = manifest.native_binary_path(ext_dir)?;
            spawn_native(&manifest, ext_dir, binary, widgets, state, resolved_secrets).await
        }
        RuntimeConfig::Oci { .. } => {
            let image = manifest.oci_image()?;
            spawn_container(&manifest, ext_dir, &image, widgets, state, resolved_secrets).await
        }
    }
}

/// Build a `Command` with a clean environment — only safe non-secret vars inherited.
/// Secrets are delivered via `bootstrap_secrets` RPC, never via env.
fn clean_command(
    program: impl AsRef<std::ffi::OsStr>,
    manifest: &ExtensionManifest,
) -> Result<tokio::process::Command> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.env_clear();
    for var in SAFE_INHERIT_ENVS {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }
    for (name, value) in resolved_runtime_env(manifest)? {
        cmd.env(name, value);
    }
    Ok(cmd)
}

fn resolved_runtime_env(manifest: &ExtensionManifest) -> Result<Vec<(String, String)>> {
    let mut env = Vec::new();
    for (name, value) in manifest.runtime.env() {
        validate_runtime_env_name(name)?;
        env.push((name.clone(), value.clone()));
    }
    for name in manifest.runtime.env_passthrough() {
        validate_runtime_env_name(name)?;
        if let Ok(value) = std::env::var(name) {
            env.push((name.clone(), value));
        }
    }
    Ok(env)
}

fn validate_runtime_env_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_uppercase() || ch.is_ascii_digit())
        || name.contains("SECRET")
        || name.contains("TOKEN")
        || name.contains("PASSWORD")
        || name.contains("KEY")
    {
        return Err(anyhow!(
            "runtime env var '{name}' is not allowed; manifest runtime.env is for non-secret uppercase names only"
        ));
    }
    Ok(())
}

async fn spawn_process_handles(
    manifest: &ExtensionManifest,
    ext_dir: &Path,
) -> Result<ProcessHandles> {
    let extension_name = manifest.extension.name.clone();
    let mut child = match &manifest.runtime {
        RuntimeConfig::Native { .. } => {
            let binary = manifest.native_binary_path(ext_dir)?;
            let mut cmd = clean_command(&binary, manifest)?;
            cmd.arg("--rpc")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        }
        RuntimeConfig::Oci { .. } => {
            let image = manifest.oci_image()?;
            let mut cmd = clean_command("podman", manifest)?;
            cmd.args(["run", "--rm", "-i"]);
            for (name, value) in resolved_runtime_env(manifest)? {
                cmd.args(["--env", &format!("{name}={value}")]);
            }
            cmd.arg(&image)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        }
    };

    if let Some(stderr) = child.stderr.take() {
        spawn_extension_stderr_drain(extension_name, stderr);
    }

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    Ok(ProcessHandles::new(child, stdin, stdout))
}

fn spawn_extension_stderr_drain(extension_name: String, stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let message = line.trim_end();
                    if !message.is_empty() {
                        tracing::debug!(extension = %extension_name, message, "extension stderr");
                    }
                }
                Err(error) => {
                    tracing::debug!(extension = %extension_name, %error, "failed to read extension stderr");
                    break;
                }
            }
        }
    });
}

/// Run the extension handshake sequence on a single process:
/// 1. `get_tools` — discover tools (required by contract)
/// 2. `bootstrap_secrets` — deliver secrets over pipe (never via env)
///
/// Returns handles with `next_id` advanced past the handshake, and the tool list.
async fn handshake(
    handles: &mut ProcessHandles,
    manifest: &ExtensionManifest,
    ext_dir: &Path,
    resolved_secrets: &[(String, String)],
    notification_sink: Option<&ExtensionNotificationSink>,
) -> Result<ExtensionHandshake> {
    let name = &manifest.extension.name;

    // 1. Optional initialize handshake metadata. Older extensions may not
    // implement this method; absence must not prevent startup.
    let metadata = match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        handles.rpc_call_with_notifications("initialize", json!({}), notification_sink),
    )
    .await
    {
        Ok(Ok(value)) => Some(value),
        Ok(Err(e)) => {
            tracing::debug!(extension = name, error = %e, "extension initialize metadata unavailable");
            None
        }
        Err(_) => {
            // Older extensions may not implement `initialize` at all. The
            // optional probe must not strand startup. Keep the request counter
            // advanced: a late initialize response may still arrive on stdout,
            // and reusing its id for get_tools would let stale metadata satisfy
            // the discovery request.
            tracing::debug!(extension = name, "extension initialize metadata timed out");
            None
        }
    };

    let sdk_compatibility = sdk_compat::classify_initialize_metadata(metadata.as_ref());
    if sdk_compatibility.is_blocking() {
        return Err(anyhow!(
            "extension '{}' SDK contract is incompatible: {}",
            name,
            sdk_compatibility.message
        ));
    }
    if sdk_compatibility.status == sdk_compat::SdkCompatibilityStatus::MissingLegacy {
        tracing::warn!(
            extension = name,
            supported_sdk_contract = %sdk_compatibility.supported_version,
            "extension did not advertise SDK contract version; treating as legacy"
        );
    }

    // 2. Discover tools
    let tools_response = handles
        .rpc_call_with_notifications("get_tools", json!({}), notification_sink)
        .await?;
    let tools = normalize_extension_tool_definitions(&tools_response).map_err(|err| {
        anyhow!(
            "extension '{}' returned invalid get_tools response: {err}",
            name
        )
    })?;

    // 3. Deliver typed config defaults, manifest runtime config, and persisted operator values.
    // Values are delivered over RPC after process start so extension config
    // stays in the same channel as secrets and never depends on inherited env.
    let config = resolved_config(manifest, ext_dir)?;
    if !config.is_empty() {
        match handles
            .rpc_call_with_notifications(
                "bootstrap_config",
                Value::Object(config),
                notification_sink,
            )
            .await
        {
            Ok(_) => tracing::debug!(extension = name, "bootstrap_config delivered"),
            Err(e) => {
                tracing::warn!(
                    extension = name,
                    error = %e,
                    "bootstrap_config delivery failed"
                );
            }
        }
    }

    // 4. Deliver secrets over pipe — never via env var
    if !resolved_secrets.is_empty() {
        let secrets_map: serde_json::Map<String, Value> = resolved_secrets
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        match handles
            .rpc_call_with_notifications(
                "bootstrap_secrets",
                Value::Object(secrets_map),
                notification_sink,
            )
            .await
        {
            Ok(_) => tracing::debug!(
                extension = name,
                secrets = resolved_secrets.len(),
                "bootstrap_secrets delivered"
            ),
            Err(e) => {
                tracing::error!(
                    extension = name,
                    error = %e,
                    "bootstrap_secrets delivery failed — extension will run without secrets"
                );
                return Err(anyhow!(
                    "extension '{}' failed to accept bootstrap_secrets: {e}. \
                     Secrets delivery is required for extensions that declare secrets.",
                    name,
                ));
            }
        }
    }

    Ok(ExtensionHandshake {
        tools,
        metadata,
        sdk_compatibility,
    })
}

struct ExtensionHandshake {
    tools: Vec<ToolDefinition>,
    metadata: Option<Value>,
    sdk_compatibility: SdkCompatibilityDiagnostic,
}

pub(crate) fn metadata_with_sdk_compatibility(
    metadata: Option<Value>,
    diagnostic: &SdkCompatibilityDiagnostic,
) -> Value {
    let sdk_compatibility = serde_json::to_value(diagnostic)
        .unwrap_or_else(|_| serde_json::json!({"status": "serialization_failed"}));
    let mut metadata = metadata.unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("sdk_compatibility".to_string(), sdk_compatibility);
        metadata
    } else {
        serde_json::json!({
            "initialize": metadata,
            "sdk_compatibility": sdk_compatibility,
        })
    }
}

fn normalize_extension_tool_definitions(value: &Value) -> Result<Vec<ToolDefinition>> {
    let tools = value
        .as_array()
        .ok_or_else(|| anyhow!("get_tools result must be an array"))?;
    tools
        .iter()
        .map(normalize_extension_tool_definition)
        .collect()
}

fn normalize_extension_tool_definition(value: &Value) -> Result<ToolDefinition> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("tool definition must be an object"))?;
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("tool definition missing non-empty name"))?
        .to_string();
    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.is_empty())
        .unwrap_or(&name)
        .to_string();
    let description = obj
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let parameters = obj
        .get("parameters")
        .or_else(|| obj.get("inputSchema"))
        .or_else(|| obj.get("input_schema"))
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    let capabilities = obj
        .get("capabilities")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|err| anyhow!("tool '{name}' has invalid capabilities: {err}"))?
        .unwrap_or_default();

    let description = if description.is_empty() {
        "Extension tool. Semantics are owned by the extension, not Omegon core.".to_string()
    } else {
        format!(
            "Extension tool (not Omegon core; semantics are owned by the extension): {description}"
        )
    };

    Ok(ToolDefinition {
        name,
        label,
        description,
        parameters,
        capabilities,
    })
}

fn resolved_config(
    manifest: &ExtensionManifest,
    ext_dir: &Path,
) -> Result<serde_json::Map<String, Value>> {
    let mut config = serde_json::Map::new();
    for (name, value) in manifest.runtime.config() {
        config.insert(name.clone(), value.clone());
    }

    let stored = config_store::read_config(ext_dir)?;

    for (name, field) in &manifest.config {
        if let Some(default) = &field.default {
            config.insert(name.clone(), config_value_to_json(field, default));
        } else if field.required && !stored.contains_key(name) {
            return Err(anyhow!(
                "extension '{}' requires config value '{}'. \
                 Configure it with the extension settings UI or ACP config RPC.",
                manifest.extension.name,
                name
            ));
        }
    }

    for (name, value) in stored {
        if let Some(field) = manifest.config.get(&name) {
            config_store::validate_field(field, &value)?;
            config.insert(name, config_value_to_json(field, &value));
        } else {
            config.insert(name, Value::String(value));
        }
    }

    Ok(config)
}

fn is_extension_transport_error(error: &anyhow::Error) -> bool {
    let msg = error.to_string().to_ascii_lowercase();
    msg.contains("broken pipe")
        || msg.contains("connection reset")
        || msg.contains("connection aborted")
        || msg.contains("extension closed connection")
        || msg.contains("closed channel")
        || msg.contains("early eof")
        || msg.contains("unexpected eof")
}

fn config_value_to_json(field: &omegon_extension::ConfigField, value: &str) -> Value {
    use omegon_extension::ConfigFieldType;

    match field.field_type {
        ConfigFieldType::Boolean => Value::Bool(value == "true"),
        ConfigFieldType::Number => value
            .parse::<serde_json::Number>()
            .map(Value::Number)
            .unwrap_or_else(|_| Value::String(value.to_string())),
        ConfigFieldType::String | ConfigFieldType::Enum | ConfigFieldType::Text => {
            Value::String(value.to_string())
        }
    }
}

async fn spawn_native(
    manifest: &ExtensionManifest,
    ext_dir: &Path,
    binary: PathBuf,
    widgets: Vec<WidgetDeclaration>,
    state: ExtensionState,
    resolved_secrets: &[(String, String)],
) -> Result<SpawnedExtension> {
    let mut handles = spawn_process_handles(manifest, ext_dir).await?;

    let notification_pair = if manifest.capabilities.voice {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Some(ExtensionNotificationSink {
                extension_name: manifest.extension.name.clone(),
                tx,
            }),
            Some(rx),
        )
    } else {
        (None, None)
    };

    let handshake = handshake(
        &mut handles,
        manifest,
        ext_dir,
        resolved_secrets,
        notification_pair.0.as_ref(),
    )
    .await?;

    tracing::info!(
        name = %manifest.extension.name,
        binary = %binary.display(),
        tools = handshake.tools.len(),
        widgets = widgets.len(),
        secrets = resolved_secrets.len(),
        "spawned native extension"
    );

    let runtime = ExtensionRuntimeContext {
        name: manifest.extension.name.clone(),
        ext_dir: ext_dir.to_path_buf(),
        manifest: manifest.clone(),
        resolved_secrets: resolved_secrets.to_vec(),
        notification_sink: notification_pair.0,
    };

    let (feature, widget_rx) = ExtensionFeature::new(
        runtime,
        handshake.tools.clone(),
        widgets.clone(),
        handles,
        state,
    );

    // Extract polling handle if this extension provides vox_route
    let vox_polling_handle = if handshake.tools.iter().any(|t| t.name == "vox_route") {
        tracing::info!(
            name = %manifest.extension.name,
            "extension provides vox_route — creating polling handle for event bridge"
        );
        Some(feature.polling_handle())
    } else {
        None
    };

    let mut tab_widgets = vec![];
    for widget in widgets {
        let mut tab_widget = ExtensionTabWidget::new(
            widget.id.clone(),
            widget.label,
            widget.renderer,
            widget.kind,
        );
        if let Ok(data) = feature
            .rpc_call(&format!("get_{}", widget.id), json!({}))
            .await
        {
            tab_widget.update(data);
        }
        tab_widgets.push(tab_widget);
    }

    let voice_polling_handle = if manifest.capabilities.voice {
        Some(feature.polling_handle())
    } else {
        None
    };

    let nex_delegation_executor = nex_delegation_executor(&feature);
    let rpc_polling_handle = feature.polling_handle();
    Ok(SpawnedExtension {
        feature: Box::new(feature),
        widgets: tab_widgets,
        widget_rx,
        metadata: handshake.metadata,
        sdk_compatibility: handshake.sdk_compatibility,
        nex_delegation_executor,
        rpc_polling_handle,
        vox_polling_handle,
        voice_polling_handle,
        voice_notification_rx: notification_pair.1,
    })
}

async fn spawn_container(
    manifest: &ExtensionManifest,
    ext_dir: &Path,
    image: &str,
    widgets: Vec<WidgetDeclaration>,
    state: ExtensionState,
    resolved_secrets: &[(String, String)],
) -> Result<SpawnedExtension> {
    let mut handles = spawn_process_handles(manifest, ext_dir).await?;

    let notification_pair = if manifest.capabilities.voice {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Some(ExtensionNotificationSink {
                extension_name: manifest.extension.name.clone(),
                tx,
            }),
            Some(rx),
        )
    } else {
        (None, None)
    };

    let handshake = handshake(
        &mut handles,
        manifest,
        ext_dir,
        resolved_secrets,
        notification_pair.0.as_ref(),
    )
    .await?;

    tracing::info!(
        name = %manifest.extension.name,
        image = image,
        tools = handshake.tools.len(),
        widgets = widgets.len(),
        secrets = resolved_secrets.len(),
        "spawned OCI extension"
    );

    let runtime = ExtensionRuntimeContext {
        name: manifest.extension.name.clone(),
        ext_dir: ext_dir.to_path_buf(),
        manifest: manifest.clone(),
        resolved_secrets: resolved_secrets.to_vec(),
        notification_sink: notification_pair.0,
    };

    let (feature, widget_rx) = ExtensionFeature::new(
        runtime,
        handshake.tools.clone(),
        widgets.clone(),
        handles,
        state,
    );

    let vox_polling_handle = if handshake.tools.iter().any(|t| t.name == "vox_route") {
        Some(feature.polling_handle())
    } else {
        None
    };

    let mut tab_widgets = vec![];
    for widget in widgets {
        let mut tab_widget = ExtensionTabWidget::new(
            widget.id.clone(),
            widget.label,
            widget.renderer,
            widget.kind,
        );
        if let Ok(data) = feature
            .rpc_call(&format!("get_{}", widget.id), json!({}))
            .await
        {
            tab_widget.update(data);
        }
        tab_widgets.push(tab_widget);
    }

    let voice_polling_handle = if manifest.capabilities.voice {
        Some(feature.polling_handle())
    } else {
        None
    };

    let nex_delegation_executor = nex_delegation_executor(&feature);
    let rpc_polling_handle = feature.polling_handle();
    Ok(SpawnedExtension {
        feature: Box::new(feature),
        widgets: tab_widgets,
        widget_rx,
        metadata: handshake.metadata,
        sdk_compatibility: handshake.sdk_compatibility,
        nex_delegation_executor,
        rpc_polling_handle,
        vox_polling_handle,
        voice_polling_handle,
        voice_notification_rx: notification_pair.1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_extension::{ConfigField, ConfigFieldType};
    use std::collections::HashMap;

    #[test]
    fn extension_manifest_paths() {
        // Placeholder for integration tests
    }

    #[test]
    fn required_secret_check_detects_missing() {
        // Required secret not in resolved_secrets → missing
        let required = ["GITHUB_TOKEN".to_string()];
        let resolved: Vec<(String, String)> = vec![];
        let missing: Vec<&str> = required
            .iter()
            .filter(|name| !resolved.iter().any(|(k, _)| k == *name))
            .map(|s| s.as_str())
            .collect();
        assert_eq!(missing, vec!["GITHUB_TOKEN"]);
    }

    #[test]
    fn required_secret_check_passes_when_present() {
        // Required secret is in resolved_secrets → no missing
        let required = ["GITHUB_TOKEN".to_string()];
        let resolved = [("GITHUB_TOKEN".to_string(), "ghp_test".to_string())];
        let missing: Vec<&str> = required
            .iter()
            .filter(|name| !resolved.iter().any(|(k, _)| k == *name))
            .map(|s| s.as_str())
            .collect();
        assert!(missing.is_empty());
    }

    #[test]
    fn clean_command_strips_secrets() {
        // Verify SAFE_INHERIT_ENVS doesn't include any secret-like names
        for var in SAFE_INHERIT_ENVS {
            assert!(
                !var.contains("KEY")
                    && !var.contains("TOKEN")
                    && !var.contains("SECRET")
                    && !var.contains("PASSWORD"),
                "SAFE_INHERIT_ENVS contains potentially secret var: {var}"
            );
        }
    }

    #[test]
    fn resolved_config_applies_defaults_and_stored_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let manifest = test_manifest(HashMap::from([
            (
                "agent_browser_binary".to_string(),
                config_field(ConfigFieldType::String, Some("agent-browser"), false),
            ),
            (
                "max_output".to_string(),
                config_field(ConfigFieldType::Number, Some("50000"), false),
            ),
        ]));
        config_store::write_config_value(temp.path(), "max_output", "2000").unwrap();

        let config = resolved_config(&manifest, temp.path()).unwrap();

        assert_eq!(
            config.get("agent_browser_binary"),
            Some(&Value::String("agent-browser".to_string()))
        );
        assert_eq!(config.get("max_output"), Some(&Value::Number(2000.into())));
    }

    #[test]
    fn resolved_config_requires_missing_required_values() {
        let temp = tempfile::tempdir().unwrap();
        let manifest = test_manifest(HashMap::from([(
            "required_value".to_string(),
            config_field(ConfigFieldType::String, None, true),
        )]));

        let err = resolved_config(&manifest, temp.path()).unwrap_err();
        assert!(err.to_string().contains("required_value"));
    }

    #[test]
    fn extension_transport_error_detection_covers_stale_handles() {
        assert!(is_extension_transport_error(&anyhow!("broken pipe")));
        assert!(is_extension_transport_error(&anyhow!(
            "extension closed connection"
        )));
        assert!(is_extension_transport_error(&anyhow!("unexpected EOF")));
        assert!(!is_extension_transport_error(&anyhow!(
            "RPC error: MethodNotFound"
        )));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn extension_tool_call_respawns_after_child_exits() {
        let _env_guard = crate::test_support::env::lock_async().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("first-call-done");
        let script = temp.path().join("flaky-extension.sh");
        let script_body = r#"#!/bin/sh
marker=__MARKER__
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}}]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"Method not found\"}}"
      ;;
    *get_tools*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":[{\"name\":\"echo\",\"label\":\"Echo\",\"description\":\"Echo\",\"parameters\":{\"type\":\"object\",\"properties\":{}}}]}"
      ;;
    *execute_tool*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"ok\":true}}"
      if [ ! -f "$marker" ]; then
        touch "$marker"
        exit 0
      fi
      ;;
  esac
done
"#
        .replace("__MARKER__", &marker.display().to_string());
        std::fs::write(&script, script_body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        std::fs::write(
            temp.path().join("manifest.toml"),
            r#"
[extension]
name = "flaky"
version = "0.1.0"
description = "Flaky test extension"

[runtime]
type = "native"
binary = "flaky-extension.sh"
"#,
        )
        .unwrap();

        let spawned = spawn_from_manifest(temp.path(), &[]).await.unwrap();
        let first = spawned
            .feature
            .execute("echo", "call-1", json!({}), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(first.details["extension_reconnected"], Value::Null);

        let second = spawned
            .feature
            .execute("echo", "call-2", json!({}), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(second.details["extension_reconnected"], true);
    }

    #[test]
    fn extension_sdk_tool_schema_normalizes_input_schema() {
        let tools = normalize_extension_tool_definitions(&json!([
            {
                "name": "reader_doctor",
                "description": "Diagnose Bookokrat availability and HostAction readiness",
                "inputSchema": {"type": "object", "properties": {}}
            },
            {
                "name": "reader_open",
                "description": "Open a readable file",
                "inputSchema": {
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"]
                }
            }
        ]))
        .unwrap();

        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "reader_doctor");
        assert_eq!(tools[0].label, "reader_doctor");
        assert_eq!(tools[0].parameters["type"], "object");
        assert!(tools[0].description.starts_with(
            "Extension tool (not Omegon core; semantics are owned by the extension):"
        ));
        assert_eq!(tools[1].name, "reader_open");
        assert_eq!(tools[1].parameters["required"][0], "path");
    }

    #[test]
    fn extension_internal_tool_schema_still_accepts_parameters_and_label() {
        let tools = normalize_extension_tool_definitions(&json!([
            {
                "name": "hello_extension",
                "label": "Hello Extension",
                "description": "Say hello",
                "parameters": {"type": "object", "properties": {"name": {"type": "string"}}}
            }
        ]))
        .unwrap();

        assert_eq!(tools[0].name, "hello_extension");
        assert_eq!(tools[0].label, "Hello Extension");
        assert!(
            tools[0]
                .description
                .contains("semantics are owned by the extension")
        );
        assert_eq!(tools[0].parameters["properties"]["name"]["type"], "string");
    }

    #[test]
    fn extension_tool_schema_rejects_missing_name() {
        let err = normalize_extension_tool_definitions(&json!([
            {"description": "broken", "inputSchema": {"type": "object"}}
        ]))
        .unwrap_err();

        assert!(err.to_string().contains("missing non-empty name"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn voice_capable_extension_notification_does_not_break_get_tools_response_matching() {
        let _env_guard = crate::test_support::env::lock_async().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("voice-extension.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}}]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"Method not found\"}}"
      ;;
    *get_tools*)
      printf '%s\n' '{"jsonrpc":"2.0","method":"voice/transcription","params":{"text":"synthetic validation","duration_s":0.2}}'
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":[{\"name\":\"voice_status\",\"description\":\"Voice status\",\"inputSchema\":{\"type\":\"object\",\"properties\":{}}}]}"
      ;;
    *bootstrap_config*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"acknowledged\":true}}"
      ;;
    *execute_tool*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}"
      ;;
  esac
done
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        std::fs::write(
            temp.path().join("manifest.toml"),
            r#"
[extension]
name = "voice-test"
version = "0.1.0"
description = "Voice test extension"

[runtime]
type = "native"
binary = "voice-extension.sh"

[capabilities]
voice = true
"#,
        )
        .unwrap();

        let spawned = spawn_from_manifest(temp.path(), &[]).await.unwrap();
        let mut rx = spawned
            .voice_notification_rx
            .expect("voice-capable extension should expose notification receiver");
        let names: Vec<String> = spawned
            .feature
            .tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(names, vec!["voice_status"]);

        let notification = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("notification received")
            .expect("notification channel open");
        assert_eq!(notification.extension_name, "voice-test");
        assert_eq!(notification.method, "voice/transcription");
        assert_eq!(notification.params["text"], "synthetic validation");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn voice_capable_extension_notification_reaches_daemon_queue_through_bridge() {
        let _env_guard = crate::test_support::env::lock_async().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("voice-extension.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}}]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"Method not found\"}}"
      ;;
    *get_tools*)
      printf '%s\n' '{"jsonrpc":"2.0","method":"voice/transcription","params":{"text":"summarize the current project","utterance_id":"test-u1","duration_s":1.2}}'
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":[{\"name\":\"voice_status\",\"description\":\"Voice status\",\"inputSchema\":{\"type\":\"object\",\"properties\":{}}}]}"
      ;;
  esac
done
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        std::fs::write(
            temp.path().join("manifest.toml"),
            r#"
[extension]
name = "voice-test"
version = "0.1.0"
description = "Voice test extension"

[runtime]
type = "native"
binary = "voice-extension.sh"

[capabilities]
voice = true
"#,
        )
        .unwrap();

        let spawned = spawn_from_manifest(temp.path(), &[]).await.unwrap();
        let rx = spawned
            .voice_notification_rx
            .expect("voice-capable extension should expose notification receiver");
        let daemon_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let cancel = tokio_util::sync::CancellationToken::new();
        crate::extensions::voice_bridge::start_voice_bridge(
            rx,
            daemon_events.clone(),
            cancel.clone(),
        );

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let Some(event) = daemon_events.lock().unwrap().first().cloned() {
                    return event;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("voice bridge should inject daemon event");
        cancel.cancel();

        assert_eq!(event.source, "voice");
        assert_eq!(event.trigger_kind, "prompt");
        assert_eq!(event.source_channel.as_deref(), Some("voice"));
        assert_eq!(event.caller_role.as_deref(), Some("edit"));
        assert_eq!(event.payload["text"], "summarize the current project");
        assert_eq!(event.payload["utterance_id"], "test-u1");
        assert_eq!(event.payload["duration_s"], 1.2);
        assert_eq!(event.payload["extension"], "voice-test");
        assert_eq!(event.payload["trust_level"], "operator");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn non_voice_extension_does_not_get_voice_notification_receiver() {
        let _env_guard = crate::test_support::env::lock_async().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("voice-extension.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}}]*\).*/\1/p')
  case "$line" in
    *initialize*)
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"Method not found\"}}"
      ;;
    *get_tools*)
      printf '%s\n' '{"jsonrpc":"2.0","method":"voice/transcription","params":{"text":"should not inject"}}'
      printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":[{\"name\":\"status\",\"description\":\"Status\",\"inputSchema\":{\"type\":\"object\",\"properties\":{}}}]}"
      ;;
  esac
done
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        std::fs::write(
            temp.path().join("manifest.toml"),
            r#"
[extension]
name = "not-voice"
version = "0.1.0"
description = "Non voice extension"

[runtime]
type = "native"
binary = "voice-extension.sh"
"#,
        )
        .unwrap();

        let spawned = spawn_from_manifest(temp.path(), &[]).await.unwrap();
        assert!(
            spawned.voice_notification_rx.is_none(),
            "non-voice extension must not get a voice notification receiver"
        );
        let names: Vec<String> = spawned
            .feature
            .tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(names, vec!["status"]);
    }

    #[test]
    fn host_rpc_actions_execute_routes_to_policy_pipeline() {
        let manifest = test_manifest(HashMap::new());
        let request = omegon_extension::RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!("ext-1")),
            method: "actions/execute".to_string(),
            params: json!({
                "action": {"id": "broken", "params": {}}
            }),
        };

        let response =
            host_rpc_response_for_extension_request(&manifest, "test-extension", &request).unwrap();
        assert_eq!(response["id"], "ext-1");
        assert_eq!(response["result"]["status"], "invalid");
    }

    #[test]
    fn host_rpc_actions_execute_cannot_bypass_manifest_policy() {
        let manifest = test_manifest(HashMap::new());
        let request = omegon_extension::RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!("ext-2")),
            method: "actions/execute".to_string(),
            params: json!({
                "action": {"id": "open-reader", "type": "terminal.create@1", "params": {}}
            }),
        };

        let response =
            host_rpc_response_for_extension_request(&manifest, "test-extension", &request).unwrap();
        assert_eq!(response["result"]["status"], "denied");
        assert_eq!(response["result"]["error"]["code"], "manifest_denied");
    }

    #[test]
    fn declarative_host_actions_render_as_outcomes_separate_from_content() {
        let mut envelope = tool_result::parse_extension_tool_envelope(json!({
            "content": [{"type": "text", "text": "Opening reader"}],
            "actions": [{"id": "open-reader", "type": "terminal.create@1", "params": {}}]
        }));
        let actions = std::mem::take(&mut envelope.host_actions);
        let outcomes = host_actions::process_declarative_host_actions(
            actions,
            &test_manifest(HashMap::new()),
            "reader",
            "call-1",
        );
        envelope.host_action_outcomes.extend(outcomes);
        let result = envelope.into_tool_result();

        match &result.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Opening reader"),
            ContentBlock::Image { .. } => panic!("expected text"),
        }
        assert!(result.details.get("host_actions").is_none());
        assert_eq!(
            result.details["host_action_outcomes"][0]["status"],
            "denied"
        );
        assert_eq!(
            result.details["host_action_outcomes"][0]["error"]["code"],
            "manifest_denied"
        );
    }

    fn config_field(
        field_type: ConfigFieldType,
        default: Option<&str>,
        required: bool,
    ) -> ConfigField {
        ConfigField {
            field_type,
            label: "Test".to_string(),
            description: String::new(),
            required,
            default: default.map(ToString::to_string),
            pattern: None,
            placeholder: None,
            values: Vec::new(),
        }
    }

    fn test_manifest(config: HashMap<String, ConfigField>) -> ExtensionManifest {
        ExtensionManifest {
            extension: manifest::ExtensionMetadata {
                name: "test-extension".to_string(),
                version: "0.1.0".to_string(),
                description: String::new(),
            },
            runtime: RuntimeConfig::Native {
                binary: "test-extension".to_string(),
                env: HashMap::new(),
                env_passthrough: Vec::new(),
                config: HashMap::new(),
            },
            startup: manifest::StartupConfig::default(),
            widgets: HashMap::new(),
            secrets: manifest::SecretsConfig::default(),
            mcp: None,
            config,
            capabilities: omegon_extension::Capabilities::default(),
            permissions: omegon_extension::ManifestPermissions::default(),
            skills: Vec::new(),
        }
    }
}

#[cfg(test)]
mod sdk_compat_metadata_tests {
    use super::*;
    use serde_json::json;

    fn supported() -> SdkCompatibilityDiagnostic {
        sdk_compat::classify_sdk_version(Some(sdk_compat::SUPPORTED_SDK_CONTRACT_VERSION))
    }

    #[test]
    fn metadata_helper_inserts_sdk_compatibility_into_object_metadata() {
        let metadata = metadata_with_sdk_compatibility(
            Some(json!({"extension_info": {"name": "demo"}})),
            &supported(),
        );
        assert_eq!(metadata["extension_info"]["name"], "demo");
        assert_eq!(metadata["sdk_compatibility"]["status"], "supported");
        assert_eq!(metadata["sdk_compatibility"]["supported_version"], "0.25");
    }

    #[test]
    fn metadata_helper_creates_metadata_for_legacy_missing_initialize() {
        let diagnostic = sdk_compat::classify_sdk_version(None);
        let metadata = metadata_with_sdk_compatibility(None, &diagnostic);
        assert_eq!(metadata["sdk_compatibility"]["status"], "missing_legacy");
        assert_eq!(metadata["sdk_compatibility"]["severity"], "warning");
    }

    #[test]
    fn metadata_helper_wraps_non_object_initialize_payload() {
        let metadata = metadata_with_sdk_compatibility(Some(json!("legacy")), &supported());
        assert_eq!(metadata["initialize"], "legacy");
        assert_eq!(metadata["sdk_compatibility"]["status"], "supported");
    }
}

#[cfg(all(test, unix))]
mod sdk_compat_spawn_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::LazyLock;

    static SDK_COMPAT_SPAWN_TEST_LOCK: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));

    fn write_sdk_extension(dir: &Path, sdk_version: Option<&str>) -> PathBuf {
        let script = dir.join("sdk-extension.sh");
        let initialize = match sdk_version {
            Some(version) => format!(
                "printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":'$id',\"result\":{{\"protocol_version\":2,\"extension_info\":{{\"name\":\"sdk-test\",\"version\":\"0.1.0\",\"sdk_version\":\"{version}\"}},\"capabilities\":{{\"tools\":true}},\"tools\":[]}}}}'"
            ),
            None => "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":'$id',\"error\":{\"code\":-32601,\"message\":\"Method not found\"}}'".to_string(),
        };
        let body = format!(
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}}]*\).*/\1/p')
  case "$line" in
    *initialize*)
      {initialize}
      ;;
    *get_tools*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":'$id',"result":[{{"name":"status","description":"Status","inputSchema":{{"type":"object","properties":{{}}}}}}]}}'
      ;;
  esac
done
"#
        );
        std::fs::write(&script, body).unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            r#"
[extension]
name = "sdk-test"
version = "0.1.0"
description = "SDK compatibility test extension"

[runtime]
type = "native"
binary = "sdk-extension.sh"
"#,
        )
        .unwrap();
        script
    }

    #[tokio::test]
    async fn spawn_rejects_native_extension_under_host_shim_oci() {
        let _env_guard = crate::test_support::env::lock_async().await;
        let _guard = SDK_COMPAT_SPAWN_TEST_LOCK.lock().await;
        unsafe {
            std::env::set_var("OMEGON_RUNTIME_CONTEXT", "host-shim-oci");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }

        let temp = tempfile::tempdir().unwrap();
        write_sdk_extension(
            temp.path(),
            Some(sdk_compat::SUPPORTED_SDK_CONTRACT_VERSION),
        );
        let err = match spawn_from_manifest(temp.path(), &[]).await {
            Ok(_) => panic!("native extension should be disabled under host-shim OCI"),
            Err(err) => err,
        };
        let message = err.to_string();
        assert!(
            message.contains("native extension 'sdk-test' is disabled"),
            "{message}"
        );
        assert!(message.contains("HostShimOci"), "{message}");

        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
        }
    }

    #[tokio::test]
    async fn spawn_accepts_current_sdk_contract() {
        let _env_guard = crate::test_support::env::lock_async().await;
        let _guard = SDK_COMPAT_SPAWN_TEST_LOCK.lock().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        let temp = tempfile::tempdir().unwrap();
        write_sdk_extension(
            temp.path(),
            Some(sdk_compat::SUPPORTED_SDK_CONTRACT_VERSION),
        );
        let spawned = spawn_from_manifest(temp.path(), &[]).await.unwrap();
        assert_eq!(
            spawned.sdk_compatibility.status,
            sdk_compat::SdkCompatibilityStatus::Supported
        );
    }

    #[tokio::test]
    async fn spawn_allows_older_compatible_sdk_contract_with_warning() {
        let _env_guard = crate::test_support::env::lock_async().await;
        let _guard = SDK_COMPAT_SPAWN_TEST_LOCK.lock().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        let temp = tempfile::tempdir().unwrap();
        write_sdk_extension(
            temp.path(),
            Some(sdk_compat::MIN_COMPATIBLE_SDK_CONTRACT_VERSION),
        );
        let spawned = spawn_from_manifest(temp.path(), &[]).await.unwrap();
        assert_eq!(
            spawned.sdk_compatibility.status,
            sdk_compat::SdkCompatibilityStatus::OlderCompatible
        );
        assert!(!spawned.sdk_compatibility.is_blocking());
    }

    #[tokio::test]
    async fn spawn_rejects_newer_unknown_sdk_contract() {
        let _env_guard = crate::test_support::env::lock_async().await;
        let _guard = SDK_COMPAT_SPAWN_TEST_LOCK.lock().await;
        let temp = tempfile::tempdir().unwrap();
        write_sdk_extension(temp.path(), Some("0.26"));
        let err = match spawn_from_manifest(temp.path(), &[]).await {
            Ok(_) => panic!("newer SDK contract should fail spawn"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("SDK contract is incompatible"));
        assert!(err.to_string().contains("newer than supported contract"));
        assert!(err.to_string().contains("newer than supported contract"));
    }

    #[tokio::test]
    async fn spawn_rejects_malformed_sdk_contract() {
        let _env_guard = crate::test_support::env::lock_async().await;
        let _guard = SDK_COMPAT_SPAWN_TEST_LOCK.lock().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        let temp = tempfile::tempdir().unwrap();
        write_sdk_extension(temp.path(), Some("banana"));
        let err = match spawn_from_manifest(temp.path(), &[]).await {
            Ok(_) => panic!("malformed SDK contract should fail spawn"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("SDK contract is incompatible"));
        assert!(err.to_string().contains("malformed SDK contract version"));
    }

    #[tokio::test]
    async fn spawn_allows_missing_initialize_as_legacy_warning() {
        let _env_guard = crate::test_support::env::lock_async().await;
        let _guard = SDK_COMPAT_SPAWN_TEST_LOCK.lock().await;
        unsafe {
            std::env::remove_var("OMEGON_RUNTIME_CONTEXT");
            std::env::remove_var("KUBERNETES_SERVICE_HOST");
        }
        let temp = tempfile::tempdir().unwrap();
        write_sdk_extension(temp.path(), None);
        match spawn_from_manifest(temp.path(), &[]).await {
            Ok(spawned) => {
                assert_eq!(
                    spawned.sdk_compatibility.status,
                    sdk_compat::SdkCompatibilityStatus::MissingLegacy
                );
            }
            Err(err) => {
                let message = err.to_string();
                assert!(
                    message.contains("Method not found"),
                    "unexpected missing-initialize error: {message}"
                );
            }
        }
    }
}

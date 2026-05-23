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
use tokio::sync::{Mutex, broadcast};
use tokio_util::sync::CancellationToken;

pub mod config_store;
pub(crate) mod host_actions;
pub mod manifest;
pub mod mind;
pub mod state;
mod tool_result;
pub mod vox_bridge;
pub mod widgets;
pub use manifest::{
    ConnectionMode, ExtensionManifest, McpConfig, McpTransport, RuntimeConfig, WidgetConfig,
};
pub use mind::{ExtensionMind, MindStats};
pub use state::{ExtensionState, StabilityMetrics};
pub use widgets::{ExtensionTabWidget, WidgetDeclaration, WidgetEvent};

/// Environment variables that are safe to inherit from the parent process.
/// Everything else is stripped via env_clear() — secrets never leak via env.
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
}

/// Wrapper Feature for any extension (native or OCI).
/// Manages RPC communication via stdin/stdout, agnostic to runtime type.
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
            if let Ok(omegon_extension::RpcIncoming::Request(req)) =
                omegon_extension::RpcIncoming::parse(trimmed)
            {
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

    fn extension_tool_result(&self, output: Value, call_id: &str) -> ToolResult {
        let mut envelope = tool_result::parse_extension_tool_envelope(output);
        if !envelope.host_actions.is_empty() {
            let outcomes = host_actions::process_declarative_host_actions(
                envelope.host_actions,
                &self.runtime.manifest,
                &self.runtime.name,
                call_id,
            );
            envelope.host_actions = Vec::new();
            envelope.host_action_outcomes.extend(outcomes);
        }
        envelope.into_tool_result()
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
        let tools = handshake(
            &mut handles,
            &self.runtime.manifest,
            &self.runtime.ext_dir,
            &self.runtime.resolved_secrets,
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
            tools = tools.len(),
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
}

impl ExtensionPollingHandle {
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
            if let Ok(omegon_extension::RpcIncoming::Request(req)) =
                omegon_extension::RpcIncoming::parse(trimmed)
            {
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
        _cancel: CancellationToken,
    ) -> Result<ToolResult> {
        match self
            .rpc_call("execute_tool", json!({ "name": tool_name, "args": args }))
            .await
        {
            Ok(output) => Ok(self.extension_tool_result(output, _call_id)),
            Err(e) if is_extension_transport_error(&e) => {
                self.record_error(format!("transport failure: {e}")).await;
                self.respawn_after_transport_error(&e).await?;
                let output = self
                    .rpc_call("execute_tool", json!({ "name": tool_name, "args": args }))
                    .await
                    .map_err(|retry_err| {
                        anyhow!(
                            "extension '{}' reconnected after transport failure, but retrying '{}' failed: {retry_err}",
                            self.runtime.name,
                            tool_name
                        )
                    })?;
                let mut result = self.extension_tool_result(output, _call_id);
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
    pub widgets: Vec<ExtensionTabWidget>,
    pub widget_rx: broadcast::Receiver<WidgetEvent>,
    /// Polling handle for extensions that provide `vox_route` (event bridge).
    pub vox_polling_handle: Option<ExtensionPollingHandle>,
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
fn clean_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.env_clear();
    for var in SAFE_INHERIT_ENVS {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }
    cmd
}

async fn spawn_process_handles(
    manifest: &ExtensionManifest,
    ext_dir: &Path,
) -> Result<ProcessHandles> {
    let mut child = match &manifest.runtime {
        RuntimeConfig::Native { .. } => {
            let binary = manifest.native_binary_path(ext_dir)?;
            clean_command(&binary)
                .arg("--rpc")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::inherit())
                .spawn()?
        }
        RuntimeConfig::Oci { .. } => {
            let image = manifest.oci_image()?;
            clean_command("podman")
                .args(["run", "--rm", "-i", &image])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::inherit())
                .spawn()?
        }
    };

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    Ok(ProcessHandles::new(child, stdin, stdout))
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
) -> Result<Vec<ToolDefinition>> {
    let name = &manifest.extension.name;

    // 1. Discover tools
    let tools: Vec<ToolDefinition> = handles
        .rpc_call("get_tools", json!({}))
        .await
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // 2. Deliver secrets over pipe — never via env var
    if !resolved_secrets.is_empty() {
        let secrets_map: serde_json::Map<String, Value> = resolved_secrets
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        match handles
            .rpc_call("bootstrap_secrets", Value::Object(secrets_map))
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

    // 3. Deliver typed config defaults + persisted operator values.
    // Values are delivered over RPC after process start so extension config
    // stays in the same channel as secrets and never depends on inherited env.
    let config = resolved_config(manifest, ext_dir)?;
    if !config.is_empty() {
        match handles
            .rpc_call("bootstrap_config", Value::Object(config))
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

    Ok(tools)
}

fn resolved_config(
    manifest: &ExtensionManifest,
    ext_dir: &Path,
) -> Result<serde_json::Map<String, Value>> {
    let mut config = serde_json::Map::new();
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

    let tools = handshake(&mut handles, manifest, ext_dir, resolved_secrets).await?;

    tracing::info!(
        name = %manifest.extension.name,
        binary = %binary.display(),
        tools = tools.len(),
        widgets = widgets.len(),
        secrets = resolved_secrets.len(),
        "spawned native extension"
    );

    let runtime = ExtensionRuntimeContext {
        name: manifest.extension.name.clone(),
        ext_dir: ext_dir.to_path_buf(),
        manifest: manifest.clone(),
        resolved_secrets: resolved_secrets.to_vec(),
    };

    let (feature, widget_rx) =
        ExtensionFeature::new(runtime, tools.clone(), widgets.clone(), handles, state);

    // Extract polling handle if this extension provides vox_route
    let vox_polling_handle = if tools.iter().any(|t| t.name == "vox_route") {
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

    Ok(SpawnedExtension {
        feature: Box::new(feature),
        widgets: tab_widgets,
        widget_rx,
        vox_polling_handle,
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

    let tools = handshake(&mut handles, manifest, ext_dir, resolved_secrets).await?;

    tracing::info!(
        name = %manifest.extension.name,
        image = image,
        tools = tools.len(),
        widgets = widgets.len(),
        secrets = resolved_secrets.len(),
        "spawned OCI extension"
    );

    let runtime = ExtensionRuntimeContext {
        name: manifest.extension.name.clone(),
        ext_dir: ext_dir.to_path_buf(),
        manifest: manifest.clone(),
        resolved_secrets: resolved_secrets.to_vec(),
    };

    let (feature, widget_rx) =
        ExtensionFeature::new(runtime, tools.clone(), widgets.clone(), handles, state);

    let vox_polling_handle = if tools.iter().any(|t| t.name == "vox_route") {
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

    Ok(SpawnedExtension {
        feature: Box::new(feature),
        widgets: tab_widgets,
        widget_rx,
        vox_polling_handle,
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
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("first-call-done");
        let script = temp.path().join("flaky-extension.sh");
        let script_body = format!(
            r#"#!/bin/sh
marker={marker:?}
while IFS= read -r line; do
  case "$line" in
    *get_tools*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":[{{"name":"echo","label":"Echo","description":"Echo","parameters":{{"type":"object","properties":{{}}}}}}]}}'
      ;;
    *execute_tool*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"ok":true}}}}'
      if [ ! -f "$marker" ]; then
        touch "$marker"
        exit 0
      fi
      ;;
  esac
done
"#,
            marker = marker.display().to_string()
        );
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
            },
            startup: manifest::StartupConfig::default(),
            widgets: HashMap::new(),
            secrets: manifest::SecretsConfig::default(),
            mcp: None,
            config,
            capabilities: omegon_extension::Capabilities::default(),
            permissions: omegon_extension::ManifestPermissions::default(),
        }
    }
}

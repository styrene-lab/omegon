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
pub mod manifest;
pub mod mind;
pub mod state;
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
    // Flynt vault root — read by the flynt-agent extension to locate the user's
    // active vault. Not a secret (just a filesystem path) and required for the
    // agent to answer "what document am I looking at?". CODEX_VAULT is kept for
    // pre-rename installs.
    "FLYNT_VAULT",
    "CODEX_VAULT",
];

/// Handles for communicating with an extension process.
pub struct ProcessHandles {
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl ProcessHandles {
    fn new(stdin: tokio::process::ChildStdin, stdout: tokio::process::ChildStdout) -> Self {
        Self {
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

/// Wrapper Feature for any extension (native or OCI).
/// Manages RPC communication via stdin/stdout, agnostic to runtime type.
pub struct ExtensionFeature {
    name: String,
    ext_dir: PathBuf,
    tools: Vec<ToolDefinition>,
    handles: Arc<Mutex<Option<ProcessHandles>>>,
    request_id: Arc<AtomicU64>,
    widgets: Vec<WidgetDeclaration>,
    widget_tx: broadcast::Sender<WidgetEvent>,
    state: Arc<Mutex<ExtensionState>>,
}

impl ExtensionFeature {
    /// Create a new extension feature from already-handshaked process handles.
    pub fn new(
        name: String,
        ext_dir: PathBuf,
        tools: Vec<ToolDefinition>,
        widgets: Vec<WidgetDeclaration>,
        handles: ProcessHandles,
        state: ExtensionState,
    ) -> (Self, broadcast::Receiver<WidgetEvent>) {
        let (widget_tx, widget_rx) = broadcast::channel::<WidgetEvent>(100);
        let next_id = handles.next_id;
        (
            Self {
                name,
                ext_dir,
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
        let _ = state.save(&self.ext_dir);
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
            name: self.name.clone(),
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
        &self.name
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
            Ok(output) => Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: output.to_string(),
                }],
                details: json!({}),
            }),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("MethodNotFound") {
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!(
                                "Extension '{}' does not support tool execution. \
                                 The tool '{}' was advertised but cannot be called.",
                                self.name, tool_name
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
            spawn_native(&manifest, binary, widgets, state, resolved_secrets).await
        }
        RuntimeConfig::Oci { .. } => {
            let image = manifest.oci_image()?;
            spawn_container(&manifest, &image, widgets, state, resolved_secrets).await
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

/// Run the extension handshake sequence on a single process:
/// 1. `get_tools` — discover tools (required by contract)
/// 2. `bootstrap_secrets` — deliver secrets over pipe (never via env)
///
/// Returns handles with `next_id` advanced past the handshake, and the tool list.
async fn handshake(
    handles: &mut ProcessHandles,
    name: &str,
    resolved_secrets: &[(String, String)],
) -> Result<Vec<ToolDefinition>> {
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

    Ok(tools)
}

async fn spawn_native(
    manifest: &ExtensionManifest,
    binary: PathBuf,
    widgets: Vec<WidgetDeclaration>,
    state: ExtensionState,
    resolved_secrets: &[(String, String)],
) -> Result<SpawnedExtension> {
    let mut child = clean_command(&binary)
        .arg("--rpc")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let mut handles = ProcessHandles::new(stdin, stdout);

    let tools = handshake(&mut handles, &manifest.extension.name, resolved_secrets).await?;

    tracing::info!(
        name = %manifest.extension.name,
        binary = %binary.display(),
        tools = tools.len(),
        widgets = widgets.len(),
        secrets = resolved_secrets.len(),
        "spawned native extension"
    );

    let (feature, widget_rx) = ExtensionFeature::new(
        manifest.extension.name.clone(),
        binary
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf(),
        tools.clone(),
        widgets.clone(),
        handles,
        state,
    );

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
    image: &str,
    widgets: Vec<WidgetDeclaration>,
    state: ExtensionState,
    resolved_secrets: &[(String, String)],
) -> Result<SpawnedExtension> {
    let mut child = clean_command("podman")
        .args(["run", "--rm", "-i", image])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    let mut handles = ProcessHandles::new(stdin, stdout);

    let tools = handshake(&mut handles, &manifest.extension.name, resolved_secrets).await?;

    tracing::info!(
        name = %manifest.extension.name,
        image = image,
        tools = tools.len(),
        widgets = widgets.len(),
        secrets = resolved_secrets.len(),
        "spawned OCI extension"
    );

    let (feature, widget_rx) = ExtensionFeature::new(
        manifest.extension.name.clone(),
        PathBuf::new(),
        tools.clone(),
        widgets.clone(),
        handles,
        state,
    );

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

    #[test]
    fn extension_manifest_paths() {
        // Placeholder for integration tests
    }

    #[test]
    fn required_secret_check_detects_missing() {
        // Required secret not in resolved_secrets → missing
        let required = vec!["GITHUB_TOKEN".to_string()];
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
        let required = vec!["GITHUB_TOKEN".to_string()];
        let resolved = vec![("GITHUB_TOKEN".to_string(), "ghp_test".to_string())];
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
}

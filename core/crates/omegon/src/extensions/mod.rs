//! Extension spawning and process management.
//!
//! Handles both native (binary) and OCI (container) extensions.
//! All extensions communicate via JSON-RPC 2.0 over stdin/stdout.
//! Stateful widgets stream updates via separate TCP connection.

use omegon_traits::{Feature, ToolDefinition, ToolResult, ContentBlock};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, broadcast};
use tokio_util::sync::CancellationToken;

pub mod manifest;
pub mod state;
pub mod widgets;
pub use manifest::{ExtensionManifest, RuntimeConfig, WidgetConfig};
pub use state::{ExtensionState, StabilityMetrics};
pub use widgets::{WidgetDeclaration, WidgetEvent, ExtensionTabWidget};

/// Handles for communicating with an extension process.
pub struct ProcessHandles {
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
}

impl ProcessHandles {
    fn new(
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
    ) -> Self {
        Self {
            stdin,
            reader: BufReader::new(stdout),
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
    /// Create a new extension feature from process handles and pre-fetched tools.
    pub fn new(
        name: String,
        ext_dir: PathBuf,
        tools: Vec<ToolDefinition>,
        widgets: Vec<WidgetDeclaration>,
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
        state: ExtensionState,
    ) -> (Self, broadcast::Receiver<WidgetEvent>) {
        let (widget_tx, widget_rx) = broadcast::channel::<WidgetEvent>(100);
        (
            Self {
                name,
                ext_dir,
                tools,
                handles: Arc::new(Mutex::new(Some(ProcessHandles::new(stdin, stdout)))),
                request_id: Arc::new(AtomicU64::new(1)),
                widgets,
                widget_tx,
                state: Arc::new(Mutex::new(state)),
            },
            widget_rx,
        )
    }

    /// Send a JSON-RPC request and receive the response.
    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let mut handles = self.handles.lock().await;
        let handles = handles.as_mut().ok_or_else(|| anyhow!("extension process not running"))?;

        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        // Write request
        handles
            .stdin
            .write_all(format!("{}\n", request.to_string()).as_bytes())
            .await?;
        handles.stdin.flush().await?;

        // Read response
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
            if let Some(resp_id) = resp.get("id").and_then(|v| v.as_u64()) {
                if resp_id == id {
                    // Found our response
                    if let Some(result) = resp.get("result") {
                        return Ok(result.clone());
                    } else if let Some(error) = resp.get("error") {
                        return Err(anyhow!("RPC error: {}", error));
                    } else {
                        return Err(anyhow!("invalid RPC response"));
                    }
                }
            }
            // Continue reading (may be out-of-order notifications)
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
        let output = self.rpc_call("execute_tool", json!({
            "name": tool_name,
            "args": args,
        }))
        .await?;

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: output.to_string(),
            }],
            details: json!({}),
        })
    }
}

/// Result of spawning an extension: feature + widgets
pub struct SpawnedExtension {
    pub feature: Box<dyn Feature>,
    pub widgets: Vec<ExtensionTabWidget>,
    pub widget_rx: broadcast::Receiver<WidgetEvent>,
}

/// Spawn an extension from its manifest directory.
pub async fn spawn_from_manifest(ext_dir: &PathBuf) -> Result<SpawnedExtension> {
    let manifest = ExtensionManifest::from_extension_dir(ext_dir)?;

    // Load extension state
    let state = ExtensionState::load(ext_dir)?;

    // Parse widgets from manifest
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
            spawn_native(&manifest, ext_dir, &binary, widgets, state).await
        }
        RuntimeConfig::Oci { .. } => {
            let image = manifest.oci_image()?;
            spawn_container(&manifest, ext_dir, &image, widgets, state).await
        }
    }
}

async fn spawn_native(
    manifest: &ExtensionManifest,
    ext_dir: &PathBuf,
    binary: &PathBuf,
    widgets: Vec<WidgetDeclaration>,
    state: ExtensionState,
) -> Result<SpawnedExtension> {
    let mut child = tokio::process::Command::new(binary)
        .arg("--rpc")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

    // Create temporary feature to fetch tools
    let (temp_feature, _) = ExtensionFeature::new(
        manifest.extension.name.clone(),
        ext_dir.clone(),
        vec![],
        vec![],
        stdin,
        stdout,
        ExtensionState::default(),
    );

    // Fetch tools via RPC
    let tools: Vec<ToolDefinition> = temp_feature
        .rpc_call("get_tools", json!({}))
        .await
        .ok()
        .and_then(|v| serde_json::from_value::<Vec<ToolDefinition>>(v).ok())
        .unwrap_or_default();

    tracing::info!(
        name = %manifest.extension.name,
        binary = %binary.display(),
        tools = tools.len(),
        widgets = widgets.len(),
        "spawned native extension"
    );

    // Create final feature with tools and widgets
    // We need to re-spawn because we consumed the handles getting tools
    let mut child = tokio::process::Command::new(binary)
        .arg("--rpc")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

    let (feature, widget_rx) = ExtensionFeature::new(
        manifest.extension.name.clone(),
        ext_dir.clone(),
        tools,
        widgets.clone(),
        stdin,
        stdout,
        state,
    );

    // Convert widget declarations to tab widgets with initial data
    let mut tab_widgets: Vec<ExtensionTabWidget> = vec![];
    for widget in widgets {
        let mut tab_widget = ExtensionTabWidget::new(widget.id.clone(), widget.label, widget.renderer, widget.kind);
        
        // Fetch initial data for the widget
        if let Ok(data) = feature.rpc_call(&format!("get_{}", widget.id), json!({})).await {
            tab_widget.update(data);
        }
        
        tab_widgets.push(tab_widget);
    }

    Ok(SpawnedExtension {
        feature: Box::new(feature),
        widgets: tab_widgets,
        widget_rx,
    })
}

async fn spawn_container(
    manifest: &ExtensionManifest,
    ext_dir: &PathBuf,
    image: &str,
    widgets: Vec<WidgetDeclaration>,
    state: ExtensionState,
) -> Result<SpawnedExtension> {
    let mut child = tokio::process::Command::new("podman")
        .args(&["run", "--rm", "-i", image])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

    // Create temporary feature to fetch tools
    let (temp_feature, _) = ExtensionFeature::new(
        manifest.extension.name.clone(),
        ext_dir.clone(),
        vec![],
        vec![],
        stdin,
        stdout,
        ExtensionState::default(),
    );

    // Fetch tools via RPC
    let tools: Vec<ToolDefinition> = temp_feature
        .rpc_call("get_tools", json!({}))
        .await
        .ok()
        .and_then(|v| serde_json::from_value::<Vec<ToolDefinition>>(v).ok())
        .unwrap_or_default();

    tracing::info!(
        name = %manifest.extension.name,
        image = image,
        tools = tools.len(),
        widgets = widgets.len(),
        "spawned OCI extension"
    );

    // Re-spawn to get fresh handles
    let mut child = tokio::process::Command::new("podman")
        .args(&["run", "--rm", "-i", image])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

    let (feature, widget_rx) = ExtensionFeature::new(
        manifest.extension.name.clone(),
        ext_dir.clone(),
        tools,
        widgets.clone(),
        stdin,
        stdout,
        state,
    );

    // Convert widget declarations to tab widgets with initial data
    let mut tab_widgets: Vec<ExtensionTabWidget> = vec![];
    for widget in widgets {
        let mut tab_widget = ExtensionTabWidget::new(widget.id.clone(), widget.label, widget.renderer, widget.kind);
        
        // Fetch initial data for the widget
        if let Ok(data) = feature.rpc_call(&format!("get_{}", widget.id), json!({})).await {
            tab_widget.update(data);
        }
        
        tab_widgets.push(tab_widget);
    }

    Ok(SpawnedExtension {
        feature: Box::new(feature),
        widgets: tab_widgets,
        widget_rx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_manifest_paths() {
        // Placeholder for integration tests
    }
}

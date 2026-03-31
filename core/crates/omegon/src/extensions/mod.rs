//! Extension spawning and process management.
//!
//! Handles both native (binary) and OCI (container) extensions.
//! All extensions communicate via JSON-RPC 2.0 over stdin/stdout.

use omegon_traits::{Feature, ToolDefinition, ToolResult, ContentBlock};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub mod manifest;
pub use manifest::{ExtensionManifest, RuntimeConfig};

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
    tools: Vec<ToolDefinition>,
    handles: Arc<Mutex<Option<ProcessHandles>>>,
    request_id: Arc<AtomicU64>,
}

impl ExtensionFeature {
    /// Create a new extension feature from process handles and pre-fetched tools.
    pub fn new(
        name: String,
        tools: Vec<ToolDefinition>,
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
    ) -> Self {
        Self {
            name,
            tools,
            handles: Arc::new(Mutex::new(Some(ProcessHandles::new(stdin, stdout)))),
            request_id: Arc::new(AtomicU64::new(1)),
        }
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

/// Spawn an extension from its manifest directory.
pub async fn spawn_from_manifest(ext_dir: &PathBuf) -> Result<Box<dyn Feature>> {
    let manifest = ExtensionManifest::from_extension_dir(ext_dir)?;

    match manifest.runtime {
        RuntimeConfig::Native { .. } => {
            let binary = manifest.native_binary_path(ext_dir)?;
            spawn_native(&manifest, &binary).await
        }
        RuntimeConfig::Oci { .. } => {
            let image = manifest.oci_image()?;
            spawn_container(&manifest, &image).await
        }
    }
}

async fn spawn_native(
    manifest: &ExtensionManifest,
    binary: &PathBuf,
) -> Result<Box<dyn Feature>> {
    let mut child = tokio::process::Command::new(binary)
        .arg("--rpc")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

    // Create temporary feature to fetch tools
    let temp_feature = ExtensionFeature::new(
        manifest.extension.name.clone(),
        vec![],
        stdin,
        stdout,
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
        "spawned native extension"
    );

    // Create final feature with tools
    // We need to re-spawn because we consumed the handles getting tools
    let mut child = tokio::process::Command::new(binary)
        .arg("--rpc")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

    Ok(Box::new(ExtensionFeature::new(
        manifest.extension.name.clone(),
        tools,
        stdin,
        stdout,
    )))
}

async fn spawn_container(
    manifest: &ExtensionManifest,
    image: &str,
) -> Result<Box<dyn Feature>> {
    let mut child = tokio::process::Command::new("podman")
        .args(&["run", "--rm", "-i", image])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

    // Create temporary feature to fetch tools
    let temp_feature = ExtensionFeature::new(
        manifest.extension.name.clone(),
        vec![],
        stdin,
        stdout,
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

    Ok(Box::new(ExtensionFeature::new(
        manifest.extension.name.clone(),
        tools,
        stdin,
        stdout,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_manifest_paths() {
        // Placeholder for integration tests
    }
}

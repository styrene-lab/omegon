//! Extension manifest — declarative configuration for native, OCI, and MCP-capable extensions.
//!
//! Each extension lives in ~/.omegon/extensions/{name}/ and declares:
//! - Extension metadata (name, version, description)
//! - Runtime type (native binary or OCI image)
//! - Startup behavior (health checks, timeouts)
//! - Optional MCP configuration for remote-capable extensions
//!
//! # MCP-capable extensions
//!
//! Extensions that declare an `[mcp]` section can operate in two modes:
//!
//! 1. **Local** (default) — spawned as a child process, JSON-RPC over stdin/stdout.
//!    This is the native extension protocol. Full feature set: widgets, secrets,
//!    vox bridge, mind integration.
//!
//! 2. **Remote** — the extension runs as a long-lived MCP server on another machine.
//!    Omegon connects to it as an MCP client. Tools work identically, but widgets
//!    and vox bridge are unavailable (MCP has no equivalent).
//!
//! MCP is the **degradation path**: any native extension can be accessed remotely
//! via MCP when the full native protocol isn't available. The proper secure path
//! for remote extensions in production is Styrene mesh transport, which provides
//! identity, encryption, and trust — MCP over raw TCP/HTTP is the fallback for
//! development and non-Styrene environments.
//!
//! The extension loader resolves connection mode at startup:
//! - If `[mcp]` is declared AND a matching `[mcp_servers.<name>]` exists in project
//!   config pointing to a remote URL, connect via MCP.
//! - Otherwise, spawn locally per the `[runtime]` section.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level manifest structure.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtensionManifest {
    pub extension: ExtensionMetadata,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub startup: StartupConfig,
    #[serde(default)]
    pub widgets: std::collections::HashMap<String, WidgetConfig>,
    #[serde(default)]
    pub secrets: SecretsConfig,
    /// MCP configuration — if present, this extension supports remote access via MCP.
    /// See [`McpConfig`] for transport hierarchy and connection mode resolution.
    #[serde(default)]
    pub mcp: Option<McpConfig>,
    /// Typed configuration fields declared by the extension.
    /// Parsed from `[config.<field_name>]` tables in manifest.toml.
    #[serde(default)]
    pub config: std::collections::HashMap<String, omegon_extension::ConfigField>,
}

/// Extension metadata.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtensionMetadata {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
}

/// Runtime configuration — how to execute the extension.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum RuntimeConfig {
    #[serde(rename = "native")]
    Native {
        /// Path to binary relative to manifest directory (e.g., "target/release/scribe-rpc")
        binary: String,
    },
    #[serde(rename = "oci")]
    Oci {
        /// OCI image to run (e.g., "python-analyzer:latest")
        image: String,
    },
}

/// Startup configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StartupConfig {
    /// Health check: send this RPC method on startup, expect response within timeout_ms
    #[serde(default)]
    pub ping_method: Option<String>,
    /// Timeout for health check in milliseconds
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    5000
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            ping_method: Some("get_tools".to_string()),
            timeout_ms: 5000,
        }
    }
}

/// Secrets required by this extension.
/// Names declared here are preflighted at startup alongside the LLM provider key,
/// so extension subprocesses receive them via inherited process environment.
/// Names must match entries in omegon-secrets WELL_KNOWN_SECRET_ENVS or
/// operator-configured recipes.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SecretsConfig {
    /// Secrets that must be resolved before the extension is spawned.
    /// Example: ["GITHUB_TOKEN", "MY_API_KEY"]
    #[serde(default)]
    pub required: Vec<String>,
    /// Secrets that are optional (extension degrades gracefully without them).
    #[serde(default)]
    pub optional: Vec<String>,
}

/// MCP configuration — declares that this extension can be accessed remotely via MCP.
///
/// When present, the extension binary must support an MCP server mode (typically a
/// `serve` subcommand). The extension loader checks for a matching remote endpoint
/// in project config before falling back to local spawn.
///
/// # Transport hierarchy (most to least capable)
///
/// 1. **Native** (stdin/stdout JSON-RPC) — full features: tools, widgets, secrets, vox, mind
/// 2. **Styrene** (mesh transport) — full features + encrypted identity-verified comms (future)
/// 3. **MCP over HTTP** — tools only, no widgets/vox/mind. Development and interop fallback.
/// 4. **MCP over stdio** — tools only, local process. Useful for non-Omegon MCP clients.
///
/// # Example manifest
///
/// ```toml
/// [mcp]
/// transport = "http"
/// default_port = 9100
/// serve_subcommand = "serve"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpConfig {
    /// Preferred MCP transport when running remotely.
    #[serde(default)]
    pub transport: McpTransport,

    /// Default port for the MCP HTTP server when self-hosting.
    /// Used when launching with `<binary> <serve_subcommand>`.
    #[serde(default)]
    pub default_port: Option<u16>,

    /// Subcommand that starts the MCP server (default: "serve").
    /// The binary is invoked as `<binary> <serve_subcommand> [--port <default_port>]`.
    #[serde(default = "default_serve_subcommand")]
    pub serve_subcommand: String,
}

fn default_serve_subcommand() -> String {
    "serve".to_string()
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            transport: McpTransport::default(),
            default_port: None,
            serve_subcommand: default_serve_subcommand(),
        }
    }
}

/// MCP transport mode.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    /// MCP over stdin/stdout — for local MCP clients (Claude Code, Cursor, etc.)
    #[default]
    Stdio,
    /// MCP over HTTP — for remote access. Development/interop fallback.
    Http,
}

/// Widget configuration — declared per-widget in manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WidgetConfig {
    /// Human-readable label for the tab/modal
    pub label: String,
    /// Widget kind: "stateful" (tab) or "ephemeral" (modal)
    pub kind: String, // "stateful" | "ephemeral"
    /// How to render: "timeline", "tree", "table", "graph", etc.
    pub renderer: String,
    /// Optional description of the widget
    #[serde(default)]
    pub description: String,
}

impl ExtensionManifest {
    /// Load manifest from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: Self = toml::from_str(&content)
            .map_err(|e| anyhow!("failed to parse manifest at {}: {}", path.display(), e))?;
        Ok(manifest)
    }

    /// Load manifest from ~/.omegon/extensions/{name}/.
    pub fn from_extension_dir(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join("manifest.toml");
        Self::from_file(&manifest_path)
    }

    /// Resolve the binary path for native extensions.
    pub fn native_binary_path(&self, base_dir: &Path) -> Result<std::path::PathBuf> {
        match &self.runtime {
            RuntimeConfig::Native { binary } => {
                let resolved = base_dir.join(binary);
                if resolved.exists() {
                    Ok(resolved)
                } else {
                    Err(anyhow!(
                        "native extension binary not found: {} (resolved to {})",
                        binary,
                        resolved.display()
                    ))
                }
            }
            RuntimeConfig::Oci { .. } => Err(anyhow!("expected native runtime, got OCI")),
        }
    }

    /// Get OCI image name for container extensions.
    pub fn oci_image(&self) -> Result<String> {
        match &self.runtime {
            RuntimeConfig::Oci { image } => Ok(image.clone()),
            RuntimeConfig::Native { .. } => Err(anyhow!("expected OCI runtime, got native")),
        }
    }

    /// Check if this extension uses native runtime.
    pub fn is_native(&self) -> bool {
        matches!(self.runtime, RuntimeConfig::Native { .. })
    }

    /// Check if this extension uses OCI runtime.
    pub fn is_oci(&self) -> bool {
        matches!(self.runtime, RuntimeConfig::Oci { .. })
    }

    /// Check if this extension declares MCP capability (can run as a remote MCP server).
    pub fn is_mcp_capable(&self) -> bool {
        self.mcp.is_some()
    }

    /// Connection mode this extension should use, given optional remote endpoint config.
    ///
    /// Resolution order:
    /// 1. If a remote URL is provided AND the extension is MCP-capable → Remote MCP
    /// 2. Otherwise → Local spawn per runtime config
    ///
    /// The caller (extension loader) is responsible for checking project config for
    /// `[mcp_servers.<extension_name>]` and passing the URL here.
    pub fn connection_mode(&self, remote_url: Option<&str>) -> ConnectionMode {
        match (&self.mcp, remote_url) {
            (Some(mcp_config), Some(url)) => ConnectionMode::RemoteMcp {
                url: url.to_owned(),
                transport: mcp_config.transport.clone(),
            },
            _ => ConnectionMode::Local,
        }
    }
}

/// Resolved connection mode for an extension at startup.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionMode {
    /// Spawn locally as a child process (native or OCI), JSON-RPC over stdin/stdout.
    /// Full feature set: tools, widgets, secrets, vox bridge, mind.
    Local,
    /// Connect to a remote MCP server. Tools only — widgets, vox bridge, and mind
    /// are unavailable over MCP.
    RemoteMcp {
        url: String,
        transport: McpTransport,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_native_manifest() {
        let toml = r#"
[extension]
name = "scribe-rpc"
version = "0.1.0"
description = "Engagement tracking"

[runtime]
type = "native"
binary = "target/release/scribe-rpc"

[startup]
ping_method = "get_tools"
timeout_ms = 5000

[widgets.timeline]
label = "Work Timeline"
kind = "stateful"
renderer = "timeline"
description = "Timeline of engagement activity"
"#;
        let manifest: ExtensionManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.extension.name, "scribe-rpc");
        assert!(manifest.is_native());
        assert!(!manifest.is_oci());
        assert_eq!(manifest.widgets.len(), 1);
        assert_eq!(manifest.widgets["timeline"].label, "Work Timeline");
        assert!(manifest.secrets.required.is_empty());
    }

    #[test]
    fn parse_manifest_with_secrets() {
        let toml = r#"
[extension]
name = "scribe-rpc"
version = "0.1.0"

[runtime]
type = "native"
binary = "bin/scribe-rpc"

[secrets]
required = ["GITHUB_TOKEN"]
optional = ["SCRIBE_API_KEY"]
"#;
        let manifest: ExtensionManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.secrets.required, vec!["GITHUB_TOKEN"]);
        assert_eq!(manifest.secrets.optional, vec!["SCRIBE_API_KEY"]);
    }

    #[test]
    fn parse_oci_manifest() {
        let toml = r#"
[extension]
name = "python-analyzer"
version = "0.2.0"
description = "Python analysis extension"

[runtime]
type = "oci"
image = "python-analyzer:latest"
"#;
        let manifest: ExtensionManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.extension.name, "python-analyzer");
        assert!(!manifest.is_native());
        assert!(manifest.is_oci());
        assert_eq!(manifest.oci_image().unwrap(), "python-analyzer:latest");
    }

    #[test]
    fn parse_mcp_capable_manifest() {
        let toml = r#"
[extension]
name = "viz"
version = "0.1.0"
description = "Broadcast graphics and display surface control"

[runtime]
type = "native"
binary = "target/release/viz"

[startup]
ping_method = "get_tools"
timeout_ms = 5000

[mcp]
transport = "http"
default_port = 9100
serve_subcommand = "serve"

[widgets.display]
label = "Display"
kind = "stateful"
renderer = "table"
description = "Active CasparCG channels and layers"
"#;
        let manifest: ExtensionManifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.extension.name, "viz");
        assert!(manifest.is_native());
        assert!(manifest.is_mcp_capable());

        let mcp = manifest.mcp.as_ref().unwrap();
        assert_eq!(mcp.transport, McpTransport::Http);
        assert_eq!(mcp.default_port, Some(9100));
        assert_eq!(mcp.serve_subcommand, "serve");

        // Widgets still declared — available in local mode, not in remote MCP mode
        assert_eq!(manifest.widgets.len(), 1);
    }

    #[test]
    fn mcp_capable_without_mcp_section_is_local_only() {
        let toml = r#"
[extension]
name = "scry"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/scry"
"#;
        let manifest: ExtensionManifest = toml::from_str(toml).unwrap();
        assert!(!manifest.is_mcp_capable());
        assert_eq!(manifest.connection_mode(None), ConnectionMode::Local);
        // Even with a URL, no MCP section means local
        assert_eq!(
            manifest.connection_mode(Some("http://192.168.0.13:9200")),
            ConnectionMode::Local
        );
    }

    #[test]
    fn connection_mode_resolution() {
        let toml = r#"
[extension]
name = "viz"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/viz"

[mcp]
transport = "http"
default_port = 9100
"#;
        let manifest: ExtensionManifest = toml::from_str(toml).unwrap();

        // No remote URL → local
        assert_eq!(manifest.connection_mode(None), ConnectionMode::Local);

        // Remote URL + MCP capable → remote
        let mode = manifest.connection_mode(Some("http://192.168.0.13:9100"));
        assert_eq!(
            mode,
            ConnectionMode::RemoteMcp {
                url: "http://192.168.0.13:9100".to_owned(),
                transport: McpTransport::Http,
            }
        );
    }

    #[test]
    fn mcp_stdio_transport_default() {
        let toml = r#"
[extension]
name = "viz"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/viz"

[mcp]
"#;
        let manifest: ExtensionManifest = toml::from_str(toml).unwrap();
        assert!(manifest.is_mcp_capable());
        let mcp = manifest.mcp.as_ref().unwrap();
        assert_eq!(mcp.transport, McpTransport::Stdio);
        assert_eq!(mcp.default_port, None);
        assert_eq!(mcp.serve_subcommand, "serve");
    }
}

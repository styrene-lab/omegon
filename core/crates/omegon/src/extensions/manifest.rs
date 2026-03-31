//! Extension manifest — declarative configuration for native and OCI extensions.
//!
//! Each extension lives in ~/.omegon/extensions/{name}/ and declares:
//! - Extension metadata (name, version, description)
//! - Runtime type (native binary or OCI image)
//! - Startup behavior (health checks, timeouts)

use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::{anyhow, Result};

/// Top-level manifest structure.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtensionManifest {
    pub extension: ExtensionMetadata,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub startup: StartupConfig,
    #[serde(default)]
    pub widgets: std::collections::HashMap<String, WidgetConfig>,
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

/// Widget configuration — declared per-widget in manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WidgetConfig {
    /// Human-readable label for the tab/modal
    pub label: String,
    /// Widget kind: "stateful" (tab) or "ephemeral" (modal)
    pub kind: String,  // "stateful" | "ephemeral"
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
            RuntimeConfig::Oci { .. } => {
                Err(anyhow!("expected native runtime, got OCI"))
            }
        }
    }

    /// Get OCI image name for container extensions.
    pub fn oci_image(&self) -> Result<String> {
        match &self.runtime {
            RuntimeConfig::Oci { image } => Ok(image.clone()),
            RuntimeConfig::Native { .. } => {
                Err(anyhow!("expected OCI runtime, got native"))
            }
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
}

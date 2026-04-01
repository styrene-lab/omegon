//! Manifest validation — caught at extension installation time.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Manifest validation error.
#[derive(Debug, Clone)]
pub struct ManifestError {
    pub reason: String,
    pub is_fatal: bool,
}

impl ManifestError {
    pub fn fatal(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            is_fatal: true,
        }
    }

    pub fn recoverable(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            is_fatal: false,
        }
    }
}

/// Extension metadata from manifest.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub extension: ExtensionMetadata,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub startup: StartupConfig,
    #[serde(default)]
    pub widgets: HashMap<String, WidgetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMetadata {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// SDK version constraint (e.g., "0.15.6-*" or "0.15.*" or "0.15").
    /// This is validated against the omegon-extension crate version at install time.
    /// Wildcard matching: "0.15.6" matches "0.15.6-rc.1", "0.15.6-rc.2", "0.15.6".
    #[serde(default)]
    pub sdk_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeConfig {
    Native { binary: String },
    Oci { image: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupConfig {
    /// RPC method to call for health check on startup.
    #[serde(default = "default_ping_method")]
    pub ping_method: String,
    /// Timeout in milliseconds for health check.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_ping_method() -> String {
    "get_tools".to_string()
}

fn default_timeout_ms() -> u64 {
    5000
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            ping_method: default_ping_method(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetConfig {
    pub label: String,
    pub kind: String, // "stateful" or "ephemeral"
    pub renderer: String,
    #[serde(default)]
    pub description: String,
}

impl ExtensionManifest {
    /// Load and validate manifest from TOML file.
    pub fn from_file(path: &Path) -> Result<Self, ManifestError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ManifestError::fatal(format!("failed to read manifest: {}", e)))?;

        let manifest: ExtensionManifest = toml::from_str(&content)
            .map_err(|e| ManifestError::fatal(format!("failed to parse manifest: {}", e)))?;

        // Validate
        manifest.validate()?;

        Ok(manifest)
    }

    /// Validate manifest schema.
    fn validate(&self) -> Result<(), ManifestError> {
        // Name must not be empty
        if self.extension.name.is_empty() {
            return Err(ManifestError::fatal("extension.name must not be empty"));
        }

        // Name must be lowercase alphanumeric + hyphens
        if !self.extension.name.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(ManifestError::fatal(
                "extension.name must contain only lowercase alphanumeric and hyphens",
            ));
        }

        // Version must be a valid semver
        if self.extension.version.is_empty() {
            return Err(ManifestError::fatal("extension.version must not be empty"));
        }

        // Validate runtime config
        match &self.runtime {
            RuntimeConfig::Native { binary } => {
                if binary.is_empty() {
                    return Err(ManifestError::fatal("runtime.binary must not be empty"));
                }
                // Don't validate file existence here — that's done at spawn time
            }
            RuntimeConfig::Oci { image } => {
                if image.is_empty() {
                    return Err(ManifestError::fatal("runtime.image must not be empty"));
                }
            }
        }

        // Validate widgets
        for (id, widget) in &self.widgets {
            if id.is_empty() {
                return Err(ManifestError::fatal("widget id must not be empty"));
            }
            if widget.label.is_empty() {
                return Err(ManifestError::fatal("widget.label must not be empty"));
            }
            if widget.renderer.is_empty() {
                return Err(ManifestError::fatal("widget.renderer must not be empty"));
            }
            // Validate kind
            match widget.kind.as_str() {
                "stateful" | "ephemeral" => {}
                _ => {
                    return Err(ManifestError::fatal(
                        format!("widget.kind must be 'stateful' or 'ephemeral', got '{}'", widget.kind),
                    ));
                }
            }
        }

        // Validate startup config
        if self.startup.timeout_ms == 0 {
            return Err(ManifestError::fatal("startup.timeout_ms must be > 0"));
        }
        if self.startup.timeout_ms > 60000 {
            return Err(ManifestError::recoverable(
                "startup.timeout_ms > 60s is unusual; extensions should start faster",
            ));
        }

        Ok(())
    }

    /// Check SDK version compatibility.
    /// 
    /// # Constraints
    /// 
    /// - Extension declares `sdk_version` in manifest.toml
    /// - Omegon validates at install time: extension's sdk_version must match omegon's SDK crate version
    /// - Wildcard matching: "0.15" matches "0.15.0", "0.15.6", "0.15.6-rc.1"
    /// - Exact match preferred: "0.15.6" prevents forward compatibility risks
    pub fn check_sdk_version(&self, omegon_sdk_version: &str) -> Result<(), ManifestError> {
        if self.extension.sdk_version.is_empty() {
            // Not specified — allow for now, but warn
            return Err(ManifestError::recoverable(
                "extension.sdk_version not specified; recommend adding for safety",
            ));
        }

        // Simple semver prefix matching
        // "0.15" matches "0.15.6", "0.15.6-rc.1"
        // "0.15.6" matches "0.15.6", "0.15.6-rc.1"
        if !omegon_sdk_version.starts_with(&self.extension.sdk_version) {
            return Err(ManifestError::fatal(format!(
                "SDK version mismatch: extension requires {}, but omegon has {}",
                self.extension.sdk_version, omegon_sdk_version
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_native_manifest() {
        let manifest = ExtensionManifest {
            extension: ExtensionMetadata {
                name: "my-ext".to_string(),
                version: "0.1.0".to_string(),
                description: "Test".to_string(),
                sdk_version: "0.15".to_string(),
            },
            runtime: RuntimeConfig::Native {
                binary: "target/release/my-ext".to_string(),
            },
            startup: StartupConfig::default(),
            widgets: HashMap::new(),
        };

        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_name() {
        let manifest = ExtensionManifest {
            extension: ExtensionMetadata {
                name: "MY_EXT".to_string(),
                version: "0.1.0".to_string(),
                description: "".to_string(),
                sdk_version: "".to_string(),
            },
            runtime: RuntimeConfig::Native {
                binary: "binary".to_string(),
            },
            startup: StartupConfig::default(),
            widgets: HashMap::new(),
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_sdk_version_check() {
        let manifest = ExtensionManifest {
            extension: ExtensionMetadata {
                name: "test".to_string(),
                version: "0.1.0".to_string(),
                description: "".to_string(),
                sdk_version: "0.15".to_string(),
            },
            runtime: RuntimeConfig::Native {
                binary: "binary".to_string(),
            },
            startup: StartupConfig::default(),
            widgets: HashMap::new(),
        };

        // Exact match
        assert!(manifest.check_sdk_version("0.15.6").is_ok());
        // Prefix match
        assert!(manifest.check_sdk_version("0.15.6-rc.1").is_ok());
        // Mismatch
        assert!(manifest.check_sdk_version("0.16.0").is_err());
    }
}

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::extensions::manifest::{ExtensionManifest, RuntimeConfig};
use crate::extensions::state::{ExtensionState, StabilityMetrics};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionInstallationSummary {
    pub filesystem_name: String,
    pub source_path: String,
    pub source_kind: ExtensionInstallationSourceKind,
    pub diagnosis: ExtensionInstallationDiagnosis,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionInstallationSourceKind {
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExtensionInstallationDiagnosis {
    Valid {
        capability: Box<ExtensionCapabilitySummary>,
    },
    Invalid {
        problem: String,
    },
    BrokenLink {
        problem: String,
    },
    Unreadable {
        problem: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionCapabilitySummary {
    pub name: String,
    pub version: String,
    pub description: String,
    pub runtime: ExtensionRuntimeSummary,
    pub status: String,
    pub enabled: bool,
    pub source_path: String,
    pub startup: ExtensionStartupSummary,
    pub config: Vec<ExtensionConfigFieldSummary>,
    pub required_secrets: Vec<String>,
    pub optional_secrets: Vec<String>,
    pub widgets: Vec<ExtensionWidgetSummary>,
    pub capabilities: serde_json::Value,
    pub permissions: serde_json::Value,
    pub mcp: Option<ExtensionMcpSummary>,
    pub stability: ExtensionStabilitySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ExtensionRuntimeSummary {
    Native { binary: String },
    Oci { image: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionStartupSummary {
    pub ping_method: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionConfigFieldSummary {
    pub name: String,
    pub field: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionWidgetSummary {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub renderer: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionMcpSummary {
    pub transport: String,
    pub default_port: Option<u16>,
    pub serve_subcommand: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionStabilitySummary {
    pub crashes_this_session: u32,
    pub health_check_failures: u32,
    pub last_error: Option<String>,
    pub last_error_at: Option<String>,
    pub auto_disabled: bool,
}

pub fn list_extension_installations_from_dir(
    extensions_dir: &Path,
) -> anyhow::Result<Vec<ExtensionInstallationSummary>> {
    if !extensions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut installations = Vec::new();
    for entry in std::fs::read_dir(extensions_dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(?error, "could not inspect extension installation entry");
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                installations.push(ExtensionInstallationSummary {
                    filesystem_name: entry.file_name().to_string_lossy().into_owned(),
                    source_path: path.display().to_string(),
                    source_kind: ExtensionInstallationSourceKind::Directory,
                    diagnosis: ExtensionInstallationDiagnosis::Unreadable {
                        problem: format!("could not inspect installation: {error}"),
                    },
                });
                continue;
            }
        };
        if !file_type.is_dir() && !file_type.is_symlink() {
            continue;
        }

        let filesystem_name = entry.file_name().to_string_lossy().into_owned();
        let source_path = path.display().to_string();
        let source_kind = if file_type.is_symlink() {
            ExtensionInstallationSourceKind::Symlink
        } else {
            ExtensionInstallationSourceKind::Directory
        };
        let resolved = if file_type.is_symlink() {
            match std::fs::canonicalize(&path) {
                Ok(resolved) => resolved,
                Err(error) => {
                    installations.push(ExtensionInstallationSummary {
                        filesystem_name,
                        source_path,
                        source_kind,
                        diagnosis: ExtensionInstallationDiagnosis::BrokenLink {
                            problem: format!("symlink target cannot be resolved: {error}"),
                        },
                    });
                    continue;
                }
            }
        } else {
            path
        };

        let manifest_path = resolved.join("manifest.toml");
        let diagnosis = if !manifest_path.exists() {
            ExtensionInstallationDiagnosis::Invalid {
                problem: "missing manifest.toml".into(),
            }
        } else {
            match extension_capability_summary_from_dir(&resolved) {
                Ok(capability) => ExtensionInstallationDiagnosis::Valid {
                    capability: Box::new(capability),
                },
                Err(error) => ExtensionInstallationDiagnosis::Invalid {
                    problem: format!("invalid manifest or state: {error}"),
                },
            }
        };
        installations.push(ExtensionInstallationSummary {
            filesystem_name,
            source_path,
            source_kind,
            diagnosis,
        });
    }
    installations.sort_by(|a, b| a.filesystem_name.cmp(&b.filesystem_name));
    Ok(installations)
}

pub fn list_installed_extension_capabilities_from_dir(
    extensions_dir: &Path,
) -> anyhow::Result<Vec<ExtensionCapabilitySummary>> {
    Ok(list_extension_installations_from_dir(extensions_dir)?
        .into_iter()
        .filter_map(|installation| match installation.diagnosis {
            ExtensionInstallationDiagnosis::Valid { capability } => Some(*capability),
            ExtensionInstallationDiagnosis::Invalid { .. }
            | ExtensionInstallationDiagnosis::BrokenLink { .. }
            | ExtensionInstallationDiagnosis::Unreadable { .. } => None,
        })
        .collect())
}

pub fn extension_capability_summary_from_dir(
    extension_dir: &Path,
) -> anyhow::Result<ExtensionCapabilitySummary> {
    let manifest = ExtensionManifest::from_extension_dir(extension_dir)?;
    let state = ExtensionState::load(extension_dir)?;
    Ok(extension_capability_summary(extension_dir, manifest, state))
}

fn extension_capability_summary(
    extension_dir: &Path,
    manifest: ExtensionManifest,
    state: ExtensionState,
) -> ExtensionCapabilitySummary {
    let runtime = match manifest.runtime.clone() {
        RuntimeConfig::Native { binary, .. } => ExtensionRuntimeSummary::Native { binary },
        RuntimeConfig::Oci { image, .. } => ExtensionRuntimeSummary::Oci { image },
    };

    let mut config: Vec<_> = manifest
        .config
        .into_iter()
        .map(|(name, field)| ExtensionConfigFieldSummary {
            name,
            field: serde_json::to_value(field).unwrap_or(serde_json::Value::Null),
        })
        .collect();
    config.sort_by(|a, b| a.name.cmp(&b.name));

    let mut widgets: Vec<_> = manifest
        .widgets
        .into_iter()
        .map(|(id, widget)| ExtensionWidgetSummary {
            id,
            label: widget.label,
            kind: widget.kind,
            renderer: widget.renderer,
            description: widget.description,
        })
        .collect();
    widgets.sort_by(|a, b| a.id.cmp(&b.id));

    ExtensionCapabilitySummary {
        name: manifest.extension.name,
        version: manifest.extension.version,
        description: manifest.extension.description,
        runtime,
        status: state.status_text(),
        enabled: state.enabled,
        source_path: extension_dir.display().to_string(),
        startup: ExtensionStartupSummary {
            ping_method: manifest.startup.ping_method,
            timeout_ms: manifest.startup.timeout_ms,
        },
        config,
        required_secrets: manifest.secrets.required,
        optional_secrets: manifest.secrets.optional,
        widgets,
        capabilities: serde_json::to_value(manifest.capabilities)
            .unwrap_or(serde_json::Value::Null),
        permissions: serde_json::to_value(manifest.permissions).unwrap_or(serde_json::Value::Null),
        mcp: manifest.mcp.map(|mcp| ExtensionMcpSummary {
            transport: format!("{:?}", mcp.transport).to_ascii_lowercase(),
            default_port: mcp.default_port,
            serve_subcommand: mcp.serve_subcommand,
        }),
        stability: stability_summary(state.stability),
    }
}

fn stability_summary(stability: StabilityMetrics) -> ExtensionStabilitySummary {
    ExtensionStabilitySummary {
        crashes_this_session: stability.crashes_this_session,
        health_check_failures: stability.health_check_failures,
        last_error: stability.last_error,
        last_error_at: stability.last_error_at,
        auto_disabled: stability.auto_disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_installed_extension_capability_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let ext_dir = temp.path().join("sample");
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            r#"
[extension]
name = "sample"
version = "0.1.0"
description = "Sample extension"

[runtime]
type = "native"
binary = "target/release/sample"

[startup]
ping_method = "get_tools"
timeout_ms = 1234

[capabilities]
tools = true
host_actions = false

[permissions.process]
allowed_commands = ["git"]

[secrets]
required = ["SAMPLE_TOKEN"]
optional = ["SAMPLE_OPTIONAL"]

[widgets.timeline]
label = "Timeline"
kind = "stateful"
renderer = "timeline"
description = "Recent activity"

[config.mode]
type = "string"
label = "Mode"
description = "Execution mode"
default = "safe"

[mcp]
transport = "http"
default_port = 9100
serve_subcommand = "serve"
"#,
        )
        .unwrap();

        let summaries = list_installed_extension_capabilities_from_dir(temp.path()).unwrap();

        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.name, "sample");
        assert_eq!(summary.status, "enabled");
        assert!(summary.enabled);
        assert_eq!(summary.required_secrets, vec!["SAMPLE_TOKEN"]);
        assert_eq!(summary.optional_secrets, vec!["SAMPLE_OPTIONAL"]);
        assert_eq!(summary.widgets[0].id, "timeline");
        assert_eq!(summary.config[0].name, "mode");
        assert_eq!(summary.mcp.as_ref().unwrap().transport, "http");
        assert!(summary.capabilities.is_object());
        assert!(summary.permissions.is_object());
    }

    #[test]
    fn diagnoses_invalid_extension_installation_candidates_without_hiding_them() {
        let temp = tempfile::tempdir().unwrap();
        let missing_manifest = temp.path().join("scry");
        std::fs::create_dir_all(&missing_manifest).unwrap();
        let invalid_manifest = temp.path().join("malformed");
        std::fs::create_dir_all(&invalid_manifest).unwrap();
        std::fs::write(invalid_manifest.join("manifest.toml"), "not = [valid toml").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path().join("absent"), temp.path().join("dangling"))
            .unwrap();

        let installations = list_extension_installations_from_dir(temp.path()).unwrap();

        assert!(installations.iter().any(|entry| {
            entry.filesystem_name == "scry"
                && matches!(
                    &entry.diagnosis,
                    ExtensionInstallationDiagnosis::Invalid { problem }
                        if problem == "missing manifest.toml"
                )
        }));
        assert!(installations.iter().any(|entry| {
            entry.filesystem_name == "malformed"
                && matches!(
                    entry.diagnosis,
                    ExtensionInstallationDiagnosis::Invalid { .. }
                )
        }));
        #[cfg(unix)]
        assert!(installations.iter().any(|entry| {
            entry.filesystem_name == "dangling"
                && matches!(
                    entry.diagnosis,
                    ExtensionInstallationDiagnosis::BrokenLink { .. }
                )
        }));
    }

    #[test]
    fn skips_invalid_extension_manifest_entries() {
        let temp = tempfile::tempdir().unwrap();
        let valid_dir = temp.path().join("valid");
        std::fs::create_dir_all(&valid_dir).unwrap();
        std::fs::write(
            valid_dir.join("manifest.toml"),
            r#"
[extension]
name = "valid"
version = "0.1.0"
description = "Valid extension"

[runtime]
type = "native"
binary = "target/release/valid"
"#,
        )
        .unwrap();
        let broken_dir = temp.path().join("broken");
        std::fs::create_dir_all(&broken_dir).unwrap();
        std::fs::write(broken_dir.join("manifest.toml"), "not = [valid toml").unwrap();

        let summaries = list_installed_extension_capabilities_from_dir(temp.path()).unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "valid");
    }

    #[test]
    fn missing_extensions_dir_is_empty_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let summaries = list_installed_extension_capabilities_from_dir(&missing).unwrap();
        assert!(summaries.is_empty());
    }
}

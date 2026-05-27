//! Manifest validation — caught at extension installation time.

use crate::Capabilities;
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

/// Declarative operator-surface contribution envelope.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiConfig {
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub commands: Vec<UiCommand>,
    #[serde(default)]
    pub status_items: Vec<UiStatusItem>,
    #[serde(default)]
    pub surfaces: Vec<UiSurface>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiCommand {
    pub id: String,
    pub title: String,
    pub slash: String,
    pub tool: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiStatusItem {
    pub id: String,
    pub title: String,
    pub refresh_tool: String,
    pub interval_ms: u64,
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSurface {
    pub id: String,
    pub title: String,
    pub surface_type: String,
    pub rendering: String,
    #[serde(default)]
    pub preferred_placements: Vec<String>,
    #[serde(default)]
    pub open_tool: Option<String>,
    #[serde(default)]
    pub status_tool: Option<String>,
    #[serde(default)]
    pub view: Option<UiPrimitiveView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPrimitiveView {
    pub primitive: String,
    pub data_tool: String,
    #[serde(default)]
    pub item: Option<UiListItemTemplate>,
    #[serde(default)]
    pub actions: Vec<UiPrimitiveAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiListItemTemplate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPrimitiveAction {
    pub id: String,
    pub title: String,
    pub tool: String,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub extension: ExtensionMetadata,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub startup: StartupConfig,
    #[serde(default)]
    pub widgets: HashMap<String, WidgetConfig>,
    #[serde(default)]
    pub mind: MindConfig,
    #[serde(default)]
    pub config: HashMap<String, ConfigField>,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub permissions: ManifestPermissions,
}

/// Permission declarations from manifest.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestPermissions {
    #[serde(default)]
    pub host_actions: HostActionPermissions,
}

/// HostAction permission declarations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostActionPermissions {
    /// Versioned action types the extension may request.
    #[serde(default)]
    pub allowed: Vec<String>,
    /// Policy for `terminal.create@1` requests.
    #[serde(default)]
    pub terminal_create: TerminalCreatePermissions,
}

/// Manifest policy for `terminal.create@1`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TerminalCreatePermissions {
    /// Whether interactive terminal actions are permitted.
    #[serde(default)]
    pub interactive: bool,
    /// Allowed executable names. Empty means no commands are allowed by manifest policy.
    #[serde(default)]
    pub allowed_commands: Vec<String>,
    /// Allowed cwd roots. Values may include host-expanded tokens such as `${workspace}`.
    #[serde(default)]
    pub allowed_cwd_roots: Vec<String>,
    /// Environment variable names the extension may pass through.
    #[serde(default)]
    pub allow_env: Vec<String>,
}

impl HostActionPermissions {
    /// Return true when the manifest allows the given versioned HostAction type.
    pub fn allows_action_type(&self, action_type: &str) -> bool {
        self.allowed.iter().any(|allowed| allowed == action_type)
    }
}

/// A declared configuration field in the extension manifest.
///
/// Extensions declare their config requirements in `[config.<field_name>]`
/// tables. The host resolves values from per-extension config files and
/// delivers them via `bootstrap_config` RPC after initialization.
///
/// Example manifest:
/// ```toml
/// [config.signal_phone]
/// type = "string"
/// label = "Signal phone number"
/// description = "E.164 format, e.g. +14155551234"
/// required = true
///
/// [config.webhook_enabled]
/// type = "boolean"
/// label = "Enable webhook"
/// default = "false"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    /// Field type — determines validation and UI widget.
    #[serde(rename = "type")]
    pub field_type: ConfigFieldType,

    /// Human-readable label for settings UI.
    pub label: String,

    /// Longer description / help text.
    #[serde(default)]
    pub description: String,

    /// Whether the field must have a value before the extension can start.
    #[serde(default)]
    pub required: bool,

    /// Default value (as string — parsed according to `field_type`).
    #[serde(default)]
    pub default: Option<String>,

    /// For `string` fields: regex pattern the value must match.
    #[serde(default)]
    pub pattern: Option<String>,

    /// For `string` fields: hint shown as input placeholder.
    #[serde(default)]
    pub placeholder: Option<String>,

    /// For `enum` fields: allowed values.
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFieldType {
    String,
    Number,
    Boolean,
    Enum,
    /// Multi-line text (rendered as textarea).
    Text,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MindConfig {
    /// Whether this extension has a persistent mind
    #[serde(default)]
    pub enabled: bool,

    /// Description of the mind for UI/documentation
    #[serde(default)]
    pub description: String,

    /// Maximum facts to keep (optional, default: unlimited)
    #[serde(default)]
    pub max_facts: Option<usize>,

    /// Retention policy: delete facts older than this many days
    #[serde(default)]
    pub retention_days: Option<u32>,
}

impl ExtensionManifest {
    /// Return true when manifest HostAction permissions allow this action type.
    pub fn allows_host_action_type(&self, action_type: &str) -> bool {
        self.permissions
            .host_actions
            .allows_action_type(action_type)
    }

    /// Manifest policy for `terminal.create@1` actions.
    pub fn terminal_create_permissions(&self) -> &TerminalCreatePermissions {
        &self.permissions.host_actions.terminal_create
    }

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
        if !self
            .extension
            .name
            .chars()
            .all(|c| (c.is_ascii_lowercase() || c.is_ascii_digit()) || c == '-')
        {
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
                    return Err(ManifestError::fatal(format!(
                        "widget.kind must be 'stateful' or 'ephemeral', got '{}'",
                        widget.kind
                    )));
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

        // Validate config fields
        for (name, field) in &self.config {
            if name.is_empty() {
                return Err(ManifestError::fatal("config field name must not be empty"));
            }
            if field.label.is_empty() {
                return Err(ManifestError::fatal(format!(
                    "config.{name}.label must not be empty"
                )));
            }
            if field.field_type == ConfigFieldType::Enum && field.values.is_empty() {
                return Err(ManifestError::fatal(format!(
                    "config.{name} is type 'enum' but declares no values"
                )));
            }
            if let Some(ref pattern) = field.pattern {
                if pattern.is_empty() {
                    return Err(ManifestError::fatal(format!(
                        "config.{name}.pattern must not be empty if specified"
                    )));
                }
            }
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
            mind: MindConfig::default(),
            config: HashMap::new(),
            capabilities: Capabilities::default(),
            ui: UiConfig::default(),
            permissions: ManifestPermissions::default(),
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
            mind: MindConfig::default(),
            config: HashMap::new(),
            capabilities: Capabilities::default(),
            ui: UiConfig::default(),
            permissions: ManifestPermissions::default(),
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
            mind: MindConfig::default(),
            config: HashMap::new(),
            capabilities: Capabilities::default(),
            ui: UiConfig::default(),
            permissions: ManifestPermissions::default(),
        };

        // Exact match
        assert!(manifest.check_sdk_version("0.15.6").is_ok());
        // Prefix match
        assert!(manifest.check_sdk_version("0.15.6-rc.1").is_ok());
        // Mismatch
        assert!(manifest.check_sdk_version("0.16.0").is_err());
    }

    #[test]
    fn test_host_action_capabilities_default_false_for_legacy_manifest() {
        let toml_str = r#"
[extension]
name = "reader"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/reader"
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert!(!manifest.capabilities.host_actions);
        assert!(!manifest.capabilities.host_action_execution);
        assert!(manifest.permissions.host_actions.allowed.is_empty());
    }

    #[test]
    fn test_host_action_capabilities_parse_from_toml() {
        let toml_str = r#"
[extension]
name = "reader"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/reader"

[capabilities]
host_actions = true
host_action_execution = true
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.capabilities.host_actions);
        assert!(manifest.capabilities.host_action_execution);
    }

    #[test]
    fn test_host_action_permissions_parse_from_toml() {
        let toml_str = r#"
[extension]
name = "reader"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/reader"

[permissions.host_actions]
allowed = ["terminal.create@1"]

[permissions.host_actions.terminal_create]
interactive = true
allowed_commands = ["bookokrat"]
allowed_cwd_roots = ["${workspace}"]
allow_env = []
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(
            manifest.permissions.host_actions.allowed,
            vec!["terminal.create@1"]
        );
        let terminal = &manifest.permissions.host_actions.terminal_create;
        assert!(terminal.interactive);
        assert_eq!(terminal.allowed_commands, vec!["bookokrat"]);
        assert_eq!(terminal.allowed_cwd_roots, vec!["${workspace}"]);
        assert!(terminal.allow_env.is_empty());
    }

    #[test]
    fn test_host_action_permission_helpers_expose_runtime_policy() {
        let toml_str = r#"
[extension]
name = "reader"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/reader"

[permissions.host_actions]
allowed = ["terminal.create@1"]

[permissions.host_actions.terminal_create]
interactive = true
allowed_commands = ["bookokrat"]
allowed_cwd_roots = ["${workspace}"]
allow_env = ["BOOKOKRAT_THEME"]
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.allows_host_action_type("terminal.create@1"));
        assert!(!manifest.allows_host_action_type("file.open@1"));

        let terminal = manifest.terminal_create_permissions();
        assert!(terminal.interactive);
        assert_eq!(terminal.allowed_commands, vec!["bookokrat"]);
        assert_eq!(terminal.allowed_cwd_roots, vec!["${workspace}"]);
        assert_eq!(terminal.allow_env, vec!["BOOKOKRAT_THEME"]);
    }

    #[test]
    fn test_ui_contributions_parse_from_toml() {
        let toml_str = r#"
[extension]
name = "reader"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/reader"

[capabilities]
ui_contributions = true

[ui]
namespace = "reader"
description = "Document reader surfaces"

[[ui.commands]]
id = "open"
title = "Open Reader"
slash = "/reader open"
tool = "reader_open"

[[ui.status_items]]
id = "status"
title = "Reader"
refresh_tool = "reader_status"
interval_ms = 30000
template = "{state}"

[[ui.surfaces]]
id = "reader"
title = "Reader"
surface_type = "document_reader"
rendering = "delegated"
preferred_placements = ["side_pane", "new_tab", "external"]
open_tool = "reader_open"
status_tool = "reader_status"
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.capabilities.ui_contributions);
        assert_eq!(manifest.ui.namespace, "reader");
        assert_eq!(manifest.ui.commands[0].slash, "/reader open");
        assert_eq!(manifest.ui.status_items[0].refresh_tool, "reader_status");
        assert_eq!(manifest.ui.surfaces[0].rendering, "delegated");
        assert_eq!(manifest.ui.surfaces[0].preferred_placements[0], "side_pane");
        assert_eq!(
            manifest.ui.surfaces[0].open_tool.as_deref(),
            Some("reader_open")
        );
    }

    #[test]
    fn test_config_fields_parse_from_toml() {
        let toml_str = r#"
[extension]
name = "vox"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/vox"

[config.signal_phone]
type = "string"
label = "Signal phone number"
description = "E.164 format"
required = true
pattern = '^\+[1-9]\d{1,14}$'
placeholder = "+14155551234"

[config.webhook_enabled]
type = "boolean"
label = "Enable webhook"
default = "false"

[config.imap_provider]
type = "enum"
label = "IMAP provider"
values = ["gmail", "fastmail", "custom"]
default = "gmail"
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.config.len(), 3);

        let phone = &manifest.config["signal_phone"];
        assert_eq!(phone.field_type, ConfigFieldType::String);
        assert!(phone.required);
        assert_eq!(phone.pattern.as_deref(), Some(r"^\+[1-9]\d{1,14}$"));

        let webhook = &manifest.config["webhook_enabled"];
        assert_eq!(webhook.field_type, ConfigFieldType::Boolean);
        assert!(!webhook.required);
        assert_eq!(webhook.default.as_deref(), Some("false"));

        let imap = &manifest.config["imap_provider"];
        assert_eq!(imap.field_type, ConfigFieldType::Enum);
        assert_eq!(imap.values, vec!["gmail", "fastmail", "custom"]);

        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn test_config_enum_requires_values() {
        let manifest = ExtensionManifest {
            extension: ExtensionMetadata {
                name: "test".into(),
                version: "0.1.0".into(),
                description: "".into(),
                sdk_version: "".into(),
            },
            runtime: RuntimeConfig::Native {
                binary: "bin".into(),
            },
            startup: StartupConfig::default(),
            widgets: HashMap::new(),
            mind: MindConfig::default(),
            config: HashMap::from([(
                "my_enum".into(),
                ConfigField {
                    field_type: ConfigFieldType::Enum,
                    label: "Pick one".into(),
                    description: "".into(),
                    required: false,
                    default: None,
                    pattern: None,
                    placeholder: None,
                    values: vec![],
                },
            )]),
            capabilities: Capabilities::default(),
            ui: UiConfig::default(),
            permissions: ManifestPermissions::default(),
        };

        let err = manifest.validate().unwrap_err();
        assert!(err.reason.contains("no values"));
    }

    #[test]
    fn test_config_backwards_compat_no_config_section() {
        let toml_str = r#"
[extension]
name = "old-ext"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/old"
"#;
        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.config.is_empty());
        assert!(manifest.validate().is_ok());
    }
}

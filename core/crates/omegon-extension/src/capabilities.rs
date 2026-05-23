//! Protocol capability negotiation.
//!
//! During the `initialize` handshake, both host and extension declare which
//! capabilities they support. Only capabilities declared by both sides are active.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol version. Bump when the wire format changes incompatibly.
pub const PROTOCOL_VERSION: u16 = 2;

/// Capability flags exchanged during the `initialize` handshake.
///
/// Both host and extension declare capabilities. A capability is active
/// only if both sides declare support for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    /// Extension provides tools (get_tools / tools/list / tools/call).
    #[serde(default = "default_true")]
    pub tools: bool,

    /// Extension provides stateful UI widgets.
    #[serde(default)]
    pub widgets: bool,

    /// Extension uses the persistent mind (knowledge) system.
    #[serde(default)]
    pub mind: bool,

    /// Extension provides a vox bridge (messaging connector).
    #[serde(default)]
    pub vox: bool,

    /// Extension exposes addressable resources.
    #[serde(default)]
    pub resources: bool,

    /// Extension provides prompt templates.
    #[serde(default)]
    pub prompts: bool,

    /// Extension may request LLM completions from the host.
    #[serde(default)]
    pub sampling: bool,

    /// Extension may request user input from the host.
    #[serde(default)]
    pub elicitation: bool,

    /// Extension supports streaming tool responses (progress with content).
    #[serde(default)]
    pub streaming: bool,

    /// Extension can return declarative host action requests in tool results.
    #[serde(default)]
    pub host_actions: bool,

    /// Extension can imperatively request host action execution.
    #[serde(default)]
    pub host_action_execution: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            tools: true,
            widgets: false,
            mind: false,
            vox: false,
            resources: false,
            prompts: false,
            sampling: false,
            elicitation: false,
            streaming: false,
            host_actions: false,
            host_action_execution: false,
        }
    }
}

impl Capabilities {
    /// All capabilities enabled — used by the host to declare full support.
    pub fn host_all() -> Self {
        Self {
            tools: true,
            widgets: true,
            mind: true,
            vox: true,
            resources: true,
            prompts: true,
            sampling: true,
            elicitation: true,
            streaming: true,
            host_actions: true,
            host_action_execution: true,
        }
    }

    /// Intersect with another set of capabilities.
    /// Only capabilities supported by BOTH sides are active.
    pub fn intersect(&self, other: &Self) -> Self {
        Self {
            tools: self.tools && other.tools,
            widgets: self.widgets && other.widgets,
            mind: self.mind && other.mind,
            vox: self.vox && other.vox,
            resources: self.resources && other.resources,
            prompts: self.prompts && other.prompts,
            sampling: self.sampling && other.sampling,
            elicitation: self.elicitation && other.elicitation,
            streaming: self.streaming && other.streaming,
            host_actions: self.host_actions && other.host_actions,
            host_action_execution: self.host_action_execution && other.host_action_execution,
        }
    }
}

/// Info about the host, sent during initialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub name: String,
    pub version: String,
}

/// Info about the extension, returned during initialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionInfo {
    pub name: String,
    pub version: String,
    pub sdk_version: String,
}

/// Parameters for the `initialize` request (host → extension).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub protocol_version: u16,
    pub host_info: HostInfo,
    pub capabilities: Capabilities,
}

/// Result of the `initialize` request (extension → host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub protocol_version: u16,
    pub extension_info: ExtensionInfo,
    pub capabilities: Capabilities,
    /// Tools are included in the initialize response so no separate
    /// `get_tools` / `tools/list` call is needed on startup.
    #[serde(default)]
    pub tools: Vec<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_capabilities() {
        let caps = Capabilities::default();
        assert!(caps.tools);
        assert!(!caps.widgets);
        assert!(!caps.sampling);
        assert!(!caps.host_actions);
        assert!(!caps.host_action_execution);
    }

    #[test]
    fn test_host_all() {
        let caps = Capabilities::host_all();
        assert!(caps.tools);
        assert!(caps.widgets);
        assert!(caps.sampling);
        assert!(caps.elicitation);
        assert!(caps.host_actions);
        assert!(caps.host_action_execution);
    }

    #[test]
    fn test_intersect() {
        let host = Capabilities::host_all();
        let ext = Capabilities {
            tools: true,
            widgets: true,
            mind: false,
            ..Default::default()
        };
        let active = host.intersect(&ext);
        assert!(active.tools);
        assert!(active.widgets);
        assert!(!active.mind);
        assert!(!active.sampling);
        assert!(!active.host_actions);
        assert!(!active.host_action_execution);
    }

    #[test]
    fn test_capabilities_deserialize_legacy_payload_defaults_host_actions_off() {
        let caps: Capabilities = serde_json::from_value(serde_json::json!({
            "tools": true,
            "streaming": true
        }))
        .unwrap();

        assert!(caps.tools);
        assert!(caps.streaming);
        assert!(!caps.host_actions);
        assert!(!caps.host_action_execution);
    }

    #[test]
    fn test_initialize_params_roundtrip() {
        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION,
            host_info: HostInfo {
                name: "omegon".to_string(),
                version: "0.16.0".to_string(),
            },
            capabilities: Capabilities::host_all(),
        };

        let json = serde_json::to_string(&params).unwrap();
        let parsed: InitializeParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.protocol_version, 2);
        assert_eq!(parsed.host_info.name, "omegon");
        assert!(parsed.capabilities.sampling);
        assert!(parsed.capabilities.host_actions);
        assert!(parsed.capabilities.host_action_execution);
    }

    #[test]
    fn test_initialize_result_roundtrip() {
        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION,
            extension_info: ExtensionInfo {
                name: "scribe".to_string(),
                version: "0.2.0".to_string(),
                sdk_version: "0.16.0".to_string(),
            },
            capabilities: Capabilities {
                tools: true,
                resources: true,
                ..Default::default()
            },
            tools: vec![serde_json::json!({
                "name": "list_issues",
                "label": "List Issues",
                "description": "List issues",
                "parameters": {"type": "object", "properties": {}}
            })],
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: InitializeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.extension_info.name, "scribe");
        assert!(parsed.capabilities.resources);
        assert!(!parsed.capabilities.sampling);
        assert_eq!(parsed.tools.len(), 1);
    }
}

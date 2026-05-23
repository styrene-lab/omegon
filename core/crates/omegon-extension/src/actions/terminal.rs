//! `terminal.create@1` HostAction types.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Versioned action type for terminal creation.
pub const TERMINAL_CREATE_V1: &str = "terminal.create@1";

/// Parameters for `terminal.create@1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCreateParams {
    /// Executable to launch. Hosts must apply manifest and runtime policy before execution.
    pub command: String,
    /// Argument vector. No shell-string variant exists in v1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Optional working directory request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Optional environment additions. Host policy decides which keys may pass through.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Optional human-readable terminal title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional advisory placement request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<TerminalPlacement>,
    /// Optional origin-scoped reuse key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reuse_key: Option<String>,
}

impl TerminalCreateParams {
    /// Create terminal params for a command with no arguments.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            title: None,
            placement: None,
            reuse_key: None,
        }
    }

    /// Add argv entries.
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
}

/// Advisory terminal placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPlacement {
    /// Host chooses placement.
    Default,
    /// Open beside the current workspace/editor pane.
    SidePane,
    /// Open below the current workspace/editor pane.
    BottomPane,
    /// Open in a new tab/window when supported.
    NewTab,
}

/// Result payload for `terminal.create@1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCreateResult {
    /// Host-assigned terminal identifier.
    pub terminal_id: String,
    /// Backend that satisfied the request, for example `zellij`.
    pub backend: String,
    /// Actual placement chosen by the host. Requested placement is advisory.
    pub actual_placement: String,
    /// Optional warnings/degradations when creation succeeded with caveats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_create_params_round_trip() {
        let params = TerminalCreateParams {
            command: "bookokrat".to_string(),
            args: vec!["/tmp/book.epub".to_string()],
            cwd: Some("/workspace".to_string()),
            env: BTreeMap::from([("BOOKOKRAT_THEME".to_string(), "dark".to_string())]),
            title: Some("Reader".to_string()),
            placement: Some(TerminalPlacement::SidePane),
            reuse_key: Some("reader".to_string()),
        };

        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["command"], "bookokrat");
        assert_eq!(json["placement"], "side_pane");
        let parsed: TerminalCreateParams = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, params);
    }

    #[test]
    fn terminal_create_result_round_trip() {
        let result = TerminalCreateResult {
            terminal_id: "term_123".to_string(),
            backend: "zellij".to_string(),
            actual_placement: "background_session".to_string(),
            warnings: vec!["placement degraded".to_string()],
        };
        let json = serde_json::to_value(&result).unwrap();
        let parsed: TerminalCreateResult = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, result);
    }
}

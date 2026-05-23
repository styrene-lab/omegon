//! Host action protocol types.
//!
//! HostActions are declarative requests for Omegon-managed side effects such as
//! opening a terminal pane. Extensions describe intent; the host remains
//! responsible for validation, policy, rendering, and execution.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Declarative host-managed side-effect request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostAction {
    /// Extension-local action identifier.
    pub id: String,
    /// Versioned action kind, for example `terminal.create@1`.
    #[serde(rename = "type")]
    pub action_type: String,
    /// Action-family-specific parameters.
    #[serde(default)]
    pub params: Value,
    /// Human-readable label suitable for review UIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional advisory execution mode requested by the extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<HostActionExecution>,
    /// Optional extension metadata. Hosts must treat this as untrusted input.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl HostAction {
    /// Create a host action with an id, type, and typed params.
    pub fn new(
        id: impl Into<String>,
        action_type: impl Into<String>,
        params: impl Serialize,
    ) -> serde_json::Result<Self> {
        Ok(Self {
            id: id.into(),
            action_type: action_type.into(),
            params: serde_json::to_value(params)?,
            label: None,
            execution: None,
            metadata: BTreeMap::new(),
        })
    }
}

/// Advisory execution preference for a HostAction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostActionExecution {
    /// Host should present the action for explicit operator confirmation.
    Manual,
    /// Host may execute automatically if manifest and runtime policy allow it.
    AutoIfAllowed,
}

/// Typed outcome returned by host-side action execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostActionOutcome {
    /// Action id from the corresponding HostAction.
    pub action_id: String,
    /// Final status for validation/execution.
    pub status: HostActionStatus,
    /// Action-family-specific result payload, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Machine-readable error information, when execution did not complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<HostActionError>,
}

/// HostAction validation/execution status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostActionStatus {
    /// Action completed successfully.
    Completed,
    /// Action was denied by manifest, project, runtime, or operator policy.
    Denied,
    /// Action family or requested feature is unsupported by the host.
    Unsupported,
    /// Action candidate is malformed or failed schema validation.
    Invalid,
    /// Action failed during execution.
    Failed,
}

/// Machine-readable HostAction error detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostActionError {
    /// Stable error code.
    pub code: String,
    /// Human-readable diagnostic.
    pub message: String,
}

/// Typed tool content block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    /// Plain text content.
    Text { text: String },
    /// Markdown content.
    Markdown { markdown: String },
}

/// Typed tool result that can carry declarative host actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ToolResult {
    /// Ordinary tool response content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ToolContent>,
    /// Declarative host side effects requested by the extension.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<HostAction>,
}

impl ToolResult {
    /// Create a text tool result.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: text.into() }],
            actions: Vec::new(),
        }
    }

    /// Attach a declarative host action.
    pub fn with_action(mut self, action: HostAction) -> Self {
        self.actions.push(action);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_action_status_invalid_serializes_as_snake_case() {
        let status = serde_json::to_value(HostActionStatus::Invalid).unwrap();
        assert_eq!(status, serde_json::json!("invalid"));
    }

    #[test]
    fn host_action_outcome_round_trips() {
        let json = serde_json::json!({
            "action_id": "open-reader",
            "status": "completed",
            "result": {
                "terminal_id": "term_123",
                "backend": "zellij"
            }
        });

        let outcome: HostActionOutcome = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(outcome.action_id, "open-reader");
        assert_eq!(outcome.status, HostActionStatus::Completed);
        assert_eq!(outcome.result.as_ref().unwrap()["terminal_id"], "term_123");
        assert_eq!(serde_json::to_value(outcome).unwrap(), json);
    }

    #[test]
    fn host_action_outcome_acceptance_example_round_trips() {
        let json = serde_json::json!({
            "action_id": "open-reader",
            "status": "completed",
            "result": {
                "terminal_id": "term_123",
                "backend": "zellij"
            }
        });

        let outcome: HostActionOutcome = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(outcome).unwrap(), json);
    }

    #[test]
    fn tool_result_carries_actions() {
        let action = HostAction::new(
            "open-reader",
            "terminal.create@1",
            serde_json::json!({"command": "bookokrat"}),
        )
        .unwrap();
        let result = ToolResult::text("Opening reader").with_action(action);
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["actions"][0]["type"], "terminal.create@1");
        assert_eq!(json["content"][0]["text"], "Opening reader");
    }
}

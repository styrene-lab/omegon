//! Tool management — list, enable, disable tools.
//!
//! Provides `manage_tools` for the agent to control which tools are active.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};

/// Shared set of disabled tool names.
pub type DisabledTools = Arc<Mutex<HashSet<String>>>;

pub struct ManageTools {
    disabled: DisabledTools,
    /// Snapshot of all tool names (set during init).
    all_tools: Arc<Mutex<Vec<String>>>,
}

impl ManageTools {
    pub fn new() -> Self {
        Self {
            disabled: Arc::new(Mutex::new(HashSet::new())),
            all_tools: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get a handle to the disabled set for the bus to check.
    pub fn disabled_handle(&self) -> DisabledTools {
        self.disabled.clone()
    }

    /// Set the full tool list (called after bus finalize).
    pub fn set_all_tools(&self, names: Vec<String>) {
        *self.all_tools.lock().unwrap() = names;
    }
}

#[async_trait]
impl Feature for ManageTools {
    fn name(&self) -> &str {
        "manage-tools"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "manage_tools".into(),
            label: "manage_tools".into(),
            description: "List, enable, or disable tools. Use to activate tools the user \
                requests or disable irrelevant ones to save context window space."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "enable", "disable"],
                        "description": "Action: list (show tools), enable/disable (toggle tools)"
                    },
                    "tools": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tool names to enable/disable"
                    }
                },
                "required": ["action"]
            }),
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        if tool_name != "manage_tools" {
            anyhow::bail!("Unknown tool: {tool_name}");
        }

        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("action required"))?;

        match action {
            "list" => {
                let all = self.all_tools.lock().unwrap().clone();
                let disabled = self.disabled.lock().unwrap();
                let mut lines = Vec::new();
                for name in &all {
                    let status = if disabled.contains(name) {
                        "disabled"
                    } else {
                        "enabled"
                    };
                    lines.push(format!("  {status:>8}  {name}"));
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "**Tools** ({} total, {} disabled)\n\n{}",
                            all.len(),
                            disabled.len(),
                            lines.join("\n")
                        ),
                    }],
                    details: Value::Null,
                })
            }
            "enable" => {
                let tools = extract_tool_names(&args);
                let mut disabled = self.disabled.lock().unwrap();
                let mut enabled = Vec::new();
                for name in &tools {
                    if disabled.remove(name) {
                        enabled.push(name.clone());
                    }
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: if enabled.is_empty() {
                            "No tools were disabled to enable.".into()
                        } else {
                            format!("Enabled: {}", enabled.join(", "))
                        },
                    }],
                    details: Value::Null,
                })
            }
            "disable" => {
                let tools = extract_tool_names(&args);
                let mut disabled = self.disabled.lock().unwrap();
                let mut newly_disabled = Vec::new();
                for name in &tools {
                    // Never allow disabling manage_tools itself
                    if name == "manage_tools" {
                        continue;
                    }
                    if disabled.insert(name.clone()) {
                        newly_disabled.push(name.clone());
                    }
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: if newly_disabled.is_empty() {
                            "No tools were newly disabled.".into()
                        } else {
                            format!("Disabled: {}", newly_disabled.join(", "))
                        },
                    }],
                    details: Value::Null,
                })
            }
            _ => anyhow::bail!("Unknown action: {action}"),
        }
    }
}

fn extract_tool_names(args: &Value) -> Vec<String> {
    args["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

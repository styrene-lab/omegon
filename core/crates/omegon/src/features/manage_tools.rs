//! Tool management — list, enable, disable tools by name or group.
//!
//! Provides `manage_tools` for the agent to control which tools are active.
//! Tool groups let operators activate/deactivate related sets with one call.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};

/// Shared set of disabled tool names.
pub type DisabledTools = Arc<Mutex<HashSet<String>>>;

/// Predefined tool groups — named sets that can be toggled together.
///
/// Groups represent coherent capability clusters. Operators enable/disable
/// groups to control the schema surface and model affordances.
/// Note: groups are mechanisms for toggling — they don't imply any default
/// enabled/disabled state. See setup.rs for the default disabled profile.
pub static TOOL_GROUPS: &[(&str, &[&str])] = &[
    (
        "memory-advanced",
        &[
            "memory_connect",
            "memory_search_archive",
            "memory_ingest_lifecycle",
        ],
    ),
    (
        "delegate",
        &["delegate", "delegate_result", "delegate_status"],
    ),
    // cleave is enabled by default — critical subagent decomposition capability.
    // Disable only in constrained/quota contexts.
    ("cleave", &["cleave_assess", "cleave_run"]),
    (
        "lifecycle-advanced",
        &["lifecycle_doctor", "codebase_search", "codebase_index"],
    ),
    (
        "model-control",
        &[
            "set_model_intent",
            "switch_to_offline_driver",
            "set_thinking_level",
        ],
    ),
    (
        "harness-lifecycle",
        &["design_tree", "design_tree_update", "openspec_manage"],
    ),
];

pub struct ManageTools {
    disabled: DisabledTools,
    /// Snapshot of all tool names (set during init).
    all_tools: Arc<Mutex<Vec<String>>>,
}

impl Default for ManageTools {
    fn default() -> Self {
        Self::new()
    }
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
            name: crate::tool_registry::manage_tools::MANAGE_TOOLS.into(),
            label: "manage_tools".into(),
            description: "List, enable, or disable tools. Use to activate tools the user \
                requests or disable irrelevant ones to save context window space."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "enable", "disable", "enable_group", "disable_group", "list_groups"]
                    },
                    "tools": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "group": { "type": "string" }
                },
                "required": ["action"]
            }),
            capabilities: vec![omegon_traits::ToolCapability::Orientation],
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
            "enable_group" | "disable_group" => {
                let group_name = args["group"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("group required for {action}"))?;
                let tools = TOOL_GROUPS
                    .iter()
                    .find(|(name, _)| *name == group_name)
                    .map(|(_, tools)| *tools)
                    .ok_or_else(|| {
                        let available: Vec<&str> = TOOL_GROUPS.iter().map(|(n, _)| *n).collect();
                        anyhow::anyhow!(
                            "Unknown group '{group_name}'. Available: {}",
                            available.join(", ")
                        )
                    })?;
                let mut disabled = self.disabled.lock().unwrap();
                let mut changed = Vec::new();
                if action == "enable_group" {
                    for name in tools {
                        if disabled.remove(*name) {
                            changed.push(*name);
                        }
                    }
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: if changed.is_empty() {
                                format!("Group '{group_name}' — all tools already enabled.")
                            } else {
                                format!("Group '{group_name}' enabled: {}", changed.join(", "))
                            },
                        }],
                        details: Value::Null,
                    })
                } else {
                    for name in tools {
                        if *name != "manage_tools" && disabled.insert(name.to_string()) {
                            changed.push(*name);
                        }
                    }
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: if changed.is_empty() {
                                format!("Group '{group_name}' — all tools already disabled.")
                            } else {
                                format!("Group '{group_name}' disabled: {}", changed.join(", "))
                            },
                        }],
                        details: Value::Null,
                    })
                }
            }
            "list_groups" => {
                let disabled = self.disabled.lock().unwrap();
                let mut lines = vec!["**Tool Groups**".to_string(), String::new()];
                for (group_name, tools) in TOOL_GROUPS {
                    let enabled_count = tools.iter().filter(|t| !disabled.contains(**t)).count();
                    let state = if enabled_count == tools.len() {
                        "all enabled"
                    } else if enabled_count == 0 {
                        "all disabled"
                    } else {
                        "partial"
                    };
                    lines.push(format!(
                        "  {group_name:<20} [{state}]  {}",
                        tools.join(", ")
                    ));
                }
                lines.push(String::new());
                lines.push(
                    "Use: manage_tools(enable_group|disable_group, group: \"<name>\")".into(),
                );
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
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

//! Agent-callable printable session variable tools.
//!
//! Variables are non-secret runtime configuration. Values are intentionally
//! visible in tool output; sensitive values belong in `/secrets` instead.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolCapability, ToolDefinition, ToolProvider, ToolResult};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub struct VariableToolsProvider;

#[async_trait]
impl ToolProvider for VariableToolsProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::variables::VARIABLE_SET.into(),
                label: "Variable Set".into(),
                description: "Set a printable session-scoped runtime variable. Values are visible in outputs; use secret_set for credentials.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Variable name, e.g. PROJECT_ENV" },
                        "value": { "type": "string", "description": "Printable non-secret value" }
                    },
                    "required": ["name", "value"],
                    "additionalProperties": false
                }),
                capabilities: vec![ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::variables::VARIABLE_LIST.into(),
                label: "Variable List".into(),
                description: "List printable session variables and values.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                capabilities: vec![ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::variables::VARIABLE_DELETE.into(),
                label: "Variable Delete".into(),
                description: "Delete a printable session-scoped runtime variable.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Variable name to delete" }
                    },
                    "required": ["name"],
                    "additionalProperties": false
                }),
                capabilities: vec![ToolCapability::StateChanging],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::variables::VARIABLE_SET => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' argument"))?;
                let value = args
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing 'value' argument"))?;
                let response = crate::control::variables::variables_set_response(name, value).await;
                Ok(response_to_tool_result(
                    response,
                    json!({ "name": name, "value": value }),
                ))
            }
            crate::tool_registry::variables::VARIABLE_LIST => {
                let response = crate::control::variables::variables_view_response().await;
                let vars = crate::control::variables::variables_snapshot();
                Ok(response_to_tool_result(
                    response,
                    json!({
                        "count": vars.len(),
                        "entries": vars.into_iter().map(|(name, value)| json!({
                            "name": name,
                            "value": value,
                            "sensitive_name_hint": crate::control::variables::variable_name_has_sensitive_hint(&name),
                        })).collect::<Vec<_>>()
                    }),
                ))
            }
            crate::tool_registry::variables::VARIABLE_DELETE => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' argument"))?;
                let response = crate::control::variables::variables_delete_response(name).await;
                Ok(response_to_tool_result(response, json!({ "name": name })))
            }
            _ => anyhow::bail!("unknown variable tool: {tool_name}"),
        }
    }
}

fn response_to_tool_result(
    response: omegon_traits::SlashCommandResponse,
    details: Value,
) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: response.output.unwrap_or_default(),
        }],
        details: json!({ "accepted": response.accepted, "variables": details }),
    }
}

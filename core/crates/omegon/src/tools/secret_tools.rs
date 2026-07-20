//! Agent-callable secret management tools.
//!
//! These tools let the harness persist internal automation secrets using the
//! same encrypted/keyring-backed SecretsManager that `/secrets` already uses.
//! They intentionally do NOT return secret values to the model.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolCapability, ToolDefinition, ToolProvider, ToolResult};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct SecretToolsProvider {
    secrets: Arc<omegon_secrets::SecretsManager>,
}

impl SecretToolsProvider {
    pub fn new(secrets: Arc<omegon_secrets::SecretsManager>) -> Self {
        Self { secrets }
    }
}

#[async_trait]
impl ToolProvider for SecretToolsProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::secrets::SECRET_SET.into(),
                label: "Secret Set".into(),
                description: "Store an internal secret for harness use. Values are written to Omegon's encrypted/keyring-backed secret manager and are not echoed back in tool output.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Secret name, e.g. OLLAMA_API_KEY" },
                        "value": { "type": "string", "description": "Raw secret value to store" },
                        "recipe": { "type": "string", "description": "Optional recipe form (env:VAR, cmd:..., vault:..., keyring:..., file:...). Mutually exclusive with value." }
                    },
                    "required": ["name"],
                    "additionalProperties": false
                }),
                capabilities: vec![ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::secrets::SECRET_LIST.into(),
                label: "Secret List".into(),
                description: "List configured internal secret names and their resolution hints without revealing secret values.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                capabilities: vec![ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::secrets::SECRET_DELETE.into(),
                label: "Secret Delete".into(),
                description: "Delete an internal secret or recipe from Omegon's secret manager.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Secret name to delete" }
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
            crate::tool_registry::secrets::SECRET_SET => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' argument"))?;
                let value = args.get("value").and_then(|v| v.as_str());
                let recipe = args.get("recipe").and_then(|v| v.as_str());

                match (value, recipe) {
                    (Some(_), Some(_)) => {
                        anyhow::bail!("provide either 'value' or 'recipe', not both")
                    }
                    (None, None) => anyhow::bail!("missing 'value' or 'recipe' argument"),
                    (Some(value), None) => {
                        self.secrets.set_keyring_secret(name, value)?;
                        Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!("Stored secret '{name}' for harness use."),
                            }],
                            details: json!({
                                "name": name,
                                "stored_via": "keyring",
                            }),
                        })
                    }
                    (None, Some(recipe)) => {
                        // Only allow safe recipe types from the model.
                        // cmd: and file: enable arbitrary code execution and
                        // filesystem reads — restrict to operator-only CLI.
                        let safe = recipe.starts_with("keyring:") || recipe.starts_with("env:");
                        if !safe {
                            anyhow::bail!(
                                "Recipe type not allowed from agent tools. \
                                 Only keyring: and env: recipes can be set programmatically. \
                                 Use `omegon secrets set` in a terminal for cmd:, file:, and vault: recipes."
                            );
                        }
                        self.secrets.set_recipe(name, recipe)?;
                        Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!("Stored secret recipe for '{name}'."),
                            }],
                            details: json!({
                                "name": name,
                                "stored_via": "recipe",
                                "recipe": recipe,
                            }),
                        })
                    }
                }
            }
            crate::tool_registry::secrets::SECRET_LIST => {
                let entries = self.secrets.list_recipe_descriptors();
                let text = if entries.is_empty() {
                    "No harness-managed secrets configured.".to_string()
                } else {
                    let mut out = String::from(
                        "Harness-managed secrets (metadata only; values not resolved):\n",
                    );
                    for entry in &entries {
                        out.push_str(&format!("- {}: {}\n", entry.name, entry.recipe));
                    }
                    out.trim_end().to_string()
                };
                let details = json!({
                    "count": entries.len(),
                    "entries": entries.iter().map(|entry| {
                        json!({
                            "name": entry.name,
                            "recipe": entry.recipe,
                            "kind": entry.kind,
                            "payload": entry.payload,
                            "status": "not_checked",
                        })
                    }).collect::<Vec<_>>()
                });
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details,
                })
            }
            crate::tool_registry::secrets::SECRET_DELETE => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' argument"))?;
                self.secrets.delete_recipe(name)?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Deleted secret '{name}'."),
                    }],
                    details: json!({ "name": name }),
                })
            }
            _ => anyhow::bail!("unknown secret tool: {tool_name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    static SUPPRESS_HOST_KEYRING: Once = Once::new();

    fn provider() -> SecretToolsProvider {
        SUPPRESS_HOST_KEYRING.call_once(|| {
            // Tests in this crate compile omegon-secrets as a normal dependency,
            // so its #[cfg(test)] in-memory keyring backend is not active. Force
            // runtime suppression before constructing SecretsManager so validation
            // never prompts the operator's real macOS Keychain.
            unsafe { std::env::set_var("OMEGON_NO_KEYRING", "1") };
        });
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(omegon_secrets::SecretsManager::new(dir.path()).unwrap());
        SecretToolsProvider::new(secrets)
    }

    #[tokio::test]
    async fn secret_set_recipe_does_not_echo_sensitive_input() {
        let provider = provider();
        let result = provider
            .execute(
                crate::tool_registry::secrets::SECRET_SET,
                "tc1",
                json!({"name": "TEST_SECRET", "recipe": "env:OMEGON_TEST_SECRET"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Stored secret recipe for 'TEST_SECRET'"));
        assert!(!text.contains("OMEGON_TEST_SECRET"));
    }

    #[tokio::test]
    async fn secret_set_recipe_and_list_work() {
        let provider = provider();
        provider
            .execute(
                crate::tool_registry::secrets::SECRET_SET,
                "tc2",
                json!({"name": "BRAVE_API_KEY", "recipe": "env:BRAVE_API_KEY"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let listed = provider
            .execute(
                crate::tool_registry::secrets::SECRET_LIST,
                "tc3",
                json!({}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let text = listed.content[0].as_text().unwrap();
        assert!(text.contains("BRAVE_API_KEY: env:BRAVE_API_KEY"));
    }

    #[tokio::test]
    async fn secret_set_value_is_idempotent_and_repairs_listing() {
        let provider = provider();
        provider
            .execute(
                crate::tool_registry::secrets::SECRET_SET,
                "tc-idempotent-1",
                json!({"name": "BRAVE_API_KEY", "value": "first"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        provider
            .execute(
                crate::tool_registry::secrets::SECRET_SET,
                "tc-idempotent-2",
                json!({"name": "BRAVE_API_KEY", "value": "second"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let listed = provider
            .execute(
                crate::tool_registry::secrets::SECRET_LIST,
                "tc-idempotent-3",
                json!({}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let text = listed.content[0].as_text().unwrap();
        assert!(text.contains("BRAVE_API_KEY: store:"));
        assert!(!text.contains("[resolves]"));
    }

    #[tokio::test]
    async fn secret_delete_removes_entry() {
        let provider = provider();
        provider
            .execute(
                crate::tool_registry::secrets::SECRET_SET,
                "tc4",
                json!({"name": "TEMP_SECRET", "recipe": "env:TEMP_SECRET"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        provider
            .execute(
                crate::tool_registry::secrets::SECRET_DELETE,
                "tc5",
                json!({"name": "TEMP_SECRET"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let listed = provider
            .execute(
                crate::tool_registry::secrets::SECRET_LIST,
                "tc6",
                json!({}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let text = listed.content[0].as_text().unwrap();
        assert!(!text.contains("TEMP_SECRET"));
    }
}

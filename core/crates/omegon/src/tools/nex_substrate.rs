use std::path::{Path, PathBuf};

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition, ToolProvider, ToolResult};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::nex::substrate::NexSubstrateDelegation;
use crate::tool_registry::core as reg;

pub struct NexSubstrateProvider {
    cwd: PathBuf,
    delegations: Vec<NexSubstrateDelegation>,
}

impl NexSubstrateProvider {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            delegations: Vec::new(),
        }
    }

    pub fn with_delegations(mut self, delegations: Vec<NexSubstrateDelegation>) -> Self {
        self.delegations = delegations;
        self
    }

    fn resolve_path(&self, path: &str) -> anyhow::Result<PathBuf> {
        let path = expand_tilde(path);
        let path = if path.is_absolute() {
            path
        } else {
            self.cwd.join(path)
        };
        Ok(path)
    }
}

#[async_trait]
impl ToolProvider for NexSubstrateProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: reg::NEX_SUBSTRATE.into(),
            label: reg::NEX_SUBSTRATE.into(),
            description: "Read-only Nex substrate inspection. Calls Nex to inspect project devenv/SecretSpec substrate facts and returns an advisory Omegon policy overlay without mutating tools, profiles, or secrets.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["inspect"],
                        "description": "Read-only substrate action to perform"
                    },
                    "path": {
                        "type": "string",
                        "description": "Project directory to inspect; defaults to the current workspace root"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["devenv"],
                        "description": "Substrate report family; first slice supports only devenv"
                    }
                },
                "required": ["action"]
            }),
            capabilities: vec![omegon_traits::ToolCapability::RepoInspection],
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        if tool_name != reg::NEX_SUBSTRATE {
            anyhow::bail!("unsupported Nex substrate tool: {tool_name}");
        }
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'action' argument"))?;
        if action != "inspect" {
            anyhow::bail!(
                "unsupported nex_substrate action: {action}; MVP is read-only and supports only inspect"
            );
        }
        let mode = args["mode"].as_str().unwrap_or("devenv");
        if mode != "devenv" {
            anyhow::bail!("unsupported nex_substrate mode: {mode}; MVP supports only devenv");
        }
        let path = match args["path"].as_str() {
            Some(path) => self.resolve_path(path)?,
            None => self.cwd.clone(),
        };
        let mut report = crate::nex::substrate::inspect_devenv(&path).await;
        report.delegation = crate::nex::substrate::delegation_for_command(
            &self.delegations,
            "devenv.inspect",
        );
        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: crate::nex::substrate::summary_text(&report),
            }],
            details: serde_json::to_value(&report)?,
        })
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    Path::new(path).to_path_buf()
}

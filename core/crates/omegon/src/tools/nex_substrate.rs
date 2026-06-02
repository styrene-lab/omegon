use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition, ToolProvider, ToolResult};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::nex::substrate::NexSubstrateDelegation;
use crate::tool_registry::core as reg;
use crate::tools::WorkspaceBoundary;

pub struct NexSubstrateProvider {
    cwd: PathBuf,
    boundary: Option<WorkspaceBoundary>,
    delegations: Vec<NexSubstrateDelegation>,
    executor: Option<Arc<dyn NexDelegationExecutor>>,
}

#[async_trait]
pub trait NexDelegationExecutor: Send + Sync {
    async fn execute_devenv_inspect(&self, tool: &str, path: &Path) -> anyhow::Result<ToolResult>;
}

impl NexSubstrateProvider {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            boundary: None,
            delegations: Vec::new(),
            executor: None,
        }
    }

    pub fn with_boundary(mut self, boundary: WorkspaceBoundary) -> Self {
        self.boundary = Some(boundary);
        self
    }

    pub fn with_delegations(mut self, delegations: Vec<NexSubstrateDelegation>) -> Self {
        self.delegations = delegations;
        self
    }

    pub fn with_executor(mut self, executor: Arc<dyn NexDelegationExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    fn resolve_path(&self, path: &str) -> anyhow::Result<PathBuf> {
        if let Some(boundary) = &self.boundary {
            return boundary.check_path(path);
        }
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
        let mut report = if let Some(delegation) =
            crate::nex::substrate::delegation_for_command(&self.delegations, "devenv.inspect")
        {
            if let Some(executor) = &self.executor {
                match executor
                    .execute_devenv_inspect(&delegation.tool, &path)
                    .await
                {
                    Ok(result) => report_from_delegated_result(&path, result)?,
                    Err(error) => {
                        let mut report = crate::nex::substrate::inspect_devenv(&path).await;
                        report.diagnostics.push(format!(
                            "omegon-nex delegation failed; used direct fallback: {error}"
                        ));
                        report
                    }
                }
            } else {
                crate::nex::substrate::inspect_devenv(&path).await
            }
        } else {
            crate::nex::substrate::inspect_devenv(&path).await
        };
        report.delegation =
            crate::nex::substrate::delegation_for_command(&self.delegations, "devenv.inspect");
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
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    Path::new(path).to_path_buf()
}

fn report_from_delegated_result(
    path: &Path,
    result: ToolResult,
) -> anyhow::Result<crate::nex::substrate::NexSubstrateReport> {
    let report_json = result
        .details
        .get("data")
        .and_then(|data| data.get("report"))
        .cloned()
        .or_else(|| result.details.get("report").cloned())
        .ok_or_else(|| {
            anyhow::anyhow!("delegated nex_devenv_inspect result did not include data.report")
        })?;
    let policy = crate::nex::substrate::derive_policy(&report_json);
    let mut diagnostics = Vec::new();
    if let Some(text) = result
        .details
        .get("data")
        .and_then(|data| data.get("degraded_reason"))
        .and_then(Value::as_str)
    {
        diagnostics.push(format!("omegon-nex degraded: {text}"));
    }
    Ok(crate::nex::substrate::NexSubstrateReport {
        schema: crate::nex::substrate::REPORT_SCHEMA,
        source: "omegon-nex",
        nex_available: true,
        path: path.display().to_string(),
        mode: "devenv".to_string(),
        reports: crate::nex::substrate::NexSubstrateReports {
            devenv_import: Some(report_json),
        },
        delegation: None,
        policy,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::ToolProvider;
    use serde_json::json;

    #[tokio::test]
    async fn rejects_paths_outside_workspace_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let provider = NexSubstrateProvider::new(dir.path().to_path_buf())
            .with_boundary(WorkspaceBoundary::new(dir.path().to_path_buf()));
        let result = provider
            .execute(
                reg::NEX_SUBSTRATE,
                "test",
                json!({"action": "inspect", "path": "/etc"}),
                CancellationToken::new(),
            )
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("PERMISSION REQUIRED")
        );
    }

    #[tokio::test]
    async fn initializes_tool_and_returns_degraded_report_without_delegation_or_nex() {
        static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let original_path = {
            let _guard = PATH_LOCK.lock().unwrap();
            let original_path = std::env::var_os("PATH");
            unsafe { std::env::set_var("PATH", "") };
            original_path
        };

        let dir = tempfile::tempdir().unwrap();
        let provider = NexSubstrateProvider::new(dir.path().to_path_buf());
        assert_eq!(provider.tools().len(), 1);

        let result = provider
            .execute(
                reg::NEX_SUBSTRATE,
                "test",
                json!({"action": "inspect"}),
                CancellationToken::new(),
            )
            .await;

        let _guard = PATH_LOCK.lock().unwrap();
        match original_path {
            Some(path) => unsafe { std::env::set_var("PATH", path) },
            None => unsafe { std::env::remove_var("PATH") },
        }
        drop(_guard);

        let result = result.expect("missing Nex should degrade, not fail the tool call");
        assert!(
            matches!(result.content.first(), Some(ContentBlock::Text { text }) if text.contains("Nex substrate inspection: unavailable"))
        );
        assert_eq!(
            result.details["schema"],
            crate::nex::substrate::REPORT_SCHEMA
        );
        assert_eq!(result.details["nex_available"], false);
        assert_eq!(result.details["policy"]["enforcement"], "advisory");
        let findings = result.details["policy"]["findings"].as_array().unwrap();
        assert!(
            findings
                .iter()
                .any(|finding| finding["code"] == "nex_unavailable")
        );
    }
}

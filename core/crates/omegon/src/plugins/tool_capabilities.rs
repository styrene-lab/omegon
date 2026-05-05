use omegon_traits::ToolCapability;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalExecutionHint {
    HttpGet,
    HttpMutating,
    ScriptOrContainer,
    McpDiscovery,
}

pub(crate) fn resolve_external_tool_capabilities(
    explicit: &[ToolCapability],
    name: &str,
    description: &str,
    parameters: &Value,
    execution_hint: ExternalExecutionHint,
) -> Vec<ToolCapability> {
    if !explicit.is_empty() {
        return dedupe_capabilities(explicit.iter().copied());
    }

    let inferred = infer_external_tool_capabilities(name, description, parameters, execution_hint);
    dedupe_capabilities(inferred)
}

pub(crate) fn mcp_resource_tool_capabilities() -> Vec<ToolCapability> {
    vec![
        ToolCapability::Orientation,
        ToolCapability::BroadOrientation,
    ]
}

pub(crate) fn mcp_prompt_tool_capabilities() -> Vec<ToolCapability> {
    vec![ToolCapability::Orientation]
}

fn infer_external_tool_capabilities(
    name: &str,
    description: &str,
    _parameters: &Value,
    execution_hint: ExternalExecutionHint,
) -> Vec<ToolCapability> {
    let text = format!("{} {}", name, description).to_ascii_lowercase();

    let is_broad_orientation = contains_any(
        &text,
        &[
            "search",
            "query",
            "list",
            "discover",
            "browse",
            "scan",
            "enumerate",
            "find",
        ],
    );
    let is_orientation = is_broad_orientation
        || contains_any(
            &text,
            &[
                "get", "fetch", "read", "view", "inspect", "describe", "status", "info", "show",
                "resource", "prompt",
            ],
        );
    let is_state_changing = contains_any(
        &text,
        &[
            "set", "write", "edit", "update", "patch", "modify", "delete", "remove", "create",
            "apply", "submit", "send", "start", "stop", "restart", "run", "execute", "ingest",
            "sync", "store",
        ],
    );

    let mut capabilities = Vec::new();
    if is_broad_orientation {
        capabilities.push(ToolCapability::BroadOrientation);
    }
    if is_orientation {
        capabilities.push(ToolCapability::Orientation);
    }
    if is_state_changing {
        capabilities.push(ToolCapability::StateChanging);
    }

    if capabilities.is_empty() {
        match execution_hint {
            ExternalExecutionHint::HttpGet | ExternalExecutionHint::McpDiscovery => {
                capabilities.push(ToolCapability::Orientation);
            }
            ExternalExecutionHint::HttpMutating | ExternalExecutionHint::ScriptOrContainer => {
                capabilities.push(ToolCapability::StateChanging);
            }
        }
    }

    capabilities
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn dedupe_capabilities(
    capabilities: impl IntoIterator<Item = ToolCapability>,
) -> Vec<ToolCapability> {
    capabilities
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_capabilities_override_inference() {
        let caps = resolve_external_tool_capabilities(
            &[ToolCapability::StateChanging],
            "list_issues",
            "List open issues",
            &json!({"type": "object"}),
            ExternalExecutionHint::HttpGet,
        );
        assert_eq!(caps, vec![ToolCapability::StateChanging]);
    }

    #[test]
    fn get_like_tools_infer_orientation() {
        let caps = resolve_external_tool_capabilities(
            &[],
            "get_status",
            "Get deployment status",
            &json!({"type": "object"}),
            ExternalExecutionHint::HttpGet,
        );
        assert_eq!(caps, vec![ToolCapability::Orientation]);
    }

    #[test]
    fn search_like_tools_infer_broad_orientation() {
        let caps = resolve_external_tool_capabilities(
            &[],
            "search_docs",
            "Search external documentation",
            &json!({"type": "object"}),
            ExternalExecutionHint::McpDiscovery,
        );
        assert_eq!(
            caps,
            vec![
                ToolCapability::Orientation,
                ToolCapability::BroadOrientation,
            ]
        );
    }

    #[test]
    fn script_tools_default_to_state_changing_when_ambiguous() {
        let caps = resolve_external_tool_capabilities(
            &[],
            "workspace_helper",
            "Workspace helper",
            &json!({"type": "object"}),
            ExternalExecutionHint::ScriptOrContainer,
        );
        assert_eq!(caps, vec![ToolCapability::StateChanging]);
    }
}

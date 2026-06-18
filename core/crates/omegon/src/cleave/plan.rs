//! Cleave plan — the input specification for a cleave run.

use crate::child_agent::ChildAgentRuntimeProfile;
use serde::{Deserialize, Serialize};

/// A cleave plan describes children to dispatch and their dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleavePlan {
    pub children: Vec<ChildPlan>,
    #[serde(default)]
    pub rationale: String,
    /// Default model for all children. Overrides scope-based heuristic routing.
    /// Individual children can further override with their own `model` field.
    /// If absent, the orchestrator applies cost-aware routing from the provider inventory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

/// A fully-specified child runtime profile controlled by the launching parent.
pub type CleaveChildRuntimeProfile = ChildAgentRuntimeProfile;

/// A single child in the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildPlan {
    pub label: String,
    pub description: String,
    pub scope: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Explicit model override for this child. Takes priority over `CleavePlan::default_model`
    /// and scope-based routing. Use for intentional up- or down-delegation
    /// (e.g. a research child that needs a higher-grade model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<CleaveChildRuntimeProfile>,
}

impl CleavePlan {
    /// Parse a plan from JSON.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let plan: CleavePlan = serde_json::from_str(json)?;
        if plan.children.is_empty() {
            anyhow::bail!("Cleave plan must have at least 1 child");
        }
        // Validate dependency references
        let labels: Vec<&str> = plan.children.iter().map(|c| c.label.as_str()).collect();
        for child in &plan.children {
            for dep in &child.depends_on {
                if !labels.contains(&dep.as_str()) {
                    anyhow::bail!(
                        "Child '{}' depends on '{}' which is not in the plan",
                        child.label,
                        dep
                    );
                }
            }
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_plan() {
        let json = r#"{
            "children": [
                {"label": "a", "description": "do A", "scope": ["a.rs"], "depends_on": []},
                {"label": "b", "description": "do B", "scope": ["b.rs"], "depends_on": ["a"]}
            ],
            "rationale": "test"
        }"#;
        let plan = CleavePlan::from_json(json).unwrap();
        assert_eq!(plan.children.len(), 2);
        assert_eq!(plan.children[1].depends_on, vec!["a"]);
    }

    #[test]
    fn parse_plan_without_rationale() {
        let json = r#"{
            "children": [
                {"label": "a", "description": "do A", "scope": ["a.rs"]},
                {"label": "b", "description": "do B", "scope": ["b.rs"]}
            ]
        }"#;
        let plan = CleavePlan::from_json(json).unwrap();
        assert_eq!(plan.children.len(), 2);
        assert_eq!(plan.rationale, "");
    }

    #[test]
    fn accept_single_child() {
        let json = r#"{
            "children": [{"label": "a", "description": "do A", "scope": ["a.rs"]}],
            "rationale": "test"
        }"#;
        let plan = CleavePlan::from_json(json).unwrap();
        assert_eq!(plan.children.len(), 1);
    }

    #[test]
    fn reject_bad_dependency() {
        let json = r#"{
            "children": [
                {"label": "a", "description": "do A", "scope": ["a.rs"]},
                {"label": "b", "description": "do B", "scope": ["b.rs"], "depends_on": ["nonexistent"]}
            ],
            "rationale": "test"
        }"#;
        assert!(CleavePlan::from_json(json).is_err());
    }

    #[test]
    fn parse_runtime_profile_fields() {
        let json = r#"{
            "children": [
                {
                    "label": "a",
                    "description": "do A",
                    "scope": ["a.rs"],
                    "runtime": {
                        "thinkingLevel": "high",
                        "contextClass": "massive",
                        "enabledTools": ["read", "bash"],
                        "disabledTools": ["web_search"],
                        "skills": ["rust", "security"],
                        "enabledExtensions": ["scribe-rpc"],
                        "disabledExtensions": ["legacy-http"],
                        "preloadedFiles": ["docs/spec.md"],
                        "nexProfile": "sandboxed",
                        "slim": true
                    }
                }
            ]
        }"#;
        let plan = CleavePlan::from_json(json).unwrap();
        let runtime = plan.children[0].runtime.as_ref().unwrap();
        assert_eq!(runtime.thinking_level.as_deref(), Some("high"));
        assert_eq!(runtime.context_class.as_deref(), Some("massive"));
        assert_eq!(runtime.enabled_tools, vec!["read", "bash"]);
        assert_eq!(runtime.disabled_tools, vec!["web_search"]);
        assert_eq!(runtime.skills, vec!["rust", "security"]);
        assert_eq!(runtime.enabled_extensions, vec!["scribe-rpc"]);
        assert_eq!(runtime.disabled_extensions, vec!["legacy-http"]);
        assert_eq!(runtime.preloaded_files, vec!["docs/spec.md"]);
        assert_eq!(runtime.nex_profile.as_deref(), Some("sandboxed"));
        assert!(runtime.slim);
    }

    #[test]
    fn parse_runtime_persona() {
        let json = r#"{
            "children": [
                {
                    "label": "review",
                    "description": "security review",
                    "scope": ["auth/"],
                    "runtime": {
                        "persona": "security-auditor"
                    }
                }
            ]
        }"#;
        let plan = CleavePlan::from_json(json).unwrap();
        let runtime = plan.children[0].runtime.as_ref().unwrap();
        assert_eq!(runtime.persona.as_deref(), Some("security-auditor"));
    }

    #[test]
    fn persona_absent_by_default() {
        let profile = CleaveChildRuntimeProfile::default();
        assert_eq!(profile.persona, None);
    }
}

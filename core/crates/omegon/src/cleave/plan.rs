//! Cleave plan — the input specification for a cleave run.

use serde::{Deserialize, Serialize};

/// A cleave plan describes children to dispatch and their dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleavePlan {
    pub children: Vec<ChildPlan>,
    #[serde(default)]
    pub rationale: String,
}

/// A single child in the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildPlan {
    pub label: String,
    pub description: String,
    pub scope: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
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
                        child.label, dep
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
}

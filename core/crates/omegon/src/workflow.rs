//! Workflow templates — declarative per-phase configuration for lifecycle-driven work.
//!
//! Templates live in `.omegon/workflows/<name>.toml` and define persona, model,
//! max_turns, and context_class for each lifecycle phase. The daemon dispatch bridge
//! and cleave orchestrator consult these when dispatching work.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// A parsed workflow template.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowTemplate {
    pub workflow: WorkflowMeta,
    #[serde(default)]
    pub phases: WorkflowPhases,
}

/// Top-level metadata for a workflow template.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Per-phase configuration. Each field is optional — only present phases are configured.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WorkflowPhases {
    pub exploring: Option<PhaseConfig>,
    pub specifying: Option<PhaseConfig>,
    pub decomposing: Option<PhaseConfig>,
    pub implementing: Option<PhaseConfig>,
    pub verifying: Option<PhaseConfig>,
}

/// Configuration for a single lifecycle phase.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PhaseConfig {
    pub persona: Option<String>,
    pub model: Option<String>,
    pub max_turns: Option<u32>,
    pub context_class: Option<String>,
    pub thinking_level: Option<String>,
}

impl WorkflowTemplate {
    /// Parse a workflow template from a TOML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let template: Self = toml::from_str(&content)?;
        Ok(template)
    }

    /// Get the phase config for a lifecycle phase.
    pub fn phase_config(&self, phase: &omegon_traits::LifecyclePhase) -> Option<&PhaseConfig> {
        use omegon_traits::LifecyclePhase;
        match phase {
            LifecyclePhase::Exploring { .. } => self.phases.exploring.as_ref(),
            LifecyclePhase::Specifying { .. } => self.phases.specifying.as_ref(),
            LifecyclePhase::Decomposing => self.phases.decomposing.as_ref(),
            LifecyclePhase::Implementing { .. } => self.phases.implementing.as_ref(),
            LifecyclePhase::Verifying { .. } => self.phases.verifying.as_ref(),
            LifecyclePhase::Idle => None,
        }
    }
}

/// Scan `.omegon/workflows/` for TOML templates. Returns the first valid one found
/// (sorted alphabetically by filename).
pub fn discover_workflow(cwd: &Path) -> Option<WorkflowTemplate> {
    let workflows_dir = cwd.join(".omegon").join("workflows");
    if !workflows_dir.is_dir() {
        return None;
    }
    let mut entries: Vec<_> = std::fs::read_dir(&workflows_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        match WorkflowTemplate::load(&entry.path()) {
            Ok(t) => {
                tracing::info!(
                    workflow = %t.workflow.name,
                    path = %entry.path().display(),
                    "loaded workflow template"
                );
                return Some(t);
            }
            Err(e) => {
                tracing::warn!(
                    path = %entry.path().display(),
                    error = %e,
                    "skipping invalid workflow template"
                );
            }
        }
    }
    None
}

/// A design-tree node that is ready for autonomous dispatch.
#[derive(Debug, Clone)]
pub struct ReadyNode {
    pub id: String,
    pub title: String,
    pub priority: Option<u8>,
}

/// Query the design tree for nodes that are ready to implement:
/// status == Decided, all dependencies Implemented, not archived.
/// Reads directly from filesystem — no bus or Feature access required.
pub fn query_ready_nodes(cwd: &Path) -> Vec<ReadyNode> {
    use crate::lifecycle::{design, types::NodeStatus};

    let docs_dir = cwd.join("docs");
    if !docs_dir.is_dir() {
        return Vec::new();
    }
    let nodes = design::scan_design_docs(&docs_dir);
    nodes
        .values()
        .filter(|n| !matches!(n.status, NodeStatus::Archived))
        .filter(|n| matches!(n.status, NodeStatus::Decided))
        .filter(|n| {
            n.dependencies.iter().all(|dep_id| {
                nodes
                    .get(dep_id)
                    .is_some_and(|d| matches!(d.status, NodeStatus::Implemented))
            })
        })
        .map(|n| ReadyNode {
            id: n.id.clone(),
            title: n.title.clone(),
            priority: n.priority,
        })
        .collect()
}

/// Build a prompt for a ready design-tree node, suitable for daemon dispatch.
pub fn build_dispatch_prompt(node: &ReadyNode) -> String {
    format!(
        "Implement design node `{}`: {}\n\n\
         This node has been marked as decided and all dependencies are satisfied. \
         Transition it to implementing, create the necessary changes, and verify \
         the implementation meets the design criteria.",
        node.id, node.title
    )
}

/// Apply workflow phase config to a LoopConfig for a given lifecycle phase.
pub fn apply_phase_config(
    loop_config: &mut crate::r#loop::LoopConfig,
    phase_config: &PhaseConfig,
    shared_settings: &crate::settings::SharedSettings,
) {
    if let Some(ref model) = phase_config.model {
        loop_config.model = model.clone();
        if let Ok(mut s) = shared_settings.lock() {
            s.set_model(model);
        }
    }
    if let Some(max_turns) = phase_config.max_turns {
        loop_config.max_turns = max_turns;
        loop_config.soft_limit_turns = if max_turns > 0 { max_turns * 2 / 3 } else { 0 };
    }
    // Persona is handled separately via OMEGON_CHILD_PERSONA env var.
    // Context class and thinking level are handled via shared settings.
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = r#"
[workflow]
name = "standard-feature"
description = "Standard feature development workflow"

[phases.exploring]
persona = "researcher"
model = "gemini-flash"
max_turns = 30

[phases.implementing]
persona = "systems-engineer"
model = "claude-sonnet-4-6"
max_turns = 60
context_class = "clan"

[phases.verifying]
persona = "security-auditor"
model = "claude-opus-4-6"
max_turns = 20
thinking_level = "high"
"#;

    #[test]
    fn parse_workflow_template() {
        let template: WorkflowTemplate = toml::from_str(EXAMPLE_TOML).unwrap();
        assert_eq!(template.workflow.name, "standard-feature");
        assert_eq!(
            template.workflow.description,
            "Standard feature development workflow"
        );
    }

    #[test]
    fn phase_configs_present() {
        let template: WorkflowTemplate = toml::from_str(EXAMPLE_TOML).unwrap();

        let exploring = template.phases.exploring.as_ref().unwrap();
        assert_eq!(exploring.persona.as_deref(), Some("researcher"));
        assert_eq!(exploring.model.as_deref(), Some("gemini-flash"));
        assert_eq!(exploring.max_turns, Some(30));

        let implementing = template.phases.implementing.as_ref().unwrap();
        assert_eq!(implementing.persona.as_deref(), Some("systems-engineer"));
        assert_eq!(implementing.context_class.as_deref(), Some("clan"));

        let verifying = template.phases.verifying.as_ref().unwrap();
        assert_eq!(verifying.thinking_level.as_deref(), Some("high"));
    }

    #[test]
    fn unconfigured_phases_are_none() {
        let template: WorkflowTemplate = toml::from_str(EXAMPLE_TOML).unwrap();
        assert!(template.phases.specifying.is_none());
        assert!(template.phases.decomposing.is_none());
    }

    #[test]
    fn phase_config_lookup() {
        let template: WorkflowTemplate = toml::from_str(EXAMPLE_TOML).unwrap();

        let result =
            template.phase_config(&omegon_traits::LifecyclePhase::Exploring { node_id: None });
        assert!(result.is_some());
        assert_eq!(result.unwrap().persona.as_deref(), Some("researcher"));

        let result = template.phase_config(&omegon_traits::LifecyclePhase::Idle);
        assert!(result.is_none());

        let result =
            template.phase_config(&omegon_traits::LifecyclePhase::Specifying { change_id: None });
        assert!(result.is_none());
    }

    #[test]
    fn minimal_template() {
        let toml_str = r#"
[workflow]
name = "bare"

[phases.implementing]
model = "ollama:llama3"
"#;
        let template: WorkflowTemplate = toml::from_str(toml_str).unwrap();
        assert_eq!(template.workflow.name, "bare");
        assert!(template.phases.implementing.is_some());
        assert!(template.phases.exploring.is_none());
    }
}

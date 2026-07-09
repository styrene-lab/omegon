use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::agent_manifest::ResolvedManifest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentBundleSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub domain: String,
    pub source_path: String,
    pub persona: AgentPersonaSummary,
    pub extensions: Vec<AgentExtensionDependency>,
    pub settings: AgentSettingsSummary,
    pub workflow: Option<AgentWorkflowSummary>,
    pub secrets: AgentSecretsSummary,
    pub triggers: Vec<AgentTriggerSummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPersonaSummary {
    pub badge: Option<String>,
    pub has_directive: bool,
    pub has_mind_facts: bool,
    pub activated_skills: Vec<String>,
    pub disabled_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentExtensionDependency {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSettingsSummary {
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub context_class: Option<String>,
    pub max_turns: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorkflowSummary {
    pub name: String,
    pub phases: Vec<AgentWorkflowPhaseSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWorkflowPhaseSummary {
    pub name: String,
    pub model: Option<String>,
    pub max_turns: Option<u32>,
    pub thinking_level: Option<String>,
    pub context_class: Option<String>,
    pub persona: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSecretsSummary {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTriggerSummary {
    pub name: String,
    pub schedule: Option<String>,
    pub interval: Option<String>,
    pub template_preview: String,
}

pub fn list_agent_bundle_summaries_from_dir(
    catalog_dir: &Path,
) -> anyhow::Result<Vec<AgentBundleSummary>> {
    if !catalog_dir.exists() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();
    for entry in std::fs::read_dir(catalog_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join("agent.toml").exists() && !path.join("agent.pkl").exists() {
            continue;
        }
        match agent_bundle_summary_from_dir(&path) {
            Ok(summary) => summaries.push(summary),
            Err(error) => {
                tracing::warn!(
                    error = ?error,
                    path = %path.display(),
                    "skipping invalid catalog agent bundle summary"
                );
            }
        }
    }
    summaries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(summaries)
}

pub fn agent_bundle_summary_from_dir(bundle_dir: &Path) -> anyhow::Result<AgentBundleSummary> {
    let resolved = crate::agent_manifest::load(bundle_dir)?;
    Ok(agent_bundle_summary(resolved))
}

pub(crate) fn agent_bundle_summary(resolved: ResolvedManifest) -> AgentBundleSummary {
    let manifest = resolved.manifest;
    let persona = manifest.persona.as_ref();

    let mut extensions: Vec<_> = manifest
        .extensions
        .unwrap_or_default()
        .into_iter()
        .map(|dep| AgentExtensionDependency {
            name: dep.name,
            version: dep.version,
        })
        .collect();
    extensions.sort_by(|a, b| a.name.cmp(&b.name));

    let settings = manifest
        .settings
        .unwrap_or(crate::agent_manifest::SettingsConfig {
            model: None,
            thinking_level: None,
            context_class: None,
            max_turns: None,
        });

    let workflow = manifest.workflow.map(|workflow| {
        let mut phases: Vec<_> = workflow
            .phases
            .unwrap_or_default()
            .into_iter()
            .map(|(name, phase)| AgentWorkflowPhaseSummary {
                name,
                model: phase.model,
                max_turns: phase.max_turns,
                thinking_level: phase.thinking_level,
                context_class: phase.context_class,
                persona: phase.persona,
            })
            .collect();
        phases.sort_by(|a, b| a.name.cmp(&b.name));
        AgentWorkflowSummary {
            name: workflow.name,
            phases,
        }
    });

    let secrets = manifest
        .secrets
        .unwrap_or(crate::agent_manifest::SecretsConfig {
            required: None,
            optional: None,
        });

    let mut triggers: Vec<_> = manifest
        .triggers
        .unwrap_or_default()
        .into_iter()
        .map(|trigger| AgentTriggerSummary {
            name: trigger.name,
            schedule: trigger.schedule,
            interval: trigger.interval,
            template_preview: preview(&trigger.template, 160),
        })
        .collect();
    triggers.sort_by(|a, b| a.name.cmp(&b.name));

    AgentBundleSummary {
        id: manifest.agent.id,
        name: manifest.agent.name,
        version: manifest.agent.version,
        description: manifest.agent.description,
        domain: manifest.agent.domain,
        source_path: resolved.bundle_dir.display().to_string(),
        persona: AgentPersonaSummary {
            badge: persona.and_then(|p| p.badge.clone()),
            has_directive: resolved.persona_directive.is_some(),
            has_mind_facts: resolved.mind_facts_content.is_some(),
            activated_skills: persona
                .and_then(|p| p.activated_skills.clone())
                .unwrap_or_default(),
            disabled_tools: persona
                .and_then(|p| p.disabled_tools.clone())
                .unwrap_or_default(),
        },
        extensions,
        settings: AgentSettingsSummary {
            model: settings.model,
            thinking_level: settings.thinking_level,
            context_class: settings.context_class,
            max_turns: settings.max_turns,
        },
        workflow,
        secrets: AgentSecretsSummary {
            required: secrets.required.unwrap_or_default(),
            optional: secrets.optional.unwrap_or_default(),
        },
        triggers,
    }
}

fn preview(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    compact.chars().take(max_chars).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_agent_bundle_template_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let bundle_dir = temp.path().join("styrene.coding-agent");
        std::fs::create_dir_all(bundle_dir.join("mind")).unwrap();
        std::fs::write(
            bundle_dir.join("PERSONA.md"),
            "You are a test coding agent.",
        )
        .unwrap();
        std::fs::write(bundle_dir.join("mind/facts.jsonl"), "{\"fact\":\"test\"}\n").unwrap();
        std::fs::write(
            bundle_dir.join("agent.toml"),
            r#"
[agent]
id = "styrene.coding-agent"
name = "Software Engineer"
version = "1.0.0"
description = "General-purpose engineering agent"
domain = "coding"

[persona]
directive = "PERSONA.md"
badge = "dev"
mind_facts = "mind/facts.jsonl"
activated_skills = ["rust"]
disabled_tools = ["terminal"]

[[extensions]]
name = "vox"
version = ">=0.3.0"

[settings]
model = "anthropic:claude-sonnet-4-6"
thinking_level = "medium"
context_class = "squad"
max_turns = 50

[workflow]
name = "standard"

[workflow.phases.exploring]
model = "anthropic:claude-opus-4-6"
max_turns = 20
thinking_level = "high"

[secrets]
required = ["ANTHROPIC_API_KEY"]
optional = ["GITHUB_TOKEN"]

[[triggers]]
name = "daily-review"
schedule = "daily"
template = "Review the project status and summarize blockers for the operator."
"#,
        )
        .unwrap();

        let summaries = list_agent_bundle_summaries_from_dir(temp.path()).unwrap();

        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.id, "styrene.coding-agent");
        assert_eq!(summary.persona.badge.as_deref(), Some("dev"));
        assert!(summary.persona.has_directive);
        assert!(summary.persona.has_mind_facts);
        assert_eq!(summary.extensions[0].name, "vox");
        assert_eq!(summary.settings.context_class.as_deref(), Some("squad"));
        assert_eq!(
            summary.workflow.as_ref().unwrap().phases[0].name,
            "exploring"
        );
        assert_eq!(summary.secrets.required, vec!["ANTHROPIC_API_KEY"]);
        assert_eq!(summary.triggers[0].name, "daily-review");
    }

    #[test]
    fn skips_invalid_agent_bundle_entries() {
        let temp = tempfile::tempdir().unwrap();
        let valid_dir = temp.path().join("valid-agent");
        std::fs::create_dir_all(&valid_dir).unwrap();
        std::fs::write(
            valid_dir.join("agent.toml"),
            r#"[agent]
id = "valid-agent"
name = "Valid Agent"
version = "0.1.0"
description = "Valid catalog agent"
domain = "test"
"#,
        )
        .unwrap();
        let broken_dir = temp.path().join("broken-agent");
        std::fs::create_dir_all(&broken_dir).unwrap();
        std::fs::write(
            broken_dir.join("agent.toml"),
            "[agent
id = ",
        )
        .unwrap();

        let summaries = list_agent_bundle_summaries_from_dir(temp.path()).unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "valid-agent");
    }

    #[test]
    fn missing_catalog_dir_is_empty_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("catalog");
        let summaries = list_agent_bundle_summaries_from_dir(&missing).unwrap();
        assert!(summaries.is_empty());
    }
}

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::agents::AgentBundleSummary;
use super::extensions::ExtensionCapabilitySummary;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretReadinessSnapshot {
    pub secrets: Vec<SecretReadiness>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretReadiness {
    pub name: String,
    pub required: bool,
    pub optional: bool,
    pub consumers: Vec<SecretConsumer>,
    pub status: SecretReadinessStatus,
    pub recipe_kind: Option<String>,
    pub warmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretConsumer {
    pub kind: SecretConsumerKind,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretConsumerKind {
    Extension,
    AgentBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretReadinessStatus {
    Warmed,
    Configured,
    Deferred,
    Missing,
}

#[derive(Debug, Clone, Default)]
pub struct SecretReadinessInputs {
    pub session_diagnostics: Vec<SecretSessionDiagnostic>,
    pub recipe_descriptors: Vec<SecretRecipeDescriptorSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretSessionDiagnostic {
    pub name: String,
    pub warmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRecipeDescriptorSummary {
    pub name: String,
    pub kind: String,
}

pub fn build_secret_readiness_snapshot(
    extensions: &[ExtensionCapabilitySummary],
    agents: &[AgentBundleSummary],
    inputs: SecretReadinessInputs,
) -> SecretReadinessSnapshot {
    let mut requirements: BTreeMap<String, SecretRequirementAccumulator> = BTreeMap::new();

    for extension in extensions {
        for name in &extension.required_secrets {
            requirements.entry(name.clone()).or_default().required = true;
            requirements
                .entry(name.clone())
                .or_default()
                .consumers
                .insert((SecretConsumerKind::Extension, extension.name.clone()));
        }
        for name in &extension.optional_secrets {
            requirements.entry(name.clone()).or_default().optional = true;
            requirements
                .entry(name.clone())
                .or_default()
                .consumers
                .insert((SecretConsumerKind::Extension, extension.name.clone()));
        }
    }

    for agent in agents {
        for name in &agent.secrets.required {
            requirements.entry(name.clone()).or_default().required = true;
            requirements
                .entry(name.clone())
                .or_default()
                .consumers
                .insert((SecretConsumerKind::AgentBundle, agent.id.clone()));
        }
        for name in &agent.secrets.optional {
            requirements.entry(name.clone()).or_default().optional = true;
            requirements
                .entry(name.clone())
                .or_default()
                .consumers
                .insert((SecretConsumerKind::AgentBundle, agent.id.clone()));
        }
    }

    let warmed: BTreeSet<_> = inputs
        .session_diagnostics
        .into_iter()
        .filter(|diag| diag.warmed)
        .map(|diag| diag.name)
        .collect();
    let recipes: BTreeMap<_, _> = inputs
        .recipe_descriptors
        .into_iter()
        .map(|descriptor| (descriptor.name, descriptor.kind))
        .collect();

    let secrets = requirements
        .into_iter()
        .map(|(name, requirement)| {
            let warmed = warmed.contains(&name);
            let recipe_kind = recipes.get(&name).cloned();
            let status = if warmed {
                SecretReadinessStatus::Warmed
            } else if matches!(recipe_kind.as_deref(), Some("vault")) {
                SecretReadinessStatus::Deferred
            } else if recipe_kind.is_some() {
                SecretReadinessStatus::Configured
            } else {
                SecretReadinessStatus::Missing
            };
            SecretReadiness {
                name,
                required: requirement.required,
                optional: requirement.optional,
                consumers: requirement
                    .consumers
                    .into_iter()
                    .map(|(kind, id)| SecretConsumer { kind, id })
                    .collect(),
                status,
                recipe_kind,
                warmed,
            }
        })
        .collect();

    SecretReadinessSnapshot { secrets }
}

#[derive(Default)]
struct SecretRequirementAccumulator {
    required: bool,
    optional: bool,
    consumers: BTreeSet<(SecretConsumerKind, String)>,
}

impl Ord for SecretConsumerKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        format!("{:?}", self).cmp(&format!("{:?}", other))
    }
}

impl PartialOrd for SecretConsumerKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::agents::{
        AgentPersonaSummary, AgentSecretsSummary, AgentSettingsSummary,
    };

    #[test]
    fn secret_readiness_uses_metadata_without_resolving_values() {
        let agent = AgentBundleSummary {
            id: "daily".into(),
            name: "Daily".into(),
            version: "0.1.0".into(),
            description: String::new(),
            domain: "ops".into(),
            source_path: "/catalog/daily".into(),
            persona: AgentPersonaSummary::default(),
            extensions: Vec::new(),
            settings: AgentSettingsSummary::default(),
            workflow: None,
            secrets: AgentSecretsSummary {
                required: vec!["ANTHROPIC_API_KEY".into()],
                optional: vec!["VAULT_TOKEN".into(), "MISSING_OPTIONAL".into()],
            },
            triggers: Vec::new(),
        };

        let snapshot = build_secret_readiness_snapshot(
            &[],
            &[agent],
            SecretReadinessInputs {
                session_diagnostics: vec![SecretSessionDiagnostic {
                    name: "ANTHROPIC_API_KEY".into(),
                    warmed: true,
                }],
                recipe_descriptors: vec![SecretRecipeDescriptorSummary {
                    name: "VAULT_TOKEN".into(),
                    kind: "vault".into(),
                }],
            },
        );

        let anthropic = snapshot
            .secrets
            .iter()
            .find(|secret| secret.name == "ANTHROPIC_API_KEY")
            .unwrap();
        assert_eq!(anthropic.status, SecretReadinessStatus::Warmed);
        assert!(anthropic.required);
        assert_eq!(anthropic.recipe_kind, None);

        let vault = snapshot
            .secrets
            .iter()
            .find(|secret| secret.name == "VAULT_TOKEN")
            .unwrap();
        assert_eq!(vault.status, SecretReadinessStatus::Deferred);
        assert_eq!(vault.recipe_kind.as_deref(), Some("vault"));

        let missing = snapshot
            .secrets
            .iter()
            .find(|secret| secret.name == "MISSING_OPTIONAL")
            .unwrap();
        assert_eq!(missing.status, SecretReadinessStatus::Missing);
        assert!(missing.optional);
    }
}

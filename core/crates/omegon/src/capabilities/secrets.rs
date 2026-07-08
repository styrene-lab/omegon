use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::agents::AgentBundleSummary;
use super::extensions::ExtensionCapabilitySummary;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretReadinessSnapshot {
    pub secrets: Vec<SecretReadiness>,
    pub harness_capabilities: Vec<HarnessCapabilitySecretReadiness>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarnessCapabilitySecretReadiness {
    pub id: String,
    pub label: String,
    pub category: HarnessCapabilityCategory,
    pub description: String,
    pub policy: HarnessCapabilitySecretPolicy,
    pub candidate_label: String,
    pub secret_names: Vec<String>,
    pub preferred_secret: Option<String>,
    pub configured_count: usize,
    pub deferred_count: usize,
    pub missing_count: usize,
    pub candidate_count: usize,
    pub status: HarnessCapabilityReadinessStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCapabilityCategory {
    LlmProvider,
    Research,
    Forge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCapabilitySecretPolicy {
    AnyOf,
    AllOf,
}

impl HarnessCapabilityCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::LlmProvider => "LLM providers",
            Self::Research => "research",
            Self::Forge => "forge",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCapabilityReadinessStatus {
    Ready,
    Partial,
    Missing,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SecretConsumerKind {
    Extension,
    AgentBundle,
    HarnessCapability,
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

    seed_first_party_secret_catalog(&mut requirements);

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

    for name in warmed.iter().chain(recipes.keys()) {
        requirements.entry(name.clone()).or_default();
    }

    let secrets: Vec<_> = requirements
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

    let harness_capabilities = build_harness_capability_readiness(&secrets);

    SecretReadinessSnapshot {
        secrets,
        harness_capabilities,
    }
}

#[derive(Default)]
struct SecretRequirementAccumulator {
    required: bool,
    optional: bool,
    consumers: BTreeSet<(SecretConsumerKind, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FirstPartySecretCatalogEntry {
    name: &'static str,
    capability_id: &'static str,
    capability_label: &'static str,
    category: HarnessCapabilityCategory,
    candidate_label: &'static str,
    description: &'static str,
    policy: HarnessCapabilitySecretPolicy,
    preferred: bool,
}

const FIRST_PARTY_SECRET_CATALOG: &[FirstPartySecretCatalogEntry] = &[
    FirstPartySecretCatalogEntry {
        name: "ANTHROPIC_API_KEY",
        capability_id: "llm_provider_api_keys",
        capability_label: "LLM provider API keys",
        category: HarnessCapabilityCategory::LlmProvider,
        candidate_label: "API keys",
        description: "API-key credentials for built-in LLM provider routes when OAuth is not used.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: true,
    },
    FirstPartySecretCatalogEntry {
        name: "OPENAI_API_KEY",
        capability_id: "llm_provider_api_keys",
        capability_label: "LLM provider API keys",
        category: HarnessCapabilityCategory::LlmProvider,
        candidate_label: "API keys",
        description: "API-key credentials for built-in LLM provider routes when OAuth is not used.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: false,
    },
    FirstPartySecretCatalogEntry {
        name: "OPENROUTER_API_KEY",
        capability_id: "llm_provider_api_keys",
        capability_label: "LLM provider API keys",
        category: HarnessCapabilityCategory::LlmProvider,
        candidate_label: "API keys",
        description: "API-key credentials for built-in LLM provider routes when OAuth is not used.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: false,
    },
    FirstPartySecretCatalogEntry {
        name: "BRAVE_API_KEY",
        capability_id: "web_search",
        capability_label: "Web search and external evidence",
        category: HarnessCapabilityCategory::Research,
        candidate_label: "providers",
        description: "Search provider credentials for first-party external research tools.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: true,
    },
    FirstPartySecretCatalogEntry {
        name: "TAVILY_API_KEY",
        capability_id: "web_search",
        capability_label: "Web search and external evidence",
        category: HarnessCapabilityCategory::Research,
        candidate_label: "providers",
        description: "Search provider credentials for first-party external research tools.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: false,
    },
    FirstPartySecretCatalogEntry {
        name: "SERPER_API_KEY",
        capability_id: "web_search",
        capability_label: "Web search and external evidence",
        category: HarnessCapabilityCategory::Research,
        candidate_label: "providers",
        description: "Search provider credentials for first-party external research tools.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: false,
    },
    FirstPartySecretCatalogEntry {
        name: "FIRECRAWL_API_KEY",
        capability_id: "web_search",
        capability_label: "Web search and external evidence",
        category: HarnessCapabilityCategory::Research,
        candidate_label: "providers",
        description: "Search provider credentials for first-party external research tools.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: false,
    },
    FirstPartySecretCatalogEntry {
        name: "GITHUB_TOKEN",
        capability_id: "github_forge",
        capability_label: "GitHub forge integration",
        category: HarnessCapabilityCategory::Forge,
        candidate_label: "tokens",
        description: "Tokens for first-party GitHub issue, PR, and repository workflows.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: true,
    },
    FirstPartySecretCatalogEntry {
        name: "GH_TOKEN",
        capability_id: "github_forge",
        capability_label: "GitHub forge integration",
        category: HarnessCapabilityCategory::Forge,
        candidate_label: "tokens",
        description: "Tokens for first-party GitHub issue, PR, and repository workflows.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: false,
    },
    FirstPartySecretCatalogEntry {
        name: "GITLAB_TOKEN",
        capability_id: "gitlab_forge",
        capability_label: "GitLab forge integration",
        category: HarnessCapabilityCategory::Forge,
        candidate_label: "tokens",
        description: "Tokens for first-party GitLab issue, merge-request, and repository workflows.",
        policy: HarnessCapabilitySecretPolicy::AnyOf,
        preferred: true,
    },
];

fn seed_first_party_secret_catalog(
    requirements: &mut BTreeMap<String, SecretRequirementAccumulator>,
) {
    for entry in FIRST_PARTY_SECRET_CATALOG {
        let requirement = requirements.entry(entry.name.to_string()).or_default();
        requirement.optional = true;
        requirement.consumers.insert((
            SecretConsumerKind::HarnessCapability,
            entry.capability_id.to_string(),
        ));
    }
}

fn build_harness_capability_readiness(
    secrets: &[SecretReadiness],
) -> Vec<HarnessCapabilitySecretReadiness> {
    let secrets_by_name: BTreeMap<_, _> = secrets
        .iter()
        .map(|secret| (secret.name.as_str(), secret))
        .collect();
    let mut capabilities: BTreeMap<&'static str, HarnessCapabilitySecretReadiness> =
        BTreeMap::new();

    for entry in FIRST_PARTY_SECRET_CATALOG {
        let capability = capabilities.entry(entry.capability_id).or_insert_with(|| {
            HarnessCapabilitySecretReadiness {
                id: entry.capability_id.to_string(),
                label: entry.capability_label.to_string(),
                category: entry.category.clone(),
                description: entry.description.to_string(),
                policy: entry.policy.clone(),
                candidate_label: entry.candidate_label.to_string(),
                secret_names: Vec::new(),
                preferred_secret: None,
                configured_count: 0,
                deferred_count: 0,
                missing_count: 0,
                candidate_count: 0,
                status: HarnessCapabilityReadinessStatus::Missing,
            }
        });
        capability.secret_names.push(entry.name.to_string());
        capability.candidate_count += 1;
        if entry.preferred {
            capability.preferred_secret = Some(entry.name.to_string());
        }

        match secrets_by_name.get(entry.name).map(|secret| &secret.status) {
            Some(SecretReadinessStatus::Warmed | SecretReadinessStatus::Configured) => {
                capability.configured_count += 1;
            }
            Some(SecretReadinessStatus::Deferred) => {
                capability.deferred_count += 1;
            }
            Some(SecretReadinessStatus::Missing) | None => {
                capability.missing_count += 1;
            }
        }
    }

    for capability in capabilities.values_mut() {
        let available_count = capability.configured_count + capability.deferred_count;
        capability.status = match capability.policy {
            HarnessCapabilitySecretPolicy::AnyOf => {
                if available_count > 0 {
                    HarnessCapabilityReadinessStatus::Ready
                } else {
                    HarnessCapabilityReadinessStatus::Missing
                }
            }
            HarnessCapabilitySecretPolicy::AllOf => {
                if capability.missing_count == 0 {
                    HarnessCapabilityReadinessStatus::Ready
                } else if available_count > 0 {
                    HarnessCapabilityReadinessStatus::Partial
                } else {
                    HarnessCapabilityReadinessStatus::Missing
                }
            }
        };
    }

    capabilities.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::agents::{
        AgentPersonaSummary, AgentSecretsSummary, AgentSettingsSummary,
    };

    #[test]
    fn first_party_secret_catalog_surfaces_core_harness_capabilities() {
        let snapshot = build_secret_readiness_snapshot(&[], &[], SecretReadinessInputs::default());

        for (name, capability) in [
            ("ANTHROPIC_API_KEY", "llm_provider_api_keys"),
            ("OPENAI_API_KEY", "llm_provider_api_keys"),
            ("OPENROUTER_API_KEY", "llm_provider_api_keys"),
            ("BRAVE_API_KEY", "web_search"),
            ("TAVILY_API_KEY", "web_search"),
            ("SERPER_API_KEY", "web_search"),
            ("FIRECRAWL_API_KEY", "web_search"),
            ("GITHUB_TOKEN", "github_forge"),
            ("GH_TOKEN", "github_forge"),
            ("GITLAB_TOKEN", "gitlab_forge"),
        ] {
            let secret = snapshot
                .secrets
                .iter()
                .find(|secret| secret.name == name)
                .unwrap_or_else(|| panic!("missing first-party secret catalog entry for {name}"));
            assert_eq!(secret.status, SecretReadinessStatus::Missing);
            assert!(!secret.required);
            assert!(secret.optional);
            assert!(secret.consumers.iter().any(|consumer| {
                consumer.kind == SecretConsumerKind::HarnessCapability && consumer.id == capability
            }));
        }
    }

    #[test]
    fn harness_capability_readiness_groups_first_party_secret_catalog() {
        let snapshot = build_secret_readiness_snapshot(
            &[],
            &[],
            SecretReadinessInputs {
                session_diagnostics: Vec::new(),
                recipe_descriptors: vec![SecretRecipeDescriptorSummary {
                    name: "BRAVE_API_KEY".into(),
                    kind: "env".into(),
                }],
            },
        );

        let web_search = snapshot
            .harness_capabilities
            .iter()
            .find(|capability| capability.id == "web_search")
            .expect("web_search harness capability readiness");
        assert_eq!(web_search.label, "Web search and external evidence");
        assert_eq!(web_search.category, HarnessCapabilityCategory::Research);
        assert_eq!(web_search.configured_count, 1);
        assert_eq!(web_search.deferred_count, 0);
        assert_eq!(web_search.missing_count, 3);
        assert_eq!(web_search.candidate_count, 4);
        assert_eq!(
            web_search.preferred_secret.as_deref(),
            Some("BRAVE_API_KEY")
        );
        assert_eq!(web_search.policy, HarnessCapabilitySecretPolicy::AnyOf);
        assert_eq!(web_search.status, HarnessCapabilityReadinessStatus::Ready);
        assert!(web_search.secret_names.contains(&"BRAVE_API_KEY".into()));
        assert!(web_search.secret_names.contains(&"TAVILY_API_KEY".into()));
    }

    #[test]
    fn deferred_any_of_capability_counts_as_ready_but_deferred() {
        let snapshot = build_secret_readiness_snapshot(
            &[],
            &[],
            SecretReadinessInputs {
                session_diagnostics: Vec::new(),
                recipe_descriptors: vec![SecretRecipeDescriptorSummary {
                    name: "BRAVE_API_KEY".into(),
                    kind: "vault".into(),
                }],
            },
        );

        let web_search = snapshot
            .harness_capabilities
            .iter()
            .find(|capability| capability.id == "web_search")
            .expect("web_search harness capability readiness");
        assert_eq!(web_search.configured_count, 0);
        assert_eq!(web_search.deferred_count, 1);
        assert_eq!(web_search.missing_count, 3);
        assert_eq!(web_search.status, HarnessCapabilityReadinessStatus::Ready);
    }

    #[test]
    fn undeclared_recipe_and_warmed_secrets_surface_in_readiness() {
        let snapshot = build_secret_readiness_snapshot(
            &[],
            &[],
            SecretReadinessInputs {
                session_diagnostics: vec![SecretSessionDiagnostic {
                    name: "CUSTOM_RUNTIME_SECRET".into(),
                    warmed: true,
                }],
                recipe_descriptors: vec![SecretRecipeDescriptorSummary {
                    name: "CUSTOM_RECIPE_SECRET".into(),
                    kind: "env".into(),
                }],
            },
        );

        let warmed = snapshot
            .secrets
            .iter()
            .find(|secret| secret.name == "CUSTOM_RUNTIME_SECRET")
            .expect("warmed undeclared secret should be visible");
        assert_eq!(warmed.status, SecretReadinessStatus::Warmed);
        assert!(warmed.consumers.is_empty());

        let recipe = snapshot
            .secrets
            .iter()
            .find(|secret| secret.name == "CUSTOM_RECIPE_SECRET")
            .expect("recipe-only undeclared secret should be visible");
        assert_eq!(recipe.status, SecretReadinessStatus::Configured);
        assert_eq!(recipe.recipe_kind.as_deref(), Some("env"));
        assert!(recipe.consumers.is_empty());
    }

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

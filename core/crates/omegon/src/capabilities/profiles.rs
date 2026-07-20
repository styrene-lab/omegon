use serde::{Deserialize, Serialize};

use super::agents::AgentBundleSummary;
use super::inventory::{CapabilityEdge, CapabilityGraph, CapabilityNode, CapabilityTrustSummary};
use super::secrets::{SecretReadiness, SecretReadinessStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantProfileSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub domain: String,
    pub source_path: String,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub context_class: Option<String>,
    pub max_turns: Option<u32>,
    pub activated_skills: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub extensions: Vec<String>,
    pub required_secrets: Vec<String>,
    pub optional_secrets: Vec<String>,
    pub triggers: Vec<String>,
    pub trust: CapabilityTrustSummary,
    pub launch_readiness: AssistantLaunchReadiness,
    pub secret_readiness: AssistantSecretReadinessSummary,
    pub capability_node_ids: Vec<String>,
    pub missing_required_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantListItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    pub model: Option<String>,
    pub launch_readiness: AssistantLaunchReadiness,
    pub required_secret_count: usize,
    pub optional_secret_count: usize,
    pub blocker_count: usize,
    pub warning_count: usize,
    pub trust: CapabilityTrustSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantLaunchReadiness {
    pub status: AssistantLaunchStatus,
    pub blockers: Vec<AssistantLaunchBlocker>,
    pub warnings: Vec<AssistantLaunchWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantLaunchStatus {
    Ready,
    Degraded,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantLaunchBlocker {
    pub kind: AssistantLaunchBlockerKind,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantLaunchBlockerKind {
    RequiredSecretMissing,
    RequiredCapabilityMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantLaunchWarning {
    pub kind: AssistantLaunchWarningKind,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantLaunchWarningKind {
    OptionalSecretMissing,
    SecretDeferred,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantSecretReadinessSummary {
    pub required_total: usize,
    pub required_ready: usize,
    pub required_missing: usize,
    pub optional_total: usize,
    pub optional_ready: usize,
    pub optional_missing: usize,
    pub missing_required: Vec<String>,
    pub missing_optional: Vec<String>,
    pub deferred: Vec<String>,
}

pub fn resolve_assistant_profiles(
    agents: &[AgentBundleSummary],
    graph: &CapabilityGraph,
    secret_readiness: &[SecretReadiness],
) -> Vec<AssistantProfileSummary> {
    let mut profiles: Vec<_> = agents
        .iter()
        .map(|agent| resolve_assistant_profile(agent, graph, secret_readiness))
        .collect();
    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    profiles
}

pub fn assistant_list_items(profiles: &[AssistantProfileSummary]) -> Vec<AssistantListItem> {
    let mut items: Vec<_> = profiles
        .iter()
        .map(|profile| AssistantListItem {
            id: profile.id.clone(),
            name: profile.name.clone(),
            description: profile.description.clone(),
            domain: profile.domain.clone(),
            model: profile.model.clone(),
            launch_readiness: profile.launch_readiness.clone(),
            required_secret_count: profile.required_secrets.len(),
            optional_secret_count: profile.optional_secrets.len(),
            blocker_count: profile.launch_readiness.blockers.len(),
            warning_count: profile.launch_readiness.warnings.len(),
            trust: profile.trust.clone(),
        })
        .collect();
    items.sort_by(|a, b| {
        launch_status_rank(&a.launch_readiness.status)
            .cmp(&launch_status_rank(&b.launch_readiness.status))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
    items
}

fn launch_status_rank(status: &AssistantLaunchStatus) -> u8 {
    match status {
        AssistantLaunchStatus::Ready => 0,
        AssistantLaunchStatus::Degraded => 1,
        AssistantLaunchStatus::Blocked => 2,
    }
}

fn resolve_assistant_profile(
    agent: &AgentBundleSummary,
    graph: &CapabilityGraph,
    secret_readiness: &[SecretReadiness],
) -> AssistantProfileSummary {
    let root_id = format!("agent:{}", agent.id);
    let capability_node_ids = reachable_node_ids(&root_id, graph);
    let missing_required_node_ids: Vec<String> = capability_node_ids
        .iter()
        .filter(|id| {
            graph
                .nodes
                .iter()
                .any(|node| node.id == **id && is_missing(node))
        })
        .cloned()
        .collect();

    let secret_summary = summarize_agent_secret_readiness(agent, secret_readiness);

    AssistantProfileSummary {
        id: agent.id.clone(),
        name: agent.name.clone(),
        version: agent.version.clone(),
        description: agent.description.clone(),
        domain: agent.domain.clone(),
        source_path: agent.source_path.clone(),
        model: agent.settings.model.clone(),
        thinking_level: agent.settings.thinking_level.clone(),
        context_class: agent.settings.context_class.clone(),
        max_turns: agent.settings.max_turns,
        activated_skills: agent.persona.activated_skills.clone(),
        disabled_tools: agent.persona.disabled_tools.clone(),
        extensions: agent
            .extensions
            .iter()
            .map(|dep| dep.name.clone())
            .collect(),
        required_secrets: agent.secrets.required.clone(),
        optional_secrets: agent.secrets.optional.clone(),
        triggers: agent
            .triggers
            .iter()
            .map(|trigger| trigger.name.clone())
            .collect(),
        trust: merge_trust(&capability_node_ids, graph),
        launch_readiness: summarize_launch_readiness(&secret_summary, &missing_required_node_ids),
        secret_readiness: secret_summary,
        capability_node_ids,
        missing_required_node_ids,
    }
}

fn summarize_launch_readiness(
    secrets: &AssistantSecretReadinessSummary,
    missing_required_node_ids: &[String],
) -> AssistantLaunchReadiness {
    let mut blockers: Vec<_> = secrets
        .missing_required
        .iter()
        .map(|id| AssistantLaunchBlocker {
            kind: AssistantLaunchBlockerKind::RequiredSecretMissing,
            id: id.clone(),
        })
        .collect();
    blockers.extend(
        missing_required_node_ids
            .iter()
            .map(|id| AssistantLaunchBlocker {
                kind: AssistantLaunchBlockerKind::RequiredCapabilityMissing,
                id: id.clone(),
            }),
    );

    let mut warnings: Vec<_> = secrets
        .missing_optional
        .iter()
        .map(|id| AssistantLaunchWarning {
            kind: AssistantLaunchWarningKind::OptionalSecretMissing,
            id: id.clone(),
        })
        .collect();
    warnings.extend(secrets.deferred.iter().map(|id| AssistantLaunchWarning {
        kind: AssistantLaunchWarningKind::SecretDeferred,
        id: id.clone(),
    }));

    let status = if !blockers.is_empty() {
        AssistantLaunchStatus::Blocked
    } else if !warnings.is_empty() {
        AssistantLaunchStatus::Degraded
    } else {
        AssistantLaunchStatus::Ready
    };

    AssistantLaunchReadiness {
        status,
        blockers,
        warnings,
    }
}

fn summarize_agent_secret_readiness(
    agent: &AgentBundleSummary,
    readiness: &[SecretReadiness],
) -> AssistantSecretReadinessSummary {
    let mut summary = AssistantSecretReadinessSummary {
        required_total: agent.secrets.required.len(),
        optional_total: agent.secrets.optional.len(),
        ..Default::default()
    };

    for name in &agent.secrets.required {
        if secret_is_ready(name, readiness) {
            summary.required_ready += 1;
        } else {
            summary.required_missing += 1;
            summary.missing_required.push(name.clone());
        }
        if secret_is_deferred(name, readiness) {
            summary.deferred.push(name.clone());
        }
    }

    for name in &agent.secrets.optional {
        if secret_is_ready(name, readiness) {
            summary.optional_ready += 1;
        } else {
            summary.optional_missing += 1;
            summary.missing_optional.push(name.clone());
        }
        if secret_is_deferred(name, readiness) {
            summary.deferred.push(name.clone());
        }
    }

    summary.deferred.sort();
    summary.deferred.dedup();
    summary
}

fn secret_is_ready(name: &str, readiness: &[SecretReadiness]) -> bool {
    readiness.iter().any(|secret| {
        secret.name == name
            && matches!(
                secret.status,
                SecretReadinessStatus::Warmed | SecretReadinessStatus::Configured
            )
    })
}

fn secret_is_deferred(name: &str, readiness: &[SecretReadiness]) -> bool {
    readiness.iter().any(|secret| {
        secret.name == name && matches!(secret.status, SecretReadinessStatus::Deferred)
    })
}

fn reachable_node_ids(root_id: &str, graph: &CapabilityGraph) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![root_id.to_string()];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for edge in graph.edges.iter().filter(|edge| edge.from == id) {
            stack.push(edge.to.clone());
        }
    }
    seen.into_iter().collect()
}

fn merge_trust(node_ids: &[String], graph: &CapabilityGraph) -> CapabilityTrustSummary {
    let mut trust = CapabilityTrustSummary::default();
    for node in graph
        .nodes
        .iter()
        .filter(|node| node_ids.iter().any(|id| id == &node.id))
    {
        trust.local_runtime |= node.trust.local_runtime;
        trust.container_runtime |= node.trust.container_runtime;
        trust.network_capable |= node.trust.network_capable;
        trust.host_action_capable |= node.trust.host_action_capable;
        trust.process_spawn_capable |= node.trust.process_spawn_capable;
        trust.browser_action_capable |= node.trust.browser_action_capable;
        trust.secret_bound |= node.trust.secret_bound;
        trust.state_changing |= node.trust.state_changing;
    }
    trust.read_only = !trust.state_changing;
    trust
}

fn is_missing(node: &CapabilityNode) -> bool {
    matches!(node.health, super::inventory::CapabilityHealth::Missing)
}

#[allow(dead_code)]
fn _edge_refs(edge: &CapabilityEdge) -> (&str, &str) {
    (&edge.from, &edge.to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::agents::{
        AgentExtensionDependency, AgentPersonaSummary, AgentSecretsSummary, AgentSettingsSummary,
    };
    use crate::capabilities::inventory::{
        CapabilityEdgeKind, CapabilityHealth, CapabilityNodeKind,
    };

    #[test]
    fn assistant_profile_merges_reachable_capability_trust() {
        let agent = test_agent(AgentSecretsSummary {
            required: vec!["ANTHROPIC_API_KEY".into()],
            optional: Vec::new(),
        });

        let profiles = resolve_assistant_profiles(
            &[agent],
            &test_graph(),
            &[ready_secret("ANTHROPIC_API_KEY")],
        );

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "daily");
        assert_eq!(profiles[0].model.as_deref(), Some("anthropic:claude"));
        assert!(profiles[0].trust.browser_action_capable);
        assert!(profiles[0].trust.process_spawn_capable);
        assert!(profiles[0].trust.secret_bound);
        assert_eq!(profiles[0].secret_readiness.required_total, 1);
        assert_eq!(profiles[0].secret_readiness.required_ready, 1);
        assert_eq!(
            profiles[0].launch_readiness.status,
            AssistantLaunchStatus::Ready
        );
        assert!(profiles[0].launch_readiness.blockers.is_empty());
        assert!(profiles[0].secret_readiness.missing_required.is_empty());
        assert!(!profiles[0].trust.read_only);
        assert!(
            profiles[0]
                .capability_node_ids
                .contains(&"extension:browser".to_string())
        );
    }

    #[test]
    fn assistant_launch_readiness_blocks_missing_required_secrets() {
        let agent = test_agent(AgentSecretsSummary {
            required: vec!["ANTHROPIC_API_KEY".into()],
            optional: Vec::new(),
        });
        let profiles = resolve_assistant_profiles(&[agent], &test_graph(), &[]);

        assert_eq!(
            profiles[0].launch_readiness.status,
            AssistantLaunchStatus::Blocked
        );
        assert_eq!(profiles[0].launch_readiness.blockers.len(), 1);
        assert_eq!(
            profiles[0].launch_readiness.blockers[0].kind,
            AssistantLaunchBlockerKind::RequiredSecretMissing
        );
        assert_eq!(
            profiles[0].launch_readiness.blockers[0].id,
            "ANTHROPIC_API_KEY"
        );
    }

    #[test]
    fn assistant_launch_readiness_degrades_for_optional_and_deferred_secrets() {
        let agent = test_agent(AgentSecretsSummary {
            required: vec!["ANTHROPIC_API_KEY".into()],
            optional: vec!["OPTIONAL_TOKEN".into(), "VAULT_TOKEN".into()],
        });
        let profiles = resolve_assistant_profiles(
            &[agent],
            &test_graph(),
            &[
                ready_secret("ANTHROPIC_API_KEY"),
                SecretReadiness {
                    name: "VAULT_TOKEN".into(),
                    required: false,
                    optional: true,
                    consumers: Vec::new(),
                    status: SecretReadinessStatus::Deferred,
                    recipe_kind: Some("vault".into()),
                    recipe_source: None,
                    reason: None,
                    process_env_available: false,
                    warmed: false,
                },
            ],
        );

        assert_eq!(
            profiles[0].launch_readiness.status,
            AssistantLaunchStatus::Degraded
        );
        assert!(profiles[0].launch_readiness.blockers.is_empty());
        assert!(profiles[0].launch_readiness.warnings.iter().any(|warning| {
            warning.kind == AssistantLaunchWarningKind::OptionalSecretMissing
                && warning.id == "OPTIONAL_TOKEN"
        }));
        assert!(profiles[0].launch_readiness.warnings.iter().any(|warning| {
            warning.kind == AssistantLaunchWarningKind::SecretDeferred
                && warning.id == "VAULT_TOKEN"
        }));
    }

    #[test]
    fn assistant_list_items_sort_by_launch_readiness_and_counts() {
        let ready = AssistantProfileSummary {
            id: "ready".into(),
            name: "Ready".into(),
            version: "0.1.0".into(),
            description: "Ready assistant".into(),
            domain: "ops".into(),
            source_path: "/catalog/ready".into(),
            model: Some("anthropic:claude".into()),
            thinking_level: None,
            context_class: None,
            max_turns: None,
            activated_skills: Vec::new(),
            disabled_tools: Vec::new(),
            extensions: Vec::new(),
            required_secrets: vec!["API_KEY".into()],
            optional_secrets: Vec::new(),
            triggers: Vec::new(),
            trust: CapabilityTrustSummary::default(),
            launch_readiness: AssistantLaunchReadiness {
                status: AssistantLaunchStatus::Ready,
                blockers: Vec::new(),
                warnings: Vec::new(),
            },
            secret_readiness: AssistantSecretReadinessSummary::default(),
            capability_node_ids: Vec::new(),
            missing_required_node_ids: Vec::new(),
        };
        let mut degraded = ready.clone();
        degraded.id = "degraded".into();
        degraded.name = "Degraded".into();
        degraded.launch_readiness = AssistantLaunchReadiness {
            status: AssistantLaunchStatus::Degraded,
            blockers: Vec::new(),
            warnings: vec![AssistantLaunchWarning {
                kind: AssistantLaunchWarningKind::OptionalSecretMissing,
                id: "OPTIONAL".into(),
            }],
        };
        degraded.optional_secrets = vec!["OPTIONAL".into()];
        let mut blocked = ready.clone();
        blocked.id = "blocked".into();
        blocked.name = "Blocked".into();
        blocked.launch_readiness = AssistantLaunchReadiness {
            status: AssistantLaunchStatus::Blocked,
            blockers: vec![AssistantLaunchBlocker {
                kind: AssistantLaunchBlockerKind::RequiredSecretMissing,
                id: "API_KEY".into(),
            }],
            warnings: Vec::new(),
        };

        let items = assistant_list_items(&[blocked, degraded, ready]);

        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ready", "degraded", "blocked"]
        );
        assert_eq!(items[0].required_secret_count, 1);
        assert_eq!(items[1].warning_count, 1);
        assert_eq!(items[2].blocker_count, 1);
        assert_eq!(
            items[2].launch_readiness.status,
            AssistantLaunchStatus::Blocked
        );
    }

    fn test_agent(secrets: AgentSecretsSummary) -> AgentBundleSummary {
        AgentBundleSummary {
            id: "daily".into(),
            name: "Daily".into(),
            version: "0.1.0".into(),
            description: "Daily operator".into(),
            domain: "ops".into(),
            source_path: "/catalog/daily".into(),
            persona: AgentPersonaSummary {
                activated_skills: vec!["rust".into()],
                ..Default::default()
            },
            extensions: vec![AgentExtensionDependency {
                name: "browser".into(),
                version: "*".into(),
            }],
            settings: AgentSettingsSummary {
                model: Some("anthropic:claude".into()),
                ..Default::default()
            },
            workflow: None,
            secrets,
            triggers: Vec::new(),
        }
    }

    fn test_graph() -> CapabilityGraph {
        CapabilityGraph {
            nodes: vec![
                CapabilityNode {
                    id: "agent:daily".into(),
                    kind: CapabilityNodeKind::AgentBundle,
                    label: "Daily".into(),
                    source_path: None,
                    health: CapabilityHealth::Available,
                    trust: CapabilityTrustSummary {
                        secret_bound: true,
                        state_changing: true,
                        ..Default::default()
                    },
                },
                CapabilityNode {
                    id: "extension:browser".into(),
                    kind: CapabilityNodeKind::Extension,
                    label: "Browser".into(),
                    source_path: None,
                    health: CapabilityHealth::Available,
                    trust: CapabilityTrustSummary {
                        browser_action_capable: true,
                        process_spawn_capable: true,
                        state_changing: true,
                        ..Default::default()
                    },
                },
            ],
            edges: vec![CapabilityEdge {
                from: "agent:daily".into(),
                to: "extension:browser".into(),
                kind: CapabilityEdgeKind::UsesExtension,
            }],
        }
    }

    fn ready_secret(name: &str) -> SecretReadiness {
        SecretReadiness {
            name: name.into(),
            required: true,
            optional: false,
            consumers: Vec::new(),
            status: SecretReadinessStatus::Warmed,
            recipe_kind: None,
            recipe_source: None,
            reason: None,
            process_env_available: true,
            warmed: true,
        }
    }
}

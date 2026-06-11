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
    pub secret_readiness: AssistantSecretReadinessSummary,
    pub capability_node_ids: Vec<String>,
    pub missing_required_node_ids: Vec<String>,
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

fn resolve_assistant_profile(
    agent: &AgentBundleSummary,
    graph: &CapabilityGraph,
    secret_readiness: &[SecretReadiness],
) -> AssistantProfileSummary {
    let root_id = format!("agent:{}", agent.id);
    let capability_node_ids = reachable_node_ids(&root_id, graph);
    let missing_required_node_ids = capability_node_ids
        .iter()
        .filter(|id| {
            graph
                .nodes
                .iter()
                .any(|node| node.id == **id && is_missing(node))
        })
        .cloned()
        .collect();

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
        secret_readiness: summarize_agent_secret_readiness(agent, secret_readiness),
        capability_node_ids,
        missing_required_node_ids,
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
        let agent = AgentBundleSummary {
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
            secrets: AgentSecretsSummary {
                required: vec!["ANTHROPIC_API_KEY".into()],
                optional: Vec::new(),
            },
            triggers: Vec::new(),
        };
        let graph = CapabilityGraph {
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
        };

        let profiles = resolve_assistant_profiles(
            &[agent],
            &graph,
            &[SecretReadiness {
                name: "ANTHROPIC_API_KEY".into(),
                required: true,
                optional: false,
                consumers: Vec::new(),
                status: SecretReadinessStatus::Warmed,
                recipe_kind: None,
                warmed: true,
            }],
        );

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "daily");
        assert_eq!(profiles[0].model.as_deref(), Some("anthropic:claude"));
        assert!(profiles[0].trust.browser_action_capable);
        assert!(profiles[0].trust.process_spawn_capable);
        assert!(profiles[0].trust.secret_bound);
        assert_eq!(profiles[0].secret_readiness.required_total, 1);
        assert_eq!(profiles[0].secret_readiness.required_ready, 1);
        assert!(profiles[0].secret_readiness.missing_required.is_empty());
        assert!(!profiles[0].trust.read_only);
        assert!(
            profiles[0]
                .capability_node_ids
                .contains(&"extension:browser".to_string())
        );
    }
}

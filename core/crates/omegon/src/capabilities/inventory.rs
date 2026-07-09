use std::path::Path;

use serde::{Deserialize, Serialize};

use super::agents::{AgentBundleSummary, list_agent_bundle_summaries_from_dir};
use super::armory::{ArmoryProfileSummary, list_armory_profiles_from_root};
use super::extensions::{
    ExtensionCapabilitySummary, list_installed_extension_capabilities_from_dir,
};
use super::profiles::{AssistantProfileSummary, assistant_list_items, resolve_assistant_profiles};
use super::secrets::{
    SecretReadinessInputs, SecretReadinessSnapshot, build_secret_readiness_snapshot,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityInventorySnapshot {
    pub installed_extensions: Vec<ExtensionCapabilitySummary>,
    pub armory_profiles: Vec<ArmoryProfileSummary>,
    pub agent_bundles: Vec<AgentBundleSummary>,
    pub assistant_profiles: Vec<AssistantProfileSummary>,
    pub assistant_list: Vec<super::profiles::AssistantListItem>,
    pub secret_readiness: SecretReadinessSnapshot,
    pub graph: CapabilityGraph,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityGraph {
    pub nodes: Vec<CapabilityNode>,
    pub edges: Vec<CapabilityEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityNode {
    pub id: String,
    pub kind: CapabilityNodeKind,
    pub label: String,
    pub source_path: Option<String>,
    pub health: CapabilityHealth,
    pub trust: CapabilityTrustSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityNodeKind {
    Extension,
    ArmoryProfile,
    AgentBundle,
    Skill,
    Secret,
    Widget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityHealth {
    Available,
    Disabled,
    Degraded,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityTrustSummary {
    pub local_runtime: bool,
    pub container_runtime: bool,
    pub network_capable: bool,
    pub host_action_capable: bool,
    pub process_spawn_capable: bool,
    pub browser_action_capable: bool,
    pub secret_bound: bool,
    pub state_changing: bool,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityEdge {
    pub from: String,
    pub to: String,
    pub kind: CapabilityEdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityEdgeKind {
    DependsOn,
    RequiresSecret,
    ProvidesWidget,
    ActivatesSkill,
    UsesExtension,
}

#[derive(Debug, Clone, Copy)]
pub struct CapabilityInventoryRoots<'a> {
    pub extensions_dir: &'a Path,
    pub armory_root: &'a Path,
    pub catalog_dir: &'a Path,
}

pub fn build_capability_inventory_snapshot(
    roots: CapabilityInventoryRoots<'_>,
) -> anyhow::Result<CapabilityInventorySnapshot> {
    build_capability_inventory_snapshot_with_secrets(roots, SecretReadinessInputs::default())
}

pub fn build_capability_inventory_snapshot_with_secrets(
    roots: CapabilityInventoryRoots<'_>,
    secret_inputs: SecretReadinessInputs,
) -> anyhow::Result<CapabilityInventorySnapshot> {
    let installed_extensions =
        list_installed_extension_capabilities_from_dir(roots.extensions_dir)?;
    let armory_profiles = list_armory_profiles_from_root(roots.armory_root)?;
    let agent_bundles = list_agent_bundle_summaries_from_dir(roots.catalog_dir)?;
    let graph = build_capability_graph(&installed_extensions, &armory_profiles, &agent_bundles);
    let secret_readiness_snapshot =
        build_secret_readiness_snapshot(&installed_extensions, &agent_bundles, secret_inputs);
    let assistant_profiles =
        resolve_assistant_profiles(&agent_bundles, &graph, &secret_readiness_snapshot.secrets);
    let assistant_list = assistant_list_items(&assistant_profiles);

    Ok(CapabilityInventorySnapshot {
        installed_extensions,
        armory_profiles,
        agent_bundles,
        assistant_profiles,
        assistant_list,
        secret_readiness: secret_readiness_snapshot,
        graph,
    })
}

pub fn build_capability_graph(
    extensions: &[ExtensionCapabilitySummary],
    profiles: &[ArmoryProfileSummary],
    agents: &[AgentBundleSummary],
) -> CapabilityGraph {
    let mut graph = CapabilityGraph::default();

    for extension in extensions {
        let extension_id = format!("extension:{}", extension.name);
        graph.nodes.push(CapabilityNode {
            id: extension_id.clone(),
            kind: CapabilityNodeKind::Extension,
            label: extension.name.clone(),
            source_path: Some(extension.source_path.clone()),
            health: extension_health(extension),
            trust: extension_trust(extension),
        });

        for secret in extension
            .required_secrets
            .iter()
            .chain(extension.optional_secrets.iter())
        {
            push_secret_node_and_edge(&mut graph, &extension_id, secret);
        }
        for widget in &extension.widgets {
            let widget_id = format!("widget:{}:{}", extension.name, widget.id);
            graph.nodes.push(CapabilityNode {
                id: widget_id.clone(),
                kind: CapabilityNodeKind::Widget,
                label: widget.label.clone(),
                source_path: Some(extension.source_path.clone()),
                health: CapabilityHealth::Available,
                trust: CapabilityTrustSummary::default(),
            });
            graph.edges.push(CapabilityEdge {
                from: extension_id.clone(),
                to: widget_id,
                kind: CapabilityEdgeKind::ProvidesWidget,
            });
        }
    }

    for profile in profiles {
        let profile_id = format!("profile:{}", profile.slug);
        graph.nodes.push(CapabilityNode {
            id: profile_id.clone(),
            kind: CapabilityNodeKind::ArmoryProfile,
            label: profile.name.clone(),
            source_path: Some(profile.source_path.clone()),
            health: CapabilityHealth::Available,
            trust: CapabilityTrustSummary::default(),
        });
        for dependency in &profile.dependencies {
            let target = dependency_node_id(&dependency.kind, &dependency.id);
            graph.edges.push(CapabilityEdge {
                from: profile_id.clone(),
                to: target,
                kind: if dependency.kind == "skill" {
                    CapabilityEdgeKind::ActivatesSkill
                } else {
                    CapabilityEdgeKind::DependsOn
                },
            });
        }
    }

    for agent in agents {
        let agent_id = format!("agent:{}", agent.id);
        graph.nodes.push(CapabilityNode {
            id: agent_id.clone(),
            kind: CapabilityNodeKind::AgentBundle,
            label: agent.name.clone(),
            source_path: Some(agent.source_path.clone()),
            health: CapabilityHealth::Available,
            trust: agent_trust(agent),
        });
        for extension in &agent.extensions {
            graph.edges.push(CapabilityEdge {
                from: agent_id.clone(),
                to: format!("extension:{}", extension.name),
                kind: CapabilityEdgeKind::UsesExtension,
            });
        }
        for skill in &agent.persona.activated_skills {
            graph.nodes.push(CapabilityNode {
                id: format!("skill:{skill}"),
                kind: CapabilityNodeKind::Skill,
                label: skill.clone(),
                source_path: None,
                health: CapabilityHealth::Unknown,
                trust: CapabilityTrustSummary::default(),
            });
            graph.edges.push(CapabilityEdge {
                from: agent_id.clone(),
                to: format!("skill:{skill}"),
                kind: CapabilityEdgeKind::ActivatesSkill,
            });
        }
        for secret in agent
            .secrets
            .required
            .iter()
            .chain(agent.secrets.optional.iter())
        {
            push_secret_node_and_edge(&mut graph, &agent_id, secret);
        }
    }

    dedupe_graph(&mut graph);
    graph.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    graph.edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
    });
    graph
}

fn extension_health(extension: &ExtensionCapabilitySummary) -> CapabilityHealth {
    if !extension.enabled {
        CapabilityHealth::Disabled
    } else if extension.stability.auto_disabled
        || extension.stability.last_error.is_some()
        || extension.stability.health_check_failures > 0
        || extension.stability.crashes_this_session > 0
    {
        CapabilityHealth::Degraded
    } else {
        CapabilityHealth::Available
    }
}

fn extension_trust(extension: &ExtensionCapabilitySummary) -> CapabilityTrustSummary {
    let raw_capabilities = extension.capabilities.to_string().to_ascii_lowercase();
    let raw_permissions = extension.permissions.to_string().to_ascii_lowercase();
    let network_capable = extension.mcp.is_some()
        || raw_capabilities.contains("network")
        || raw_permissions.contains("network")
        || raw_permissions.contains("http");
    let host_action_capable = raw_capabilities.contains("host_action")
        || raw_capabilities.contains("host-actions")
        || raw_permissions.contains("host_action")
        || raw_permissions.contains("host-actions");
    let process_spawn_capable = raw_permissions.contains("process")
        || raw_permissions.contains("command")
        || raw_permissions.contains("spawn");
    let browser_action_capable = extension.name.to_ascii_lowercase().contains("browser")
        || raw_capabilities.contains("browser")
        || raw_permissions.contains("browser");
    let state_changing = host_action_capable
        || process_spawn_capable
        || browser_action_capable
        || extension
            .capabilities
            .get("tools")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

    CapabilityTrustSummary {
        local_runtime: matches!(
            extension.runtime,
            super::extensions::ExtensionRuntimeSummary::Native { .. }
        ),
        container_runtime: matches!(
            extension.runtime,
            super::extensions::ExtensionRuntimeSummary::Oci { .. }
        ),
        network_capable,
        host_action_capable,
        process_spawn_capable,
        browser_action_capable,
        secret_bound: !extension.required_secrets.is_empty()
            || !extension.optional_secrets.is_empty(),
        state_changing,
        read_only: !state_changing,
    }
}

fn agent_trust(agent: &AgentBundleSummary) -> CapabilityTrustSummary {
    CapabilityTrustSummary {
        secret_bound: !agent.secrets.required.is_empty() || !agent.secrets.optional.is_empty(),
        state_changing: !agent.extensions.is_empty() || !agent.triggers.is_empty(),
        read_only: agent.extensions.is_empty() && agent.triggers.is_empty(),
        ..CapabilityTrustSummary::default()
    }
}

fn push_secret_node_and_edge(graph: &mut CapabilityGraph, from: &str, secret: &str) {
    let secret_id = format!("secret:{secret}");
    graph.nodes.push(CapabilityNode {
        id: secret_id.clone(),
        kind: CapabilityNodeKind::Secret,
        label: secret.to_string(),
        source_path: None,
        health: CapabilityHealth::Unknown,
        trust: CapabilityTrustSummary::default(),
    });
    graph.edges.push(CapabilityEdge {
        from: from.to_string(),
        to: secret_id,
        kind: CapabilityEdgeKind::RequiresSecret,
    });
}

fn dependency_node_id(kind: &str, id: &str) -> String {
    match kind {
        "extension" => format!("extension:{id}"),
        "skill" => format!("skill:{id}"),
        "agent" => format!("agent:{id}"),
        "profile" => format!("profile:{id}"),
        _ => format!("{kind}:{id}"),
    }
}

fn dedupe_graph(graph: &mut CapabilityGraph) {
    let mut seen_nodes = std::collections::BTreeSet::new();
    graph
        .nodes
        .retain(|node| seen_nodes.insert(node.id.clone()));

    let mut seen_edges = std::collections::BTreeSet::new();
    graph.edges.retain(|edge| {
        seen_edges.insert((
            edge.from.clone(),
            edge.to.clone(),
            format!("{:?}", edge.kind),
        ))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::agents::{
        AgentExtensionDependency, AgentPersonaSummary, AgentSecretsSummary, AgentSettingsSummary,
    };
    use crate::capabilities::extensions::{
        ExtensionRuntimeSummary, ExtensionStabilitySummary, ExtensionStartupSummary,
    };

    #[test]
    fn missing_roots_build_empty_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = build_capability_inventory_snapshot(CapabilityInventoryRoots {
            extensions_dir: &temp.path().join("extensions"),
            armory_root: &temp.path().join("armory"),
            catalog_dir: &temp.path().join("catalog"),
        })
        .unwrap();

        assert!(snapshot.installed_extensions.is_empty());
        assert!(snapshot.armory_profiles.is_empty());
        assert!(snapshot.agent_bundles.is_empty());
        assert!(snapshot.assistant_profiles.is_empty());
        assert!(
            snapshot
                .secret_readiness
                .secrets
                .iter()
                .any(|secret| secret.name == "ANTHROPIC_API_KEY")
        );
        assert!(
            snapshot
                .secret_readiness
                .harness_capabilities
                .iter()
                .any(|capability| capability.id == "llm_provider_api_keys")
        );
        assert!(snapshot.graph.nodes.is_empty());
        assert!(snapshot.graph.edges.is_empty());
    }

    #[test]
    fn graph_connects_agents_extensions_skills_and_secrets() {
        let extensions = vec![ExtensionCapabilitySummary {
            name: "browser".into(),
            version: "0.1.0".into(),
            description: "Browser automation".into(),
            runtime: ExtensionRuntimeSummary::Native {
                binary: "browser".into(),
            },
            status: "enabled".into(),
            enabled: true,
            source_path: "/ext/browser".into(),
            startup: ExtensionStartupSummary {
                ping_method: Some("get_tools".into()),
                timeout_ms: 1000,
            },
            config: Vec::new(),
            required_secrets: vec!["BROWSER_TOKEN".into()],
            optional_secrets: Vec::new(),
            widgets: Vec::new(),
            capabilities: serde_json::json!({"tools": true, "browser": true}),
            permissions: serde_json::json!({"process": {"allowed_commands": ["open"]}}),
            mcp: None,
            stability: ExtensionStabilitySummary {
                crashes_this_session: 0,
                health_check_failures: 0,
                last_error: None,
                last_error_at: None,
                auto_disabled: false,
            },
        }];
        let agents = vec![AgentBundleSummary {
            id: "daily".into(),
            name: "Daily agent".into(),
            version: "0.1.0".into(),
            description: String::new(),
            domain: "ops".into(),
            source_path: "/catalog/daily".into(),
            persona: AgentPersonaSummary {
                activated_skills: vec!["rust".into()],
                ..Default::default()
            },
            extensions: vec![AgentExtensionDependency {
                name: "browser".into(),
                version: "0.1.0".into(),
            }],
            settings: AgentSettingsSummary::default(),
            workflow: None,
            secrets: AgentSecretsSummary {
                required: vec!["ANTHROPIC_API_KEY".into()],
                optional: Vec::new(),
            },
            triggers: Vec::new(),
        }];

        let graph = build_capability_graph(&extensions, &[], &agents);

        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "extension:browser")
        );
        assert!(graph.nodes.iter().any(|node| node.id == "agent:daily"));
        assert!(graph.nodes.iter().any(|node| node.id == "skill:rust"));
        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "secret:BROWSER_TOKEN")
        );
        assert!(graph.edges.iter().any(|edge| {
            edge.from == "agent:daily"
                && edge.to == "extension:browser"
                && edge.kind == CapabilityEdgeKind::UsesExtension
        }));
        let browser = graph
            .nodes
            .iter()
            .find(|node| node.id == "extension:browser")
            .unwrap();
        assert!(browser.trust.browser_action_capable);
        assert!(browser.trust.process_spawn_capable);
        assert!(browser.trust.state_changing);
    }
}

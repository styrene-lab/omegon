//! Lifecycle Feature — design-tree + openspec as a unified Feature.
//!
//! Provides:
//! - Tools: `design_tree` (query), `design_tree_update` (mutation),
//!   `openspec_manage` (lifecycle management)
//! - Commands: `/focus`, `/design`, `/unfocus`
//! - Context injection: focused design node + active openspec changes
//! - Event handling: refresh on TurnEnd

use std::path::PathBuf;
#[cfg(test)]
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[cfg(test)]
use omegon_traits::ContextProvider;
use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, ContentBlock, ContextInjection,
    ContextSignals, Feature, RuntimeActivationBoundary, RuntimeCleanupRequirement,
    RuntimeCompositionTransitionPolicy, RuntimeContributionGenerationId, RuntimeFailureDisposition,
    RuntimeLifecyclePolicy, RuntimeLifecycleRequirement, ToolDefinition, ToolResult,
};

#[cfg(test)]
use crate::lifecycle::context::LifecycleContextProvider;
#[cfg(test)]
use crate::lifecycle::mutation::{
    AddDesignNodeDecisionRequest, AddDesignNodeImplNotesRequest, AddDesignNodeLinkRequest,
    AddDesignNodeResearchRequest, BranchDesignNodeQuestionRequest, CreateDesignNodeRequest,
    ImplementDesignNodeRequest, LifecycleMutationService, SetDesignNodeIssueTypeRequest,
    SetDesignNodePriorityRequest, SetDesignNodeStatusRequest, UpdateDesignNodeQuestionRequest,
};
#[cfg(test)]
use crate::lifecycle::{archive, doctor, spec, sync};
use crate::lifecycle::{design, query, types::*};

use omegon_opsx::NodeState as OpsxNodeState;
#[cfg(test)]
use omegon_opsx::{ChangeState as OpsxChangeState, JsonFileStore, Lifecycle as OpsxLifecycle};

/// The lifecycle Feature — wraps the LifecycleContextProvider and adds
/// tools + commands for design-tree and openspec operations.
///
/// The provider is behind RefCell because `Feature::execute()` takes `&self`
/// but mutations need `&mut`. The bus guarantees sequential delivery so
/// this is safe in practice.
pub struct LifecycleFeature {
    #[cfg(test)]
    provider: Option<Arc<Mutex<LifecycleContextProvider>>>,
    repo_path: PathBuf,
    /// Counter for refresh throttling — only refresh every N turns.
    turn_counter: u32,
    /// omegon-opsx lifecycle engine — validates state transitions before
    /// markdown is written. The FSM is the authority for what transitions
    /// are legal; markdown is the content store.
    #[cfg(test)]
    opsx: Option<Arc<Mutex<OpsxLifecycle<JsonFileStore>>>>,
    /// Memory facts queued from execute() to be returned from on_event(TurnEnd).
    /// execute() takes &self so can't return BusRequests directly — this bridges the gap.
    pending_memory: Mutex<Vec<BusRequest>>,
    /// Lifecycle-domain mutation service. Tool adapters keep JSON parsing and
    /// response rendering; the service owns store coordination.
    #[cfg(test)]
    mutation_service: Option<LifecycleMutationService>,
    lifecycle_binding: Option<crate::lifecycle_service::LifecycleBinding>,
    lifecycle_host: Option<crate::runtime_state::LifecycleHostHandle>,
    /// Optional Codex vault path — exports design tree on session end.
    codex_vault_path: Option<PathBuf>,
}

impl LifecycleFeature {
    #[cfg(test)]
    fn opsx_change_states(&self) -> std::collections::HashMap<String, String> {
        self.compatibility_opsx()
            .lock()
            .unwrap()
            .state()
            .changes
            .iter()
            .map(|c| (c.name.clone(), c.state.as_str().to_string()))
            .collect()
    }

    fn has_non_archived_descendants(
        nodes: &std::collections::HashMap<String, DesignNode>,
        node_id: &str,
    ) -> bool {
        for child in design::get_children(nodes, node_id) {
            if !query::is_archived(child) || Self::has_non_archived_descendants(nodes, &child.id) {
                return true;
            }
        }
        false
    }

    fn archive_timestamp() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        secs.to_string()
    }

    #[cfg(test)]
    pub fn try_new(repo_path: &std::path::Path) -> anyhow::Result<Self> {
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(repo_path)));
        let store = JsonFileStore::new(repo_path);
        let opsx = OpsxLifecycle::load(store)
            .map_err(|error| anyhow::anyhow!("lifecycle state unavailable: {error}"))?;
        let opsx = Arc::new(Mutex::new(opsx));
        let repo_path = repo_path.to_path_buf();
        let mutation_service = LifecycleMutationService::new(
            repo_path.clone(),
            Arc::clone(&provider),
            Arc::clone(&opsx),
        );
        Ok(Self {
            provider: Some(provider),
            repo_path,
            turn_counter: 0,
            opsx: Some(opsx),
            pending_memory: Mutex::new(vec![]),
            mutation_service: Some(mutation_service),
            lifecycle_binding: None,
            lifecycle_host: None,
            codex_vault_path: None,
        })
    }

    pub(crate) fn managed(
        repo_path: &std::path::Path,
        lifecycle_binding: crate::lifecycle_service::LifecycleBinding,
        lifecycle_host: crate::runtime_state::LifecycleHostHandle,
    ) -> Self {
        Self {
            #[cfg(test)]
            provider: None,
            repo_path: repo_path.to_path_buf(),
            turn_counter: 0,
            #[cfg(test)]
            opsx: None,
            pending_memory: Mutex::new(vec![]),
            #[cfg(test)]
            mutation_service: None,
            lifecycle_binding: Some(lifecycle_binding),
            lifecycle_host: Some(lifecycle_host),
            codex_vault_path: None,
        }
    }

    #[cfg(test)]
    fn compatibility_provider(&self) -> &Arc<Mutex<LifecycleContextProvider>> {
        self.provider
            .as_ref()
            .expect("compatibility lifecycle provider is unavailable in managed mode")
    }

    #[cfg(test)]
    fn compatibility_opsx(&self) -> &Arc<Mutex<OpsxLifecycle<JsonFileStore>>> {
        self.opsx
            .as_ref()
            .expect("compatibility lifecycle ledger is unavailable in managed mode")
    }

    #[cfg(test)]
    fn compatibility_mutation_service(&self) -> &LifecycleMutationService {
        self.mutation_service
            .as_ref()
            .expect("compatibility lifecycle mutation service is unavailable in managed mode")
    }

    #[cfg(test)]
    fn new(repo_path: &std::path::Path) -> Self {
        Self::try_new(repo_path).expect("lifecycle test fixture should load")
    }

    /// Set the Codex vault path for automatic design tree export on session end.
    pub fn with_codex_vault(mut self, path: PathBuf) -> Self {
        self.codex_vault_path = Some(path);
        self
    }

    pub(crate) fn with_lifecycle_host(
        mut self,
        lifecycle_host: crate::runtime_state::LifecycleHostHandle,
    ) -> Self {
        self.lifecycle_host = Some(lifecycle_host);
        self
    }

    fn focused_node_id(&self) -> Option<String> {
        let managed = self
            .lifecycle_host
            .as_ref()
            .and_then(|host| host.observe().ok()?.focus.node_id);
        if managed.is_some() {
            return managed;
        }
        #[cfg(test)]
        {
            return self
                .compatibility_provider()
                .lock()
                .ok()?
                .focused_node_id()
                .map(str::to_string);
        }
        #[cfg(not(test))]
        None
    }

    fn set_focus(&self, node_id: Option<String>) -> anyhow::Result<()> {
        if let Some(host) = &self.lifecycle_host {
            host.set_focus(node_id.clone())?;
        }
        #[cfg(test)]
        if let Some(provider) = &self.provider {
            provider
                .lock()
                .map_err(|_| anyhow::anyhow!("lifecycle provider lock poisoned"))?
                .set_focus(node_id);
        }
        Ok(())
    }

    fn observed_design_nodes(&self) -> std::collections::HashMap<String, DesignNode> {
        let managed = self
            .lifecycle_host
            .as_ref()
            .and_then(|host| host.observe().ok()?.repository)
            .map(|repository| repository.design.nodes.clone());
        if let Some(nodes) = managed {
            return nodes;
        }
        #[cfg(test)]
        {
            self.provider
                .as_ref()
                .and_then(|provider| {
                    provider
                        .lock()
                        .ok()
                        .map(|provider| provider.all_nodes().clone())
                })
                .unwrap_or_default()
        }
        #[cfg(not(test))]
        std::collections::HashMap::new()
    }

    fn managed_context(&self) -> Option<ContextInjection> {
        let observation = self.lifecycle_host.as_ref()?.observe().ok()?;
        let repository = observation.repository?;
        let mut parts = Vec::new();

        if let Some(node_id) = observation.focus.node_id.as_deref()
            && let Some(node) = repository.design.nodes.get(node_id)
            && let Some(sections) = repository.sections.get(node_id)
        {
            let injection = design::build_context_injection(node, sections);
            if !injection.is_empty() {
                parts.push(injection);
            }
        }

        let active = repository
            .lifecycle
            .openspec
            .changes
            .iter()
            .filter(|change| {
                matches!(
                    change.lifecycle_state.as_str(),
                    "implementing" | "verifying"
                )
            })
            .collect::<Vec<_>>();
        if !active.is_empty() {
            let mut lines = vec!["[OpenSpec — active changes]".to_string()];
            for change in active {
                let icon = if change.lifecycle_state == "implementing" {
                    "⟳"
                } else {
                    "◉"
                };
                let progress = if change.total_tasks > 0 {
                    format!(" ({}/{})", change.done_tasks, change.total_tasks)
                } else {
                    String::new()
                };
                lines.push(format!(
                    "  {icon} {} — {}{progress}",
                    change.name, change.lifecycle_state
                ));
                for spec in &change.spec_documents {
                    let scenario_count = spec
                        .requirements
                        .iter()
                        .map(|requirement| requirement.scenarios.len())
                        .sum::<usize>();
                    if scenario_count > 0 {
                        lines.push(format!(
                            "    specs/{}: {scenario_count} scenarios",
                            spec.domain
                        ));
                    }
                }
            }
            parts.push(lines.join("\n"));
        }

        (!parts.is_empty()).then(|| ContextInjection {
            source: "lifecycle".into(),
            content: parts.join("\n\n"),
            priority: 150,
            ttl_turns: 3,
        })
    }

    fn managed_binding(&self) -> anyhow::Result<&crate::lifecycle_service::LifecycleBinding> {
        self.lifecycle_binding
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("managed lifecycle service is unavailable"))
    }

    fn operation_id(tool_name: &str, call_id: &str) -> String {
        let digest = Sha256::digest(format!("{tool_name}\0{call_id}").as_bytes());
        let digest = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("tool-{digest}")
    }

    async fn refresh_managed(
        &self,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<crate::runtime_state::LifecycleHostObservation> {
        self.lifecycle_host
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("managed lifecycle host is unavailable"))?
            .refresh(
                crate::lifecycle::read_model::SnapshotOptions::default(),
                cancellation,
            )
            .await
    }

    async fn managed_design_tree(
        &self,
        args: &Value,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");
        let node_id = args["node_id"].as_str();
        let observation = self.refresh_managed(cancellation).await?;
        let repository = observation
            .repository
            .ok_or_else(|| anyhow::anyhow!("managed lifecycle service is unavailable"))?;
        let nodes = &repository.design.nodes;
        let result = match action {
            "list" => nodes
                .values()
                .filter(|node| !query::is_archived(node))
                .map(|node| {
                    json!({
                        "id": node.id,
                        "title": node.title,
                        "status": node.status.as_str(),
                        "parent": node.parent,
                        "tags": node.tags,
                        "open_questions": node.open_questions.len(),
                        "dependencies": node.dependencies,
                        "branches": node.branches,
                        "openspec_change": node.openspec_change,
                        "priority": node.priority,
                        "issue_type": node.issue_type.map(|kind| match kind {
                            IssueType::Epic => "epic",
                            IssueType::Feature => "feature",
                            IssueType::Task => "task",
                            IssueType::Bug => "bug",
                            IssueType::Chore => "chore",
                        }),
                        "children": design::get_children(nodes, &node.id).len(),
                    })
                })
                .collect::<Vec<_>>(),
            "node" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let node = nodes
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("Node '{id}' not found"))?;
                let mut value = json!({
                    "id": node.id,
                    "title": node.title,
                    "status": node.status.as_str(),
                    "parent": node.parent,
                    "tags": node.tags,
                    "open_questions": node.open_questions,
                    "dependencies": node.dependencies,
                    "related": node.related,
                    "branches": node.branches,
                    "openspec_change": node.openspec_change,
                    "priority": node.priority,
                    "archive_reason": node.archive_reason,
                    "superseded_by": node.superseded_by,
                    "archived_at": node.archived_at,
                    "children": design::get_children(nodes, id).into_iter().map(|child| json!({
                        "id": child.id,
                        "title": child.title,
                        "status": child.status.as_str(),
                    })).collect::<Vec<_>>(),
                });
                if let Some(sections) = repository.sections.get(id) {
                    value["overview"] = json!(sections.overview);
                    value["research"] = json!(
                        sections
                            .research
                            .iter()
                            .map(|item| json!({
                                "heading": item.heading,
                                "content": item.content,
                            }))
                            .collect::<Vec<_>>()
                    );
                    value["decisions"] = json!(
                        sections
                            .decisions
                            .iter()
                            .map(|item| json!({
                                "title": item.title,
                                "status": item.status,
                                "rationale": item.rationale,
                            }))
                            .collect::<Vec<_>>()
                    );
                    value["impl_file_scope"] = json!(
                        sections
                            .impl_file_scope
                            .iter()
                            .map(|item| json!({
                                "path": item.path,
                                "description": item.description,
                                "action": item.action,
                            }))
                            .collect::<Vec<_>>()
                    );
                    value["impl_constraints"] = json!(sections.impl_constraints);
                    value["readiness"] = json!({
                        "score": sections.readiness_score(),
                        "decisions": sections.decisions.iter().filter(|item| item.status == "decided").count(),
                        "questions": sections.question_count(),
                        "assumptions": sections.assumption_count(),
                    });
                }
                return Ok(text_result(&serde_json::to_string_pretty(&value)?));
            }
            "frontier" => query::frontier(nodes)
                .into_iter()
                .map(|node| {
                    json!({
                        "id": node.id,
                        "title": node.title,
                        "status": node.status,
                        "open_questions": node.open_questions,
                    })
                })
                .collect(),
            "children" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                query::children(nodes, id)
                    .into_iter()
                    .map(
                        |node| json!({ "id": node.id, "title": node.title, "status": node.status }),
                    )
                    .collect()
            }
            "dependencies" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let node = nodes
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("Node '{id}' not found"))?;
                query::dependencies(nodes, node)
                    .into_iter()
                    .map(
                        |node| json!({ "id": node.id, "title": node.title, "status": node.status }),
                    )
                    .collect()
            }
            "ready" => query::ready(nodes)
                .into_iter()
                .map(
                    |node| json!({ "id": node.id, "title": node.title, "priority": node.priority }),
                )
                .collect(),
            "blocked" => query::blocked(nodes)
                .into_iter()
                .map(|node| {
                    json!({
                        "id": node.id,
                        "title": node.title,
                        "status": node.status,
                        "blocked_by": node.blocked_by,
                    })
                })
                .collect(),
            _ => anyhow::bail!(
                "Unknown action: {action}. Valid: list, node, frontier, children, dependencies, ready, blocked"
            ),
        };
        Ok(text_result(&serde_json::to_string_pretty(&result)?))
    }

    async fn invoke_design_mutation(
        &self,
        call_id: &str,
        expected_revision: crate::lifecycle_service::LifecycleRepositoryRevisionV1,
        mutation: crate::lifecycle_service::DesignMutationV1,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<crate::lifecycle_service::LifecycleMutationReceiptV1> {
        let response = self
            .managed_binding()?
            .invoke(crate::lifecycle_service::LifecycleRequestV1::MutateDesign {
                operation_id: Self::operation_id(
                    crate::tool_registry::lifecycle::DESIGN_TREE_UPDATE,
                    call_id,
                ),
                expected_revision,
                mutation: Box::new(mutation),
                cancellation: cancellation.clone(),
            })
            .await
            .map_err(|error| anyhow::anyhow!("managed lifecycle mutation failed: {error:?}"))?;
        let crate::lifecycle_service::LifecyclePayloadV1::DesignMutation(receipt) =
            response.payload
        else {
            anyhow::bail!("managed lifecycle returned an unexpected design mutation response");
        };
        if let Err(error) = self.refresh_managed(cancellation).await {
            tracing::warn!(%error, "managed lifecycle cache refresh failed after design mutation");
        }
        Ok(receipt)
    }

    async fn managed_design_tree_update(
        &self,
        call_id: &str,
        args: &Value,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        use crate::lifecycle_service::{
            DesignFileScopeV1, DesignIssueTypeV1, DesignMutationV1, LifecycleMutationOutcomeV1,
        };

        let action = args["action"].as_str().unwrap_or("");
        let node_id = args["node_id"].as_str();
        if action == "focus" {
            let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
            self.refresh_managed(cancellation).await?;
            self.set_focus(Some(id.to_string()))?;
            return Ok(text_result(&format!("Focused on design node '{id}'")));
        }
        if action == "unfocus" {
            self.set_focus(None)?;
            return Ok(text_result("Cleared design focus"));
        }

        let observation = self.refresh_managed(cancellation.child_token()).await?;
        let repository = observation
            .repository
            .ok_or_else(|| anyhow::anyhow!("managed lifecycle service is unavailable"))?;
        let nodes = &repository.design.nodes;
        let expected_revision = repository.revision.clone();
        let mut memory_fact = None;
        let mutation = match action {
            "create" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let title = args["title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("title required"))?;
                let status = args["status"]
                    .as_str()
                    .map(|status| {
                        OpsxNodeState::parse(status)
                            .ok_or_else(|| anyhow::anyhow!("Invalid status: {status}"))
                    })
                    .transpose()?;
                DesignMutationV1::Create {
                    id: id.into(),
                    title: title.into(),
                    parent: args["parent"].as_str().map(str::to_string),
                    status,
                    tags: args["tags"]
                        .as_array()
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|value| value.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                    overview: args["overview"].as_str().unwrap_or("").into(),
                }
            }
            "archive" | "set_status" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let status_text = if action == "archive" {
                    "archived"
                } else {
                    args["status"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("status required"))?
                };
                let status = OpsxNodeState::parse(status_text)
                    .ok_or_else(|| anyhow::anyhow!("Invalid status: {status_text}"))?;
                if status == OpsxNodeState::Archived
                    && Self::has_non_archived_descendants(nodes, id)
                {
                    anyhow::bail!("cannot archive '{id}' while non-archived descendants remain");
                }
                if matches!(status_text, "resolved" | "decided" | "implementing") {
                    let title = nodes.get(id).map(|node| node.title.as_str()).unwrap_or(id);
                    memory_fact = Some(format!(
                        "Design node '{id}' ({title}) status → {status_text}"
                    ));
                }
                DesignMutationV1::SetState {
                    id: id.into(),
                    state: status,
                    archive_reason: args["archive_reason"].as_str().map(str::to_string),
                    superseded_by: args["superseded_by"].as_str().map(str::to_string),
                    archived_at: (status == OpsxNodeState::Archived).then(Self::archive_timestamp),
                }
            }
            "add_question" | "remove_question" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let question = args["question"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("question required"))?;
                if action == "add_question" {
                    DesignMutationV1::AddQuestion {
                        id: id.into(),
                        question: question.into(),
                    }
                } else {
                    DesignMutationV1::RemoveQuestion {
                        id: id.into(),
                        question: question.into(),
                    }
                }
            }
            "add_research" => DesignMutationV1::AddResearch {
                id: node_id
                    .ok_or_else(|| anyhow::anyhow!("node_id required"))?
                    .into(),
                heading: args["heading"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("heading required"))?
                    .into(),
                content: args["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("content required"))?
                    .into(),
            },
            "add_decision" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let title = args["decision_title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("decision_title required"))?;
                let status = args["decision_status"].as_str().unwrap_or("exploring");
                let rationale = args["rationale"].as_str().unwrap_or("");
                memory_fact = Some(if rationale.is_empty() {
                    format!("Decision on '{id}': {title} [{status}]")
                } else {
                    format!("Decision on '{id}': {title} [{status}]. {rationale}")
                });
                DesignMutationV1::AddDecision {
                    id: id.into(),
                    title: title.into(),
                    status: status.into(),
                    rationale: rationale.into(),
                }
            }
            "add_dependency" | "add_related" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let target_id = args["target_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("target_id required"))?;
                if action == "add_dependency" {
                    DesignMutationV1::AddDependency {
                        id: id.into(),
                        target_id: target_id.into(),
                    }
                } else {
                    DesignMutationV1::AddRelated {
                        id: id.into(),
                        target_id: target_id.into(),
                    }
                }
            }
            "add_impl_notes" => DesignMutationV1::AddImplementationNotes {
                id: node_id
                    .ok_or_else(|| anyhow::anyhow!("node_id required"))?
                    .into(),
                file_scope: args["file_scope"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| {
                                Some(DesignFileScopeV1 {
                                    path: value["path"].as_str()?.into(),
                                    description: value["description"].as_str().unwrap_or("").into(),
                                    action: value["action"].as_str().map(str::to_string),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                constraints: args["constraints"]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            "branch" => DesignMutationV1::BranchQuestion {
                parent_id: node_id
                    .ok_or_else(|| anyhow::anyhow!("node_id required"))?
                    .into(),
                question: args["question"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("question required"))?
                    .into(),
                child_id: args["child_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("child_id required"))?
                    .into(),
                child_title: args["child_title"]
                    .as_str()
                    .or_else(|| args["question"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("child_title required"))?
                    .into(),
            },
            "implement" => DesignMutationV1::ImplementOpenSpec {
                id: node_id
                    .ok_or_else(|| anyhow::anyhow!("node_id required"))?
                    .into(),
            },
            "set_priority" => {
                let priority = args["priority"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("priority required (1-5)"))?;
                if !(1..=5).contains(&priority) {
                    anyhow::bail!("Priority must be 1-5, got {priority}");
                }
                DesignMutationV1::SetPriority {
                    id: node_id
                        .ok_or_else(|| anyhow::anyhow!("node_id required"))?
                        .into(),
                    priority: priority as u8,
                }
            }
            "set_issue_type" => {
                let issue_type = match args["issue_type"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("issue_type required"))?
                {
                    "epic" => DesignIssueTypeV1::Epic,
                    "feature" => DesignIssueTypeV1::Feature,
                    "task" => DesignIssueTypeV1::Task,
                    "bug" => DesignIssueTypeV1::Bug,
                    "chore" => DesignIssueTypeV1::Chore,
                    other => anyhow::bail!("Invalid issue_type: {other}"),
                };
                DesignMutationV1::SetIssueType {
                    id: node_id
                        .ok_or_else(|| anyhow::anyhow!("node_id required"))?
                        .into(),
                    issue_type,
                }
            }
            _ => anyhow::bail!("Unknown action: {action}"),
        };

        let receipt = self
            .invoke_design_mutation(call_id, expected_revision, mutation, cancellation)
            .await?;
        if let Some(content) = memory_fact
            && let Ok(mut pending) = self.pending_memory.lock()
        {
            pending.push(BusRequest::AutoStoreFact {
                section: "Decisions".into(),
                content,
                source: if action == "add_decision" {
                    "lifecycle:add-decision".into()
                } else {
                    "lifecycle:node-transition".into()
                },
            });
        }

        let text = match action {
            "create" => match receipt.outcome {
                LifecycleMutationOutcomeV1::DesignCreated { path } => format!(
                    "Created design node '{}' at {path}",
                    node_id.unwrap_or_default()
                ),
                _ => format!("Created design node '{}'", node_id.unwrap_or_default()),
            },
            "archive" => format!("Archived '{}'", node_id.unwrap_or_default()),
            "set_status" => format!(
                "Set '{}' status to {}",
                node_id.unwrap_or_default(),
                args["status"].as_str().unwrap_or_default()
            ),
            "add_question" => format!("Added question to '{}'", node_id.unwrap_or_default()),
            "remove_question" => {
                format!("Removed question from '{}'", node_id.unwrap_or_default())
            }
            "add_research" => format!(
                "Added research '{}' to '{}'",
                args["heading"].as_str().unwrap_or_default(),
                node_id.unwrap_or_default()
            ),
            "add_decision" => format!(
                "Added decision '{}' to '{}'",
                args["decision_title"].as_str().unwrap_or_default(),
                node_id.unwrap_or_default()
            ),
            "add_dependency" => format!(
                "Added dependency '{}' → '{}'",
                node_id.unwrap_or_default(),
                args["target_id"].as_str().unwrap_or_default()
            ),
            "add_related" => format!(
                "Added related '{}' ↔ '{}'",
                node_id.unwrap_or_default(),
                args["target_id"].as_str().unwrap_or_default()
            ),
            "add_impl_notes" => {
                format!(
                    "Added implementation notes to '{}'",
                    node_id.unwrap_or_default()
                )
            }
            "branch" => format!(
                "Branched '{}' from '{}', removed question",
                args["child_id"].as_str().unwrap_or_default(),
                node_id.unwrap_or_default()
            ),
            "implement" => match receipt.outcome {
                LifecycleMutationOutcomeV1::DesignImplemented {
                    node_id,
                    change,
                    path,
                } => format!(
                    "Scaffolded OpenSpec change '{change}' at {path}\nNode '{node_id}' → implementing"
                ),
                _ => format!(
                    "Scaffolded OpenSpec change '{}'\nNode '{}' → implementing",
                    node_id.unwrap_or_default(),
                    node_id.unwrap_or_default()
                ),
            },
            "set_priority" => format!(
                "Set '{}' priority to {}",
                node_id.unwrap_or_default(),
                args["priority"].as_u64().unwrap_or_default()
            ),
            "set_issue_type" => format!(
                "Set '{}' issue_type to {}",
                node_id.unwrap_or_default(),
                args["issue_type"].as_str().unwrap_or_default()
            ),
            _ => unreachable!(),
        };
        Ok(text_result(&text))
    }

    async fn invoke_openspec_mutation(
        &self,
        call_id: &str,
        expected_revision: crate::lifecycle_service::LifecycleRepositoryRevisionV1,
        mutation: crate::lifecycle_service::OpenSpecMutationV1,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<crate::lifecycle_service::LifecycleMutationReceiptV1> {
        let response = self
            .managed_binding()?
            .invoke(
                crate::lifecycle_service::LifecycleRequestV1::MutateOpenSpec {
                    operation_id: Self::operation_id(
                        crate::tool_registry::lifecycle::OPENSPEC_MANAGE,
                        call_id,
                    ),
                    expected_revision,
                    mutation: Box::new(mutation),
                    cancellation: cancellation.clone(),
                },
            )
            .await
            .map_err(|error| anyhow::anyhow!("managed lifecycle mutation failed: {error:?}"))?;
        let crate::lifecycle_service::LifecyclePayloadV1::OpenSpecMutation(receipt) =
            response.payload
        else {
            anyhow::bail!("managed lifecycle returned an unexpected OpenSpec mutation response");
        };
        if let Err(error) = self.refresh_managed(cancellation).await {
            tracing::warn!(%error, "managed lifecycle cache refresh failed after OpenSpec mutation");
        }
        Ok(receipt)
    }

    async fn managed_openspec(
        &self,
        call_id: &str,
        args: &Value,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        use crate::lifecycle_service::{LifecycleMutationOutcomeV1, OpenSpecMutationV1};

        let action = args["action"].as_str().unwrap_or("");
        let observation = self.refresh_managed(cancellation.child_token()).await?;
        let repository = observation
            .repository
            .ok_or_else(|| anyhow::anyhow!("managed lifecycle service is unavailable"))?;
        if action == "status" {
            if repository.lifecycle.openspec.changes.is_empty() {
                return Ok(text_result("No active OpenSpec changes."));
            }
            let list = repository
                .lifecycle
                .openspec
                .changes
                .iter()
                .map(|change| {
                    json!({
                        "name": change.name,
                        "state": change.lifecycle_state,
                        "stage": change.lifecycle_state,
                        "file_stage": change.file_stage,
                        "has_proposal": change.has_proposal,
                        "has_specs": change.has_specs,
                        "has_tasks": change.has_tasks,
                        "total_tasks": change.total_tasks,
                        "done_tasks": change.done_tasks,
                    })
                })
                .collect::<Vec<_>>();
            return Ok(text_result(&serde_json::to_string_pretty(&list)?));
        }
        if action == "get" {
            let name = args["change_name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("change_name required"))?;
            let change = repository
                .lifecycle
                .openspec
                .changes
                .iter()
                .find(|change| change.name == name)
                .ok_or_else(|| anyhow::anyhow!("Change '{name}' not found"))?;
            let result = json!({
                "name": change.name,
                "state": change.lifecycle_state,
                "stage": change.lifecycle_state,
                "file_stage": change.file_stage,
                "has_proposal": change.has_proposal,
                "has_design": change.has_design,
                "has_specs": change.has_specs,
                "has_tasks": change.has_tasks,
                "total_tasks": change.total_tasks,
                "done_tasks": change.done_tasks,
                "specs": change.spec_documents.iter().map(|spec| json!({
                    "domain": spec.domain,
                    "requirements": spec.requirements.iter().map(|requirement| json!({
                        "title": requirement.title,
                        "scenarios": requirement.scenarios.len(),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            });
            return Ok(text_result(&serde_json::to_string_pretty(&result)?));
        }

        let expected_revision = repository.revision.clone();
        let mutation = match action {
            "propose" => OpenSpecMutationV1::Propose {
                name: args["name"]
                    .as_str()
                    .or_else(|| args["change_name"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?
                    .into(),
                title: args["title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("title required"))?
                    .into(),
                intent: args["intent"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("intent required"))?
                    .into(),
                bound_node: None,
            },
            "add_spec" => OpenSpecMutationV1::AddSpec {
                change: args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?
                    .into(),
                domain: args["domain"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("domain required"))?
                    .into(),
                content: args["spec_content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("spec_content required"))?
                    .into(),
            },
            "register_tasks" => {
                if args.get("total_tasks").is_some() || args.get("done_tasks").is_some() {
                    anyhow::bail!(
                        "register_tasks reads task counts from tasks.md; update OpenSpec tasks first"
                    );
                }
                OpenSpecMutationV1::ReconcileTasks {
                    change: args["change_name"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("change_name required"))?
                        .into(),
                }
            }
            "set_task_status" => {
                let status = args["status"].as_str().unwrap_or("done");
                let done = match status {
                    "done" | "complete" | "completed" => true,
                    "pending" | "open" | "reopen" => false,
                    other => {
                        anyhow::bail!("unsupported task status '{other}'; expected done or pending")
                    }
                };
                OpenSpecMutationV1::SetTaskStatus {
                    change: args["change_name"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("change_name required"))?
                        .into(),
                    group: args["group"]
                        .as_str()
                        .or_else(|| args["group_title"].as_str())
                        .ok_or_else(|| anyhow::anyhow!("group required"))?
                        .into(),
                    task_id: args["task_id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("task_id required"))?
                        .into(),
                    done,
                }
            }
            "register_test_file" => {
                let path = args["path"]
                    .as_str()
                    .or_else(|| args["test_file"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("path required"))?;
                if path.trim().is_empty() {
                    anyhow::bail!("path required");
                }
                OpenSpecMutationV1::RegisterTestFile {
                    change: args["change_name"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("change_name required"))?
                        .into(),
                    path: path.into(),
                }
            }
            "archive" => OpenSpecMutationV1::Archive {
                change: args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?
                    .into(),
            },
            _ => anyhow::bail!(
                "Unknown action: {action}. Valid: status, get, propose, add_spec, register_tasks, set_task_status, register_test_file, archive"
            ),
        };
        let receipt = self
            .invoke_openspec_mutation(call_id, expected_revision, mutation, cancellation)
            .await?;
        let text = match action {
            "propose" => match receipt.outcome {
                LifecycleMutationOutcomeV1::OpenSpecProposed { path } => format!(
                    "Proposed change '{}' at {path}",
                    args["name"]
                        .as_str()
                        .or_else(|| args["change_name"].as_str())
                        .unwrap_or_default()
                ),
                _ => format!(
                    "Proposed change '{}'",
                    args["name"]
                        .as_str()
                        .or_else(|| args["change_name"].as_str())
                        .unwrap_or_default()
                ),
            },
            "add_spec" => match receipt.outcome {
                LifecycleMutationOutcomeV1::OpenSpecSpecAdded { path } => format!(
                    "Added spec '{}' to '{}' at {path}",
                    args["domain"].as_str().unwrap_or_default(),
                    args["change_name"].as_str().unwrap_or_default()
                ),
                _ => format!(
                    "Added spec '{}' to '{}'",
                    args["domain"].as_str().unwrap_or_default(),
                    args["change_name"].as_str().unwrap_or_default()
                ),
            },
            "register_tasks" => match receipt.outcome {
                LifecycleMutationOutcomeV1::OpenSpecTasksReconciled {
                    total_tasks,
                    done_tasks,
                } => format!(
                    "Registered tasks for '{}': {done_tasks}/{total_tasks}",
                    args["change_name"].as_str().unwrap_or_default()
                ),
                _ => format!(
                    "Registered tasks for '{}'",
                    args["change_name"].as_str().unwrap_or_default()
                ),
            },
            "set_task_status" => match receipt.outcome {
                LifecycleMutationOutcomeV1::OpenSpecTaskStatusChanged {
                    change,
                    group,
                    task_id,
                    path,
                    line,
                    previous_done,
                    new_done,
                    description,
                } => format!(
                    "Updated OpenSpec task:\n- change: {change}\n- group: {group}\n- task: {task_id}\n- file: {path}:{line}\n- status: {} -> {}\n- description: {description}",
                    if previous_done { "done" } else { "pending" },
                    if new_done { "done" } else { "pending" }
                ),
                _ => "Updated OpenSpec task".into(),
            },
            "register_test_file" => format!(
                "Registered test file '{}' for '{}'",
                args["path"]
                    .as_str()
                    .or_else(|| args["test_file"].as_str())
                    .unwrap_or_default(),
                args["change_name"].as_str().unwrap_or_default()
            ),
            "archive" => format!(
                "Archived change '{}'",
                args["change_name"].as_str().unwrap_or_default()
            ),
            _ => unreachable!(),
        };
        Ok(text_result(&text))
    }

    async fn managed_doctor(
        &self,
        args: &Value,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let response = self
            .managed_binding()?
            .invoke(crate::lifecycle_service::LifecycleRequestV1::Doctor { cancellation })
            .await
            .map_err(|error| anyhow::anyhow!("managed lifecycle doctor failed: {error:?}"))?;
        let crate::lifecycle_service::LifecyclePayloadV1::Doctor(report) = response.payload else {
            anyhow::bail!("managed lifecycle returned an unexpected doctor response");
        };
        let kinds = args["kinds"].as_array().map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<std::collections::HashSet<_>>()
        });
        let node_id = args["node_id"].as_str();
        let findings = report
            .findings
            .iter()
            .filter(|finding| {
                node_id.is_none_or(|id| finding.node_id == id)
                    && kinds
                        .as_ref()
                        .is_none_or(|values| values.contains(finding.kind.as_str()))
            })
            .collect::<Vec<_>>();
        let counts = findings
            .iter()
            .fold(serde_json::Map::new(), |mut counts, finding| {
                let next = counts
                    .get(&finding.kind)
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    + 1;
                counts.insert(finding.kind.clone(), json!(next));
                counts
            });
        let details = json!({
            "findings": findings.iter().map(|finding| json!({
                "node_id": finding.node_id,
                "title": finding.title,
                "kind": finding.kind,
                "detail": finding.detail,
            })).collect::<Vec<_>>(),
            "counts": counts,
            "total": findings.len(),
            "recovered": report.recovered,
        });
        let text = if findings.is_empty() {
            "✓ No suspicious lifecycle drift found.".into()
        } else {
            let mut text = format!("Lifecycle doctor: {} finding(s)\n\n", findings.len());
            for finding in findings {
                text.push_str(&format!(
                    "- {} [{}]\n  {}\n  {}\n",
                    finding.node_id, finding.kind, finding.title, finding.detail
                ));
            }
            text.trim_end().into()
        };
        Ok(ToolResult {
            content: vec![ContentBlock::Text { text }],
            details,
        })
    }

    /// Bootstrap a markdown design node into omegon-opsx.
    /// Creates the node and syncs state + open questions from the markdown source.
    #[cfg(test)]
    fn bootstrap_node_to_opsx(&self, opsx: &mut OpsxLifecycle<JsonFileStore>, node: &DesignNode) {
        let current_opsx =
            OpsxNodeState::parse(node.status.as_str()).unwrap_or(OpsxNodeState::Seed);
        // Create (parent validation is skipped — parent may not be in opsx yet)
        let _ = opsx.create_node(&node.id, &node.title, None);
        if current_opsx != OpsxNodeState::Seed {
            let _ =
                opsx.force_transition_node(&node.id, current_opsx, "bootstrap sync from markdown");
        }
        // Sync open questions
        for q in &node.open_questions {
            let _ = opsx.add_question(&node.id, q);
        }
    }

    // ── Tool dispatch ───────────────────────────────────────────────────

    #[cfg(test)]
    fn execute_design_tree(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");
        let node_id = args["node_id"].as_str();
        let p = self.compatibility_provider().lock().unwrap();

        match action {
            "list" => {
                let nodes = p.all_nodes();
                let list: Vec<Value> = nodes
                    .values()
                    .filter(|n| !query::is_archived(n))
                    .map(|n| {
                        let children_count = design::get_children(nodes, &n.id).len();
                        json!({
                            "id": n.id,
                            "title": n.title,
                            "status": n.status.as_str(),
                            "parent": n.parent,
                            "tags": n.tags,
                            "open_questions": n.open_questions.len(),
                            "dependencies": n.dependencies,
                            "branches": n.branches,
                            "openspec_change": n.openspec_change,
                            "priority": n.priority,
                            "issue_type": n.issue_type.map(|t| match t {
                                IssueType::Epic => "epic",
                                IssueType::Feature => "feature",
                                IssueType::Task => "task",
                                IssueType::Bug => "bug",
                                IssueType::Chore => "chore",
                            }),
                            "children": children_count,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&list)?))
            }

            "node" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let node = p
                    .get_node(id)
                    .ok_or_else(|| anyhow::anyhow!("Node '{id}' not found"))?;
                let sections = design::read_node_sections(node);
                let children = design::get_children(p.all_nodes(), id);

                let mut result = json!({
                    "id": node.id,
                    "title": node.title,
                    "status": node.status.as_str(),
                    "parent": node.parent,
                    "tags": node.tags,
                    "open_questions": node.open_questions,
                    "dependencies": node.dependencies,
                    "related": node.related,
                    "branches": node.branches,
                    "openspec_change": node.openspec_change,
                    "priority": node.priority,
                    "archive_reason": node.archive_reason,
                    "superseded_by": node.superseded_by,
                    "archived_at": node.archived_at,
                    "children": children.iter().map(|c| json!({
                        "id": c.id,
                        "title": c.title,
                        "status": c.status.as_str(),
                    })).collect::<Vec<_>>(),
                });

                if let Some(ref s) = sections {
                    result["overview"] = json!(s.overview);
                    result["research"] = json!(
                        s.research
                            .iter()
                            .map(|r| json!({
                                "heading": r.heading,
                                "content": r.content,
                            }))
                            .collect::<Vec<_>>()
                    );
                    result["decisions"] = json!(
                        s.decisions
                            .iter()
                            .map(|d| json!({
                                "title": d.title,
                                "status": d.status,
                                "rationale": d.rationale,
                            }))
                            .collect::<Vec<_>>()
                    );
                    result["impl_file_scope"] = json!(
                        s.impl_file_scope
                            .iter()
                            .map(|f| json!({
                                "path": f.path,
                                "description": f.description,
                                "action": f.action,
                            }))
                            .collect::<Vec<_>>()
                    );
                    result["impl_constraints"] = json!(s.impl_constraints);

                    // Knowledge quadrant readiness
                    result["readiness"] = json!({
                        "score": s.readiness_score(),
                        "decisions": s.decisions.iter().filter(|d| d.status == "decided").count(),
                        "questions": s.question_count(),
                        "assumptions": s.assumption_count(),
                    });
                }

                Ok(text_result(&serde_json::to_string_pretty(&result)?))
            }

            "frontier" => {
                let frontier: Vec<Value> = query::frontier(p.all_nodes())
                    .into_iter()
                    .map(|n| {
                        json!({
                            "id": n.id,
                            "title": n.title,
                            "status": n.status,
                            "open_questions": n.open_questions,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&frontier)?))
            }

            "children" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let list: Vec<Value> = query::children(p.all_nodes(), id)
                    .into_iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "title": c.title,
                            "status": c.status,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&list)?))
            }

            "dependencies" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let node = p
                    .get_node(id)
                    .ok_or_else(|| anyhow::anyhow!("Node '{id}' not found"))?;
                let deps: Vec<Value> = query::dependencies(p.all_nodes(), node)
                    .into_iter()
                    .map(|d| {
                        json!({
                            "id": d.id,
                            "title": d.title,
                            "status": d.status,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&deps)?))
            }

            "ready" => {
                let ready: Vec<Value> = query::ready(p.all_nodes())
                    .into_iter()
                    .map(|n| {
                        json!({
                            "id": n.id,
                            "title": n.title,
                            "priority": n.priority,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&ready)?))
            }

            "blocked" => {
                let blocked: Vec<Value> = query::blocked(p.all_nodes())
                    .into_iter()
                    .map(|n| {
                        json!({
                            "id": n.id,
                            "title": n.title,
                            "status": n.status,
                            "blocked_by": n.blocked_by,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&blocked)?))
            }

            _ => anyhow::bail!(
                "Unknown action: {action}. Valid: list, node, frontier, children, dependencies, ready, blocked"
            ),
        }
    }

    #[cfg(test)]
    fn execute_design_tree_update(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");
        let node_id = args["node_id"].as_str();
        match action {
            "create" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let title = args["title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("title required"))?;
                let tags: Vec<String> = args["tags"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let node = self.compatibility_mutation_service().create_design_node(
                    CreateDesignNodeRequest {
                        id: id.to_string(),
                        title: title.to_string(),
                        parent: args["parent"].as_str().map(str::to_string),
                        status: args["status"].as_str().map(str::to_string),
                        tags,
                        overview: args["overview"].as_str().unwrap_or("").to_string(),
                    },
                )?;
                Ok(text_result(&format!(
                    "Created design node '{id}' at {}",
                    node.file_path.display()
                )))
            }

            "archive" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let nodes = self
                    .compatibility_provider()
                    .lock()
                    .unwrap()
                    .all_nodes()
                    .clone();
                if Self::has_non_archived_descendants(&nodes, id) {
                    anyhow::bail!("cannot archive '{id}' while non-archived descendants remain");
                }

                self.compatibility_mutation_service()
                    .set_design_node_status(SetDesignNodeStatusRequest {
                        id: id.to_string(),
                        status: NodeStatus::Archived,
                        archive_reason: args["archive_reason"].as_str().map(str::to_string),
                        superseded_by: args["superseded_by"].as_str().map(str::to_string),
                        archived_at: Some(Self::archive_timestamp()),
                    })?;
                Ok(text_result(&format!("Archived '{id}'")))
            }

            "set_status" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let status_str = args["status"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("status required"))?;
                let status = NodeStatus::parse(status_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid status: {status_str}"))?;

                if matches!(status, NodeStatus::Archived) {
                    let nodes = self
                        .compatibility_provider()
                        .lock()
                        .unwrap()
                        .all_nodes()
                        .clone();
                    if Self::has_non_archived_descendants(&nodes, id) {
                        anyhow::bail!(
                            "cannot archive '{id}' while non-archived descendants remain"
                        );
                    }
                }

                let result = self
                    .compatibility_mutation_service()
                    .set_design_node_status(SetDesignNodeStatusRequest {
                        id: id.to_string(),
                        status,
                        archive_reason: args["archive_reason"].as_str().map(str::to_string),
                        superseded_by: args["superseded_by"].as_str().map(str::to_string),
                        archived_at: if matches!(status, NodeStatus::Archived) {
                            Some(Self::archive_timestamp())
                        } else {
                            None
                        },
                    })?;

                if matches!(status_str, "resolved" | "decided" | "implementing") {
                    let content = format!(
                        "Design node '{id}' ({}) status → {status_str}",
                        result.node_title
                    );
                    if let Ok(mut q) = self.pending_memory.lock() {
                        q.push(BusRequest::AutoStoreFact {
                            section: "Decisions".into(),
                            content,
                            source: "lifecycle:node-transition".into(),
                        });
                    }
                }

                Ok(text_result(&format!("Set '{id}' status to {status_str}")))
            }

            "add_question" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let question = args["question"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("question required"))?;

                self.compatibility_mutation_service()
                    .add_design_node_question(UpdateDesignNodeQuestionRequest {
                        id: id.to_string(),
                        question: question.to_string(),
                    })?;
                Ok(text_result(&format!("Added question to '{id}'")))
            }

            "remove_question" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let question = args["question"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("question required"))?;

                self.compatibility_mutation_service()
                    .remove_design_node_question(UpdateDesignNodeQuestionRequest {
                        id: id.to_string(),
                        question: question.to_string(),
                    })?;
                Ok(text_result(&format!("Removed question from '{id}'")))
            }

            "add_research" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let heading = args["heading"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("heading required"))?;
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("content required"))?;

                self.compatibility_mutation_service()
                    .add_design_node_research(AddDesignNodeResearchRequest {
                        id: id.to_string(),
                        heading: heading.to_string(),
                        content: content.to_string(),
                    })?;
                Ok(text_result(&format!(
                    "Added research '{heading}' to '{id}'"
                )))
            }

            "add_decision" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let title = args["decision_title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("decision_title required"))?;
                let status = args["decision_status"].as_str().unwrap_or("exploring");
                let rationale = args["rationale"].as_str().unwrap_or("");

                self.compatibility_mutation_service()
                    .add_design_node_decision(AddDesignNodeDecisionRequest {
                        id: id.to_string(),
                        title: title.to_string(),
                        status: status.to_string(),
                        rationale: rationale.to_string(),
                    })?;

                // Auto-ingest decisions to memory
                let content = if rationale.is_empty() {
                    format!("Decision on '{id}': {title} [{status}]")
                } else {
                    format!("Decision on '{id}': {title} [{status}]. {rationale}")
                };
                if let Ok(mut q) = self.pending_memory.lock() {
                    q.push(BusRequest::AutoStoreFact {
                        section: "Decisions".into(),
                        content,
                        source: "lifecycle:add-decision".into(),
                    });
                }

                Ok(text_result(&format!("Added decision '{title}' to '{id}'")))
            }

            "add_dependency" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let target = args["target_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("target_id required"))?;

                self.compatibility_mutation_service()
                    .add_design_node_dependency(AddDesignNodeLinkRequest {
                        id: id.to_string(),
                        target_id: target.to_string(),
                    })?;
                Ok(text_result(&format!(
                    "Added dependency '{id}' → '{target}'"
                )))
            }

            "add_related" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let target = args["target_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("target_id required"))?;

                self.compatibility_mutation_service()
                    .add_design_node_related(AddDesignNodeLinkRequest {
                        id: id.to_string(),
                        target_id: target.to_string(),
                    })?;
                Ok(text_result(&format!("Added related '{id}' ↔ '{target}'")))
            }

            "add_impl_notes" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let file_scope: Vec<FileScope> = args["file_scope"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| {
                                Some(FileScope {
                                    path: v["path"].as_str()?.to_string(),
                                    description: v["description"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string(),
                                    action: v["action"].as_str().map(String::from),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let constraints: Vec<String> = args["constraints"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                self.compatibility_mutation_service()
                    .add_design_node_impl_notes(AddDesignNodeImplNotesRequest {
                        id: id.to_string(),
                        file_scope,
                        constraints,
                    })?;
                Ok(text_result(&format!(
                    "Added implementation notes to '{id}'"
                )))
            }

            "branch" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let question = args["question"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("question required"))?;
                let child_id = args["child_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("child_id required"))?;
                let child_title = args["child_title"].as_str().unwrap_or(question);

                self.compatibility_mutation_service()
                    .branch_design_node_question(BranchDesignNodeQuestionRequest {
                        parent_id: id.to_string(),
                        question: question.to_string(),
                        child_id: child_id.to_string(),
                        child_title: child_title.to_string(),
                    })?;
                Ok(text_result(&format!(
                    "Branched '{child_id}' from '{id}', removed question"
                )))
            }

            "focus" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                if self
                    .compatibility_provider()
                    .lock()
                    .unwrap()
                    .get_node(id)
                    .is_none()
                {
                    anyhow::bail!("Node '{id}' not found");
                }
                self.set_focus(Some(id.to_string()))?;
                Ok(text_result(&format!("Focused on design node '{id}'")))
            }

            "unfocus" => {
                self.set_focus(None)?;
                Ok(text_result("Cleared design focus"))
            }

            "implement" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let result = self
                    .compatibility_mutation_service()
                    .implement_design_node(ImplementDesignNodeRequest { id: id.to_string() })?;
                Ok(text_result(&format!(
                    "Scaffolded OpenSpec change '{}' at {}\nNode '{}' → implementing",
                    result.openspec_change,
                    result.change_path.display(),
                    result.node_id
                )))
            }

            "set_priority" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let priority = args["priority"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("priority required (1-5)"))?;
                if !(1..=5).contains(&priority) {
                    anyhow::bail!("Priority must be 1-5, got {priority}");
                }

                self.compatibility_mutation_service()
                    .set_design_node_priority(SetDesignNodePriorityRequest {
                        id: id.to_string(),
                        priority: priority as u8,
                    })?;
                Ok(text_result(&format!("Set '{id}' priority to {priority}")))
            }

            "set_issue_type" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let type_str = args["issue_type"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("issue_type required"))?;
                let issue_type = IssueType::parse(type_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid issue_type: {type_str}"))?;

                self.compatibility_mutation_service()
                    .set_design_node_issue_type(SetDesignNodeIssueTypeRequest {
                        id: id.to_string(),
                        issue_type,
                    })?;
                Ok(text_result(&format!("Set '{id}' issue_type to {type_str}")))
            }

            _ => anyhow::bail!("Unknown action: {action}"),
        }
    }

    #[cfg(test)]
    fn execute_lifecycle_doctor(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let kinds_filter: Option<std::collections::HashSet<&str>> = args["kinds"]
            .as_array()
            .map(|values| values.iter().filter_map(|v| v.as_str()).collect());
        let node_filter = args["node_id"].as_str();

        let recovered =
            archive::recover_archive_transactions(&self.repo_path, self.compatibility_opsx())?;

        let mut findings = doctor::audit_repo(&self.repo_path);
        let changes = spec::list_changes(&self.repo_path);
        let opsx_states = self
            .compatibility_opsx()
            .lock()
            .unwrap()
            .state()
            .changes
            .iter()
            .map(|c| (c.name.clone(), c.state))
            .collect();
        findings.extend(doctor::audit_openspec_changes(&changes, &opsx_states));
        findings.extend(doctor::audit_openspec_archives(
            &self.repo_path,
            &opsx_states,
        ));

        let filtered: Vec<&doctor::AuditFinding> = findings
            .iter()
            .filter(|finding| {
                node_filter.is_none_or(|node_id| finding.node_id == node_id)
                    && kinds_filter
                        .as_ref()
                        .is_none_or(|kinds| kinds.contains(finding.kind.as_str()))
            })
            .collect();

        let counts = filtered
            .iter()
            .fold(serde_json::Map::new(), |mut acc, finding| {
                let key = finding.kind.as_str().to_string();
                let next = acc.get(&key).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                acc.insert(key, json!(next));
                acc
            });

        let details = json!({
            "findings": filtered.iter().map(|f| json!({
                "node_id": f.node_id,
                "title": f.title,
                "kind": f.kind.as_str(),
                "detail": f.detail,
            })).collect::<Vec<_>>(),
            "counts": counts,
            "total": filtered.len(),
            "recovered": recovered,
        });

        let text = if filtered.is_empty() {
            "✓ No suspicious lifecycle drift found.".to_string()
        } else {
            let mut out = format!("Lifecycle doctor: {} finding(s)\n\n", filtered.len());
            for f in &filtered {
                out.push_str(&format!(
                    "- {} [{}]\n  {}\n  {}\n",
                    f.node_id,
                    f.kind.as_str(),
                    f.title,
                    f.detail
                ));
            }
            out.trim_end().to_string()
        };

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text }],
            details,
        })
    }

    #[cfg(test)]
    fn execute_openspec_manage(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");

        match action {
            "status" => {
                self.compatibility_provider().lock().unwrap().refresh();
                let p = self.compatibility_provider().lock().unwrap();
                let changes = p.changes();
                if changes.is_empty() {
                    return Ok(text_result("No active OpenSpec changes."));
                }
                let opsx_states = self.opsx_change_states();
                let list: Vec<Value> = changes
                    .iter()
                    .map(|c| {
                        let state = opsx_states
                            .get(&c.name)
                            .cloned()
                            .unwrap_or_else(|| c.state.as_str().to_string());
                        json!({
                            "name": c.name,
                            "state": state,
                            "stage": state,
                            "file_stage": c.state.as_str(),
                            "has_proposal": c.has_proposal,
                            "has_specs": c.has_specs,
                            "has_tasks": c.has_tasks,
                            "total_tasks": c.total_tasks,
                            "done_tasks": c.done_tasks,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&list)?))
            }

            "get" => {
                let name = args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                let change = spec::get_change(&self.repo_path, name)
                    .ok_or_else(|| anyhow::anyhow!("Change '{name}' not found"))?;
                let state = self
                    .opsx_change_states()
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| change.state.as_str().to_string());

                let result = json!({
                    "name": change.name,
                    "state": state,
                    "stage": state,
                    "file_stage": change.state.as_str(),
                    "has_proposal": change.has_proposal,
                    "has_design": change.has_design,
                    "has_specs": change.has_specs,
                    "has_tasks": change.has_tasks,
                    "total_tasks": change.total_tasks,
                    "done_tasks": change.done_tasks,
                    "specs": change.specs.iter().map(|s| json!({
                        "domain": s.domain,
                        "requirements": s.requirements.iter().map(|r| json!({
                            "title": r.title,
                            "scenarios": r.scenarios.len(),
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                });
                Ok(text_result(&serde_json::to_string_pretty(&result)?))
            }

            "propose" => {
                let name = args["name"]
                    .as_str()
                    .or_else(|| args["change_name"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let title = args["title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("title required"))?;
                let intent = args["intent"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("intent required"))?;

                if self
                    .compatibility_opsx()
                    .lock()
                    .unwrap()
                    .state()
                    .changes
                    .iter()
                    .any(|c| c.name == name)
                {
                    anyhow::bail!("Change '{name}' already exists");
                }
                let change = spec::propose_change(&self.repo_path, name, title, intent)?;
                if let Err(err) = self
                    .compatibility_opsx()
                    .lock()
                    .unwrap()
                    .create_change(name, title, None)
                {
                    let _ = std::fs::remove_dir_all(&change.path);
                    return Err(err.into());
                }
                self.compatibility_provider().lock().unwrap().refresh();
                Ok(text_result(&format!(
                    "Proposed change '{name}' at {}",
                    change.path.display()
                )))
            }

            "add_spec" => {
                let name = args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                let domain = args["domain"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("domain required"))?;
                let content = args["spec_content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("spec_content required"))?;

                let path = omegon_opsx::OpenSpecRepository::new(&self.repo_path)
                    .add_spec(name, domain, content)?;
                {
                    let mut opsx = self.compatibility_opsx().lock().unwrap();
                    sync::sync_change_by_name(&mut opsx, &self.repo_path, name)?;
                }
                self.compatibility_provider().lock().unwrap().refresh();
                Ok(text_result(&format!(
                    "Added spec '{domain}' to '{name}' at {}",
                    path.display()
                )))
            }

            "register_tasks" => {
                let name = args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?;

                let mut opsx = self.compatibility_opsx().lock().unwrap();
                let (change, _) = sync::sync_change_by_name(&mut opsx, &self.repo_path, name)?;
                if args.get("total_tasks").is_some() || args.get("done_tasks").is_some() {
                    anyhow::bail!(
                        "register_tasks reads task counts from tasks.md; update OpenSpec tasks first"
                    );
                }
                let total_tasks = change.total_tasks;
                let done_tasks = change.done_tasks;
                if done_tasks > total_tasks {
                    anyhow::bail!("done_tasks cannot exceed total_tasks");
                }
                opsx.update_change_progress(name, total_tasks, done_tasks)?;
                sync::transition_change_if(
                    &mut opsx,
                    &self.repo_path,
                    name,
                    OpsxChangeState::Specced,
                    OpsxChangeState::Planned,
                )?;
                if total_tasks > 0
                    && done_tasks >= total_tasks
                    && sync::change_state(&opsx, name) == Some(OpsxChangeState::Implementing)
                {
                    opsx.transition_change(name, OpsxChangeState::Verifying)?;
                }
                Ok(text_result(&format!(
                    "Registered tasks for '{name}': {done_tasks}/{total_tasks}"
                )))
            }

            "set_task_status" => {
                let name = args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                let group = args["group"]
                    .as_str()
                    .or_else(|| args["group_title"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("group required"))?;
                let task_id = args["task_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("task_id required"))?;
                let status = match args["status"].as_str().unwrap_or("done") {
                    "done" | "complete" | "completed" => spec::TaskCheckboxStatus::Done,
                    "pending" | "open" | "reopen" => spec::TaskCheckboxStatus::Pending,
                    other => {
                        anyhow::bail!("unsupported task status '{other}'; expected done or pending")
                    }
                };
                let report =
                    spec::set_task_checkbox_status(&self.repo_path, name, group, task_id, status)?;
                {
                    let mut opsx = self.compatibility_opsx().lock().unwrap();
                    let (change, _) = sync::sync_change_by_name(&mut opsx, &self.repo_path, name)?;
                    opsx.update_change_progress(name, change.total_tasks, change.done_tasks)?;
                }
                self.compatibility_provider().lock().unwrap().refresh();
                Ok(text_result(&format!(
                    "Updated OpenSpec task:
- change: {}
- group: {}
- task: {}
- file: {}:{}
- status: {} -> {}
- description: {}",
                    report.change,
                    report.group,
                    report.task_id,
                    report.path.display(),
                    report.line,
                    if report.previous_done {
                        "done"
                    } else {
                        "pending"
                    },
                    if report.new_done { "done" } else { "pending" },
                    report.description
                )))
            }

            "register_test_file" => {
                let name = args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                let path = args["path"]
                    .as_str()
                    .or_else(|| args["test_file"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("path required"))?;
                if path.trim().is_empty() {
                    anyhow::bail!("path required");
                }

                let mut opsx = self.compatibility_opsx().lock().unwrap();
                sync::sync_change_by_name(&mut opsx, &self.repo_path, name)?;
                let state = sync::change_state(&opsx, name);
                if !matches!(
                    state,
                    Some(
                        OpsxChangeState::Planned
                            | OpsxChangeState::Testing
                            | OpsxChangeState::Implementing
                    )
                ) {
                    anyhow::bail!("Change '{name}' must be planned before registering test files");
                }
                sync::transition_change_if(
                    &mut opsx,
                    &self.repo_path,
                    name,
                    OpsxChangeState::Planned,
                    OpsxChangeState::Testing,
                )?;
                opsx.add_test_file(name, path)?;
                sync::transition_change_if(
                    &mut opsx,
                    &self.repo_path,
                    name,
                    OpsxChangeState::Testing,
                    OpsxChangeState::Implementing,
                )?;
                Ok(text_result(&format!(
                    "Registered test file '{path}' for '{name}'"
                )))
            }

            "archive" => {
                let name = args["change_name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                archive::recover_archive_transactions(&self.repo_path, self.compatibility_opsx())?;
                let change_dir = self.repo_path.join("openspec/changes").join(name);
                if !change_dir.exists() {
                    anyhow::bail!("Change '{name}' does not exist");
                }
                let archive_dir = self.repo_path.join("openspec/archive").join(name);
                if archive_dir.exists() {
                    anyhow::bail!("Archived change '{name}' already exists");
                }
                {
                    let mut opsx = self.compatibility_opsx().lock().unwrap();
                    sync::sync_change_by_name(&mut opsx, &self.repo_path, name)?;
                    if sync::change_state(&opsx, name) != Some(OpsxChangeState::Verifying) {
                        anyhow::bail!(
                            "Change '{name}' must be verifying before archive; register specs, tasks, test files, and completed tasks first"
                        );
                    }
                    let tx_from_state =
                        sync::change_state(&opsx, name).unwrap_or(OpsxChangeState::Verifying);
                    let archive_repo = self.repo_path.clone();
                    let archive_name = name.to_string();
                    let rollback_repo = self.repo_path.clone();
                    let rollback_name = name.to_string();
                    opsx.archive_change_with(
                        name,
                        move || {
                            archive::archive_content_with_tx(
                                &archive_repo,
                                &archive_name,
                                tx_from_state,
                            )
                        },
                        move || archive::rollback_archive_content(&rollback_repo, &rollback_name),
                    )?;
                    archive::remove_archive_tx(&self.repo_path, name)?;
                }
                self.compatibility_provider().lock().unwrap().refresh();
                Ok(text_result(&format!("Archived change '{name}'")))
            }

            _ => anyhow::bail!(
                "Unknown action: {action}. Valid: status, get, propose, add_spec, register_tasks, set_task_status, register_test_file, archive"
            ),
        }
    }
}

#[async_trait]
impl Feature for LifecycleFeature {
    fn name(&self) -> &str {
        "lifecycle"
    }

    fn runtime_contribution_generation_id(&self) -> Option<RuntimeContributionGenerationId> {
        Some(
            RuntimeContributionGenerationId::new(crate::lifecycle_service::LIFECYCLE_GENERATION)
                .expect("static generation id is valid"),
        )
    }

    fn runtime_lifecycle_policy(&self) -> Option<RuntimeLifecyclePolicy> {
        Some(RuntimeLifecyclePolicy {
            requirement: RuntimeLifecycleRequirement::Optional,
            failure_disposition: RuntimeFailureDisposition::DegradeLocally,
            readiness_timeout_ms: 0,
            heartbeat_timeout_ms: None,
            restart_limit: 0,
        })
    }

    fn runtime_transition_policy(&self) -> Option<RuntimeCompositionTransitionPolicy> {
        Some(RuntimeCompositionTransitionPolicy {
            activation_boundary: RuntimeActivationBoundary::Boot,
            cleanup: RuntimeCleanupRequirement::Strict,
            cleanup_timeout_ms: 5_000,
        })
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::lifecycle::DESIGN_TREE.into(),
                label: "design_tree".into(),
                description: "Query the design tree: list nodes, get node details, find open questions (frontier), check dependencies, list children.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "node", "frontier", "dependencies", "children", "ready", "blocked"],
                            "description": "Query action"
                        },
                        "node_id": {
                            "type": "string",
                            "description": "Node ID (required for node, dependencies, children)"
                        }
                    },
                    "required": ["action"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::lifecycle::DESIGN_TREE_UPDATE.into(),
                label: "design_tree_update".into(),
                description: "Mutate the design tree: create nodes, change status, archive stale nodes, add questions/research/decisions, branch, set focus, implement.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["create", "set_status", "archive", "add_question", "remove_question", "add_research", "add_decision", "add_dependency", "add_related", "add_impl_notes", "branch", "focus", "unfocus", "implement", "set_priority", "set_issue_type"],
                            "description": "Mutation action"
                        },
                        "node_id": { "type": "string", "description": "Primary design node ID. Required for most actions; for create, this is the new node ID." },
                        "title": { "type": "string", "description": "Node title. Required for create." },
                        "parent": { "type": "string", "description": "Parent node ID for create, branch, or implement." },
                        "status": { "type": "string", "description": "Lifecycle status. Required for set_status; optional initial status for create." },
                        "archive_reason": { "type": "string", "description": "Archive reason for archive or archived status transitions." },
                        "superseded_by": { "type": "string", "description": "Replacement node ID when archived as superseded." },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "overview": { "type": "string", "description": "Node overview/summary. Required for create." },
                        "question": { "type": "string", "description": "Open question text. Required for add_question/remove_question." },
                        "heading": { "type": "string", "description": "Research heading or impl-notes heading. Required for add_research/add_impl_notes." },
                        "content": { "type": "string", "description": "Body content. Required for add_research/add_impl_notes." },
                        "decision_title": { "type": "string", "description": "Decision title. Required for add_decision." },
                        "decision_status": { "type": "string" },
                        "rationale": { "type": "string" },
                        "target_id": { "type": "string", "description": "Target node ID. Required for add_dependency/add_related/focus/unfocus/set_priority/set_issue_type when applicable." },
                        "child_id": { "type": "string", "description": "Child node ID. Required for branch." },
                        "child_title": { "type": "string", "description": "Child node title. Required for branch." },
                        "file_scope": { "type": "array", "items": { "type": "object" } },
                        "constraints": { "type": "array", "items": { "type": "string" } },
                        "priority": { "type": "number", "description": "Priority value. Required for set_priority." },
                        "issue_type": { "type": "string", "description": "Issue classification. Required for set_issue_type." }
                    },
                    "required": ["action"],
                    "allOf": [
                        { "if": { "properties": { "action": { "const": "create" } } }, "then": { "required": ["action", "node_id", "title", "overview"] } },
                        { "if": { "properties": { "action": { "const": "set_status" } } }, "then": { "required": ["action", "node_id", "status"] } },
                        { "if": { "properties": { "action": { "const": "archive" } } }, "then": { "required": ["action", "node_id"] } },
                        { "if": { "properties": { "action": { "const": "add_question" } } }, "then": { "required": ["action", "node_id", "question"] } },
                        { "if": { "properties": { "action": { "const": "remove_question" } } }, "then": { "required": ["action", "node_id", "question"] } },
                        { "if": { "properties": { "action": { "const": "add_research" } } }, "then": { "required": ["action", "node_id", "heading", "content"] } },
                        { "if": { "properties": { "action": { "const": "add_decision" } } }, "then": { "required": ["action", "node_id", "decision_title"] } },
                        { "if": { "properties": { "action": { "const": "add_dependency" } } }, "then": { "required": ["action", "node_id", "target_id"] } },
                        { "if": { "properties": { "action": { "const": "add_related" } } }, "then": { "required": ["action", "node_id", "target_id"] } },
                        { "if": { "properties": { "action": { "const": "add_impl_notes" } } }, "then": { "required": ["action", "node_id", "heading", "content"] } },
                        { "if": { "properties": { "action": { "const": "branch" } } }, "then": { "required": ["action", "node_id", "child_id", "child_title"] } },
                        { "if": { "properties": { "action": { "const": "focus" } } }, "then": { "required": ["action", "node_id"] } },
                        { "if": { "properties": { "action": { "const": "unfocus" } } }, "then": { "required": ["action", "node_id"] } },
                        { "if": { "properties": { "action": { "const": "implement" } } }, "then": { "required": ["action", "node_id"] } },
                        { "if": { "properties": { "action": { "const": "set_priority" } } }, "then": { "required": ["action", "node_id", "priority"] } },
                        { "if": { "properties": { "action": { "const": "set_issue_type" } } }, "then": { "required": ["action", "node_id", "issue_type"] } }
                    ]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::Mutation,
                    omegon_traits::ToolCapability::StateChanging,
                ],
            },
            ToolDefinition {
                name: crate::tool_registry::lifecycle::OPENSPEC_MANAGE.into(),
                label: "openspec_manage".into(),
                description: "Manage OpenSpec changes: list status, get details, propose changes, add specs, register tasks/test files, set task checkbox status, archive.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["status", "get", "propose", "add_spec", "register_tasks", "set_task_status", "register_test_file", "archive"] },
                        "change_name": { "type": "string" },
                        "name": { "type": "string" },
                        "title": { "type": "string" },
                        "intent": { "type": "string" },
                        "domain": { "type": "string" },
                        "spec_content": { "type": "string" },
                        "path": { "type": "string", "description": "Test file path for register_test_file." },
                        "test_file": { "type": "string", "description": "Alias for path." },
                        "group": { "type": "string", "description": "OpenSpec task group title for set_task_status." },
                        "group_title": { "type": "string", "description": "Alias for group." },
                        "task_id": { "type": "string", "description": "Stable numeric task id for set_task_status, such as 2.5." },
                        "status": { "type": "string", "enum": ["done", "pending"], "description": "Target task checkbox status for set_task_status." }
                    },
                    "required": ["action"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::Orientation,
                    omegon_traits::ToolCapability::StateChanging,
                ],
            },
            ToolDefinition {
                name: crate::tool_registry::lifecycle::LIFECYCLE_DOCTOR.into(),
                label: "lifecycle_doctor".into(),
                description: "Audit design-tree state for suspicious lifecycle drift. Returns structured findings the harness can act on directly.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "node_id": { "type": "string" },
                        "kinds": { "type": "array", "items": { "type": "string", "enum": ["implemented_has_open_questions", "resolved_without_questions", "seed_without_questions", "exploring_without_questions", "parent_implemented_with_active_children", "question_appears_answered_by_decision"] } }
                    }
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        if self.lifecycle_binding.is_some() {
            return match tool_name {
                crate::tool_registry::lifecycle::DESIGN_TREE => {
                    self.managed_design_tree(&args, cancel).await
                }
                crate::tool_registry::lifecycle::DESIGN_TREE_UPDATE => {
                    self.managed_design_tree_update(call_id, &args, cancel)
                        .await
                }
                crate::tool_registry::lifecycle::OPENSPEC_MANAGE => {
                    self.managed_openspec(call_id, &args, cancel).await
                }
                crate::tool_registry::lifecycle::LIFECYCLE_DOCTOR => {
                    self.managed_doctor(&args, cancel).await
                }
                _ => anyhow::bail!("Unknown tool: {tool_name}"),
            };
        }
        #[cfg(test)]
        {
            return match tool_name {
                crate::tool_registry::lifecycle::DESIGN_TREE => self.execute_design_tree(&args),
                crate::tool_registry::lifecycle::DESIGN_TREE_UPDATE => {
                    self.execute_design_tree_update(&args)
                }
                crate::tool_registry::lifecycle::OPENSPEC_MANAGE => {
                    self.execute_openspec_manage(&args)
                }
                crate::tool_registry::lifecycle::LIFECYCLE_DOCTOR => {
                    self.execute_lifecycle_doctor(&args)
                }
                _ => anyhow::bail!("Unknown tool: {tool_name}"),
            };
        }
        #[cfg(not(test))]
        {
            let _ = (call_id, args, cancel);
            anyhow::bail!("managed lifecycle service is unavailable")
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![
            CommandDefinition {
                name: "design-focus".into(),
                description: "Pin a design node (inject its context) — use via agent tool, not operator command".into(),
                subcommands: self.observed_design_nodes().keys().cloned().collect(),
            availability: omegon_traits::CommandAvailability::ALL,
            safety: omegon_traits::CommandSafety::STATE_CHANGING,
                surface: Default::default(),
            },
            CommandDefinition {
                name: "design-unfocus".into(),
                description: "Clear design node pin — use via agent tool, not operator command".into(),
                subcommands: vec![],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: omegon_traits::CommandSafety::STATE_CHANGING,
                surface: Default::default(),
            },
            CommandDefinition {
                name: "design".into(),
                description: "Show design tree summary".into(),
                subcommands: vec!["list".into(), "frontier".into(), "ready".into()],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: omegon_traits::CommandSafety::READ_ONLY,
                surface: Default::default(),
            },
        ]
    }

    fn runtime_command_surfaces(
        &self,
        command_name: &str,
    ) -> Option<Vec<omegon_traits::RuntimeSurface>> {
        (command_name == "design").then(|| {
            vec![
                omegon_traits::RuntimeSurface::Tui,
                omegon_traits::RuntimeSurface::Cli,
                omegon_traits::RuntimeSurface::Acp,
                omegon_traits::RuntimeSurface::Ipc,
                omegon_traits::RuntimeSurface::Web,
            ]
        })
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        match name {
            "design-focus" => {
                let id = args.trim();
                if id.is_empty() {
                    if let Some(focused) = self.focused_node_id() {
                        return CommandResult::Display(format!("Currently focused on: {focused}"));
                    }
                    return CommandResult::Display(
                        "No node focused. Usage: design-focus <node-id>".into(),
                    );
                }
                let nodes = self.observed_design_nodes();
                let Some(node) = nodes.get(id) else {
                    return CommandResult::Display(format!("Node '{id}' not found"));
                };
                let display = format!(
                    "Focused → {} {} — {}",
                    node.status.icon(),
                    node.id,
                    node.title
                );
                if let Err(error) = self.set_focus(Some(id.to_string())) {
                    return CommandResult::Display(error.to_string());
                }
                CommandResult::Display(display)
            }

            "design-unfocus" => {
                if let Err(error) = self.set_focus(None) {
                    return CommandResult::Display(error.to_string());
                }
                CommandResult::Display("Design focus cleared".into())
            }

            "design" => {
                let sub = args.trim();
                let focused_node_id = self.focused_node_id();
                let nodes = self.observed_design_nodes();
                if sub == "frontier" || sub.is_empty() && nodes.is_empty() {
                    return CommandResult::Display(format!("{} design nodes", nodes.len()));
                }

                let mut lines = vec![format!("Design tree: {} nodes", nodes.len())];

                // Count by status
                let mut by_status = std::collections::HashMap::new();
                for n in nodes.values() {
                    *by_status.entry(n.status.as_str()).or_insert(0u32) += 1;
                }
                for (status, count) in &by_status {
                    lines.push(format!("  {status}: {count}"));
                }

                // Show focused
                if let Some(focused) = focused_node_id {
                    lines.push(format!("  Focused: {focused}"));
                }

                CommandResult::Display(lines.join("\n"))
            }

            _ => CommandResult::NotHandled,
        }
    }

    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        if self.lifecycle_host.is_some() {
            return self.managed_context();
        }
        #[cfg(test)]
        {
            return self
                .compatibility_provider()
                .lock()
                .unwrap()
                .provide_context(signals);
        }
        #[cfg(not(test))]
        {
            let _ = signals;
            None
        }
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::TurnEnd(_) => {
                self.turn_counter += 1;
                // Refresh every 5 turns to pick up external changes
                if self.turn_counter.is_multiple_of(5) {
                    #[cfg(test)]
                    if let Some(provider) = &self.provider {
                        provider.lock().unwrap().refresh();
                    }
                    if let Some(host) = self.lifecycle_host.clone()
                        && let Ok(runtime) = tokio::runtime::Handle::try_current()
                    {
                        runtime.spawn(async move {
                            if let Err(error) = host
                                .refresh(
                                    crate::lifecycle::read_model::SnapshotOptions::default(),
                                    tokio_util::sync::CancellationToken::new(),
                                )
                                .await
                            {
                                tracing::debug!(%error, "managed lifecycle periodic refresh failed");
                            }
                        });
                    }
                }
                // Drain auto-store facts queued by execute() handlers
                self.pending_memory
                    .lock()
                    .map(|mut q| std::mem::take(&mut *q))
                    .unwrap_or_default()
            }
            BusEvent::SessionEnd { turns, .. } if *turns > 0 => {
                // Export design tree to Codex vault if configured
                if let Some(ref vault_path) = self.codex_vault_path {
                    let export = self
                        .lifecycle_host
                        .as_ref()
                        .and_then(|host| host.observe().ok()?.repository)
                        .map(|repository| {
                            (
                                repository
                                    .design
                                    .nodes
                                    .values()
                                    .cloned()
                                    .collect::<Vec<_>>(),
                                repository.sections.clone(),
                            )
                        });
                    #[cfg(test)]
                    let export = export.or_else(|| {
                        self.provider.as_ref().and_then(|provider| {
                            let provider = provider.lock().ok()?;
                            Some((
                                provider.all_nodes().values().cloned().collect::<Vec<_>>(),
                                provider.sections_cache().clone(),
                            ))
                        })
                    });
                    if let Some((owned_nodes, sections)) = export
                        && !owned_nodes.is_empty()
                    {
                        match crate::lifecycle::codex_export::export_design_tree_to_vault(
                            vault_path,
                            &owned_nodes,
                            &sections,
                        ) {
                            Ok(count) => {
                                tracing::info!(
                                    nodes = count,
                                    vault = %vault_path.display(),
                                    "exported design tree to Codex vault"
                                );
                            }
                            Err(e) => {
                                tracing::warn!("design tree export to vault failed: {e}");
                            }
                        }
                    }
                }
                vec![]
            }
            _ => vec![],
        }
    }
}

fn text_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        details: json!(null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::EventBus;
    use std::fs;
    #[cfg(unix)]
    use std::path::Path;

    fn setup_test_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let docs = repo.join("docs");
        fs::create_dir_all(&docs).unwrap();

        // Create a design node
        let doc = docs.join("test-node.md");
        fs::write(&doc, "---\nid: test-node\ntitle: \"Test Node\"\nstatus: exploring\ntags: [test]\nopen_questions:\n  - \"What about X?\"\ndependencies: []\nrelated: []\n---\n\n# Test Node\n\n## Overview\n\nTest overview.\n").unwrap();

        // Create openspec dir
        let openspec = repo.join("openspec/changes");
        fs::create_dir_all(&openspec).unwrap();

        (dir, repo)
    }

    async fn managed_feature(repo: &std::path::Path) -> (crate::bus::EventBus, LifecycleFeature) {
        let (bus, binding) = crate::lifecycle_service::test_binding(repo.to_path_buf())
            .await
            .unwrap();
        let host = crate::runtime_state::LifecycleHostHandle::new(binding.clone());
        host.refresh(
            crate::lifecycle::read_model::SnapshotOptions::default(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        (bus, LifecycleFeature::managed(repo, binding, host))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_start_does_not_probe_vault() {
        const SCENARIO: &str = "OMEGON_TEST_LIFECYCLE_VAULT_SCENARIO";
        const MARKER: &str = "OMEGON_TEST_VAULT_PROBE_MARKER";
        if let Ok(scenario) = std::env::var(SCENARIO) {
            let (_dir, repo) = setup_test_repo();
            match scenario.as_str() {
                "environment" => {
                    assert!(std::env::var("VAULT_ADDR").is_ok());
                    assert!(!repo.join(".omegon/vault.json").exists());
                }
                "project-config" => {
                    assert!(std::env::var_os("VAULT_ADDR").is_none());
                    fs::create_dir_all(repo.join(".omegon")).unwrap();
                    fs::write(
                        repo.join(".omegon/vault.json"),
                        r#"{"addr":"http://127.0.0.1:1","auth":{"method":"token"}}"#,
                    )
                    .unwrap();
                }
                _ => panic!("unknown Vault test scenario: {scenario}"),
            }
            let (_bus, mut feature) = managed_feature(&repo).await;
            let requests = feature.on_event(&BusEvent::SessionStart {
                cwd: repo,
                session_id: "lifecycle-vault-ownership".into(),
            });
            assert!(
                !PathBuf::from(std::env::var_os(MARKER).unwrap()).exists(),
                "lifecycle SessionStart invoked the Vault CLI ({scenario})"
            );
            assert!(
                requests.is_empty(),
                "lifecycle SessionStart must not emit secret-health policy: {requests:?}"
            );
            return;
        }

        // Re-exec only this test with a private environment: parallel tests and
        // the operator's Vault credentials must never see the fixture PATH.
        for scenario in ["environment", "project-config"] {
            let home = tempfile::tempdir().unwrap();
            let bin = home.path().join("bin");
            fs::create_dir(&bin).unwrap();
            let vault = bin.join("vault");
            std::os::unix::fs::symlink(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lifecycle/vault.sh"),
                &vault,
            )
            .unwrap();
            let marker = home.path().join("vault-invoked");
            // Prove the executable trap works before trusting its absence.
            let probe = std::process::Command::new(&vault)
                .args(["status", "-format=json"])
                .env_clear()
                .env(MARKER, &marker)
                .output()
                .unwrap();
            assert_eq!(probe.status.code(), Some(2));
            assert_eq!(
                fs::read_to_string(&marker).unwrap(),
                "status -format=json\n"
            );
            fs::remove_file(&marker).unwrap();

            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "features::lifecycle::tests::session_start_does_not_probe_vault",
                    "--nocapture",
                ])
                .env_clear()
                .env(SCENARIO, scenario)
                .env(MARKER, &marker)
                .env("HOME", home.path())
                .env("OMEGON_HOME", home.path().join(".omegon"))
                .env("PATH", &bin);
            if scenario == "environment" {
                command.env("VAULT_ADDR", "http://127.0.0.1:1");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "isolated {scenario} test failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!marker.exists(), "Vault CLI invoked for {scenario}");
        }
    }

    #[test]
    fn production_setup_does_not_construct_the_compatibility_lifecycle_owner() {
        let setup = include_str!("../setup.rs");
        assert!(!setup.contains("LifecycleFeature::try_new"));
        assert!(setup.contains("LifecycleFeature::managed"));
    }

    #[test]
    fn production_consumers_do_not_reconstruct_lifecycle_authority() {
        let sources = [
            ("setup", include_str!("../setup.rs")),
            ("context", include_str!("context.rs")),
            ("acp", include_str!("../acp.rs")),
            ("tui", include_str!("../tui/mod.rs")),
            ("web", include_str!("../web/api.rs")),
            ("ipc", include_str!("../ipc/snapshot.rs")),
            ("workflow", include_str!("../workflow.rs")),
            ("work aggregation", include_str!("work_aggregation.rs")),
            ("project rules", include_str!("../project_rules.rs")),
            ("session log", include_str!("session_log.rs")),
            ("check-in", include_str!("../control_runtime.rs")),
            ("prompt", include_str!("../prompt.rs")),
            ("startup", include_str!("../startup.rs")),
        ];
        for (name, source) in sources {
            for forbidden in [
                "LifecycleContextProvider",
                "OpenSpecRepository",
                "JsonFileStore",
                "scan_design_docs",
                "lifecycle::spec::list_changes",
                "doctor::audit_repo",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{name} must not reconstruct lifecycle authority through {forbidden}"
                );
            }
        }

        let tools = include_str!("../tools/mod.rs");
        assert!(tools.contains("#[cfg(test)]\npub fn lifecycle_plan_projection"));
        assert!(tools.contains("#[cfg(test)]\npub fn render_lifecycle_plan_list"));
    }

    #[tokio::test]
    async fn managed_feature_reads_and_mutates_only_through_the_service() {
        let (_dir, repo) = setup_test_repo();
        let (_bus, feature) = managed_feature(&repo).await;

        let list = feature
            .execute(
                crate::tool_registry::lifecycle::DESIGN_TREE,
                "managed-list",
                json!({"action": "list"}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(matches!(
            &list.content[0],
            ContentBlock::Text { text } if text.contains("test-node")
        ));

        let created = feature
            .execute(
                crate::tool_registry::lifecycle::DESIGN_TREE_UPDATE,
                "managed-create",
                json!({
                    "action": "create",
                    "node_id": "managed-node",
                    "title": "Managed Node",
                    "overview": "Managed repository mutation."
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(matches!(
            &created.content[0],
            ContentBlock::Text { text } if text.contains("Created design node 'managed-node'")
        ));
        assert!(repo.join("docs/managed-node.md").is_file());

        let proposed = feature
            .execute(
                crate::tool_registry::lifecycle::OPENSPEC_MANAGE,
                "managed-propose",
                json!({
                    "action": "propose",
                    "name": "managed-change",
                    "title": "Managed Change",
                    "intent": "Prove managed ownership."
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(matches!(
            &proposed.content[0],
            ContentBlock::Text { text } if text.contains("Proposed change 'managed-change'")
        ));
        assert!(
            repo.join("openspec/changes/managed-change/proposal.md")
                .is_file()
                || repo
                    .join("ai/openspec/changes/managed-change/proposal.md")
                    .is_file()
        );
    }

    #[test]
    fn feature_provides_tools() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LifecycleFeature::new(dir.path());
        let tools = feature.tools();
        assert_eq!(tools.len(), 4);
        assert!(tools.iter().any(|t| t.name == "design_tree"));
        assert!(tools.iter().any(|t| t.name == "design_tree_update"));
        assert!(tools.iter().any(|t| t.name == "openspec_manage"));
        assert!(tools.iter().any(|t| t.name == "lifecycle_doctor"));
    }

    #[test]
    fn feature_provides_commands() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LifecycleFeature::new(dir.path());
        let commands = feature.commands();
        assert!(commands.iter().any(|c| c.name == "design-focus"));
        assert!(commands.iter().any(|c| c.name == "design"));
    }

    #[test]
    fn fallible_construction_rejects_corrupt_and_future_state_without_retry() {
        for content in [
            "{not-json}".to_string(),
            serde_json::json!({
                "version": 999,
                "revision": 0,
                "nodes": [],
                "changes": [],
                "milestones": [],
                "audit_log": []
            })
            .to_string(),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let state_dir = dir.path().join("ai/lifecycle");
            fs::create_dir_all(&state_dir).unwrap();
            fs::write(state_dir.join("state.json"), content).unwrap();

            let error = match LifecycleFeature::try_new(dir.path()) {
                Ok(_) => panic!("invalid lifecycle state must fail closed"),
                Err(error) => error,
            };

            assert!(error.to_string().contains("lifecycle state unavailable"));
        }
    }

    #[test]
    fn design_tree_update_schema_requires_create_fields() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LifecycleFeature::new(dir.path());
        let tools = feature.tools();
        let schema = tools
            .iter()
            .find(|t| t.name == "design_tree_update")
            .expect("design_tree_update tool")
            .parameters
            .clone();

        let all_of = schema["allOf"].as_array().expect("allOf array");
        let create_rule = all_of
            .iter()
            .find(|rule| rule["if"]["properties"]["action"]["const"] == "create")
            .expect("create rule");
        let required = create_rule["then"]["required"]
            .as_array()
            .expect("create required array");
        let required: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

        assert!(required.contains(&"node_id"), "create must require node_id");
        assert!(required.contains(&"title"), "create must require title");
        assert!(
            required.contains(&"overview"),
            "create must require overview"
        );
    }

    #[test]
    fn design_tree_update_schema_requires_node_id_for_set_status() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LifecycleFeature::new(dir.path());
        let tools = feature.tools();
        let schema = tools
            .iter()
            .find(|t| t.name == "design_tree_update")
            .expect("design_tree_update tool")
            .parameters
            .clone();

        let all_of = schema["allOf"].as_array().expect("allOf array");
        let set_status_rule = all_of
            .iter()
            .find(|rule| rule["if"]["properties"]["action"]["const"] == "set_status")
            .expect("set_status rule");
        let required = set_status_rule["then"]["required"]
            .as_array()
            .expect("set_status required array");
        let required: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

        assert!(
            required.contains(&"node_id"),
            "set_status must require node_id"
        );
        assert!(
            required.contains(&"status"),
            "set_status must require status"
        );
    }

    fn design_tree_update_schema_requires_node_id_for_archive() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LifecycleFeature::new(dir.path());
        let tools = feature.tools();
        let schema = tools
            .iter()
            .find(|t| t.name == "design_tree_update")
            .expect("design_tree_update tool")
            .parameters
            .clone();

        let all_of = schema["allOf"].as_array().expect("allOf array");
        let archive_rule = all_of
            .iter()
            .find(|rule| rule["if"]["properties"]["action"]["const"] == "archive")
            .expect("archive rule");
        let required = archive_rule["then"]["required"]
            .as_array()
            .expect("archive required array");
        let required: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

        assert!(
            required.contains(&"node_id"),
            "archive must require node_id"
        );
    }

    #[test]
    fn design_tree_list() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        let result = feature
            .execute_design_tree(&json!({"action": "list"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("test-node"), "should list the node: {text}");
    }

    #[test]
    fn design_tree_node() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        let result = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "test-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Test Node"), "should show title: {text}");
        assert!(
            text.contains("What about X"),
            "should show questions: {text}"
        );
    }

    #[test]
    fn design_tree_create() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        let result = feature
            .execute_design_tree_update(&json!({
                "action": "create",
                "node_id": "new-node",
                "title": "New Node",
                "parent": "test-node",
                "tags": ["new"],
            }))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Created"), "{text}");

        // Verify it's readable
        let result = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "new-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("New Node"), "{text}");
        assert!(text.contains("test-node"), "should show parent: {text}");
    }

    #[test]
    fn design_tree_set_status() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // Remove open questions first — FSM requires no open questions for decided
        feature
            .execute_design_tree_update(&json!({
                "action": "remove_question",
                "node_id": "test-node",
                "question": "What about X?",
            }))
            .unwrap();

        feature
            .execute_design_tree_update(&json!({
                "action": "set_status",
                "node_id": "test-node",
                "status": "decided",
            }))
            .unwrap();

        let result = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "test-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("decided"), "should show new status: {text}");
    }

    #[test]
    fn design_tree_add_decision() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        feature
            .execute_design_tree_update(&json!({
                "action": "add_decision",
                "node_id": "test-node",
                "decision_title": "Use approach A",
                "decision_status": "decided",
                "rationale": "Because it's simpler",
            }))
            .unwrap();

        let result = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "test-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("Use approach A"),
            "should show decision: {text}"
        );
    }

    #[test]
    fn design_tree_branch() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        feature
            .execute_design_tree_update(&json!({
                "action": "branch",
                "node_id": "test-node",
                "question": "What about X?",
                "child_id": "child-node",
                "child_title": "Child from question",
            }))
            .unwrap();

        // Child exists
        let result = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "child-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Child from question"), "{text}");

        // Question removed from parent
        let result = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "test-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            !text.contains("What about X"),
            "question should be removed from parent: {text}"
        );
    }

    #[test]
    fn focus_and_unfocus() {
        let (_dir, repo) = setup_test_repo();
        let mut feature = LifecycleFeature::new(&repo);

        let result = feature.handle_command("design-focus", "test-node");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("Focused")));
        assert_eq!(
            feature
                .compatibility_provider()
                .lock()
                .unwrap()
                .focused_node_id()
                .map(String::from),
            Some("test-node".to_string())
        );

        let result = feature.handle_command("design-unfocus", "");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("cleared")));
        assert!(
            feature
                .compatibility_provider()
                .lock()
                .unwrap()
                .focused_node_id()
                .is_none()
        );
    }

    #[test]
    fn lifecycle_doctor_returns_structured_findings() {
        let (_dir, repo) = setup_test_repo();
        fs::write(
            repo.join("docs/stale-node.md"),
            "---\nid: stale-node\ntitle: \"Stale Node\"\nstatus: resolved\ntags: [test]\nopen_questions: []\ndependencies: []\nrelated: []\n---\n\n# Stale Node\n\n## Overview\n\nStale overview.\n",
        )
        .unwrap();
        let feature = LifecycleFeature::new(&repo);

        let result = feature
            .execute_lifecycle_doctor(&json!({"node_id": "stale-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Lifecycle doctor: 1 finding"), "{text}");
        assert_eq!(result.details["total"].as_u64(), Some(1));
        assert_eq!(
            result.details["findings"][0]["node_id"].as_str(),
            Some("stale-node")
        );
        assert_eq!(
            result.details["findings"][0]["kind"].as_str(),
            Some("resolved_without_questions")
        );
    }

    #[tokio::test]
    async fn lifecycle_doctor_is_dispatchable_through_event_bus() {
        let (_dir, repo) = setup_test_repo();
        fs::write(
            repo.join("docs/stale-node.md"),
            "---\nid: stale-node\ntitle: \"Stale Node\"\nstatus: resolved\ntags: [test]\nopen_questions: []\ndependencies: []\nrelated: []\n---\n\n# Stale Node\n\n## Overview\n\nStale overview.\n",
        )
        .unwrap();

        let mut bus = EventBus::new();
        bus.register(Box::new(LifecycleFeature::new(&repo)));
        bus.finalize();

        let result = bus
            .execute_tool(
                crate::tool_registry::lifecycle::LIFECYCLE_DOCTOR,
                "tc1",
                json!({"node_id": "stale-node"}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(result.details["total"].as_u64(), Some(1));
        assert_eq!(
            result.details["findings"][0]["node_id"].as_str(),
            Some("stale-node")
        );
    }

    #[test]
    fn openspec_status_does_not_materialize_discovered_changes() {
        let (_dir, repo) = setup_test_repo();
        let change_dir = repo.join("openspec/changes/discovered");
        fs::create_dir_all(&change_dir).unwrap();
        fs::write(change_dir.join("proposal.md"), "# Discovered\n").unwrap();
        fs::write(change_dir.join("tasks.md"), "- [ ] pending\n").unwrap();
        let feature = LifecycleFeature::new(&repo);

        let result = feature
            .execute_openspec_manage(&json!({"action": "status"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("discovered"),
            "should list file-backed change: {text}"
        );
        assert!(
            !repo.join("ai/lifecycle/state.json").exists(),
            "read-only status must not write lifecycle state"
        );
    }

    #[test]
    fn openspec_propose_and_status() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        feature
            .execute_openspec_manage(&json!({
                "action": "propose",
                "name": "my-change",
                "title": "My Change",
                "intent": "Do the thing",
            }))
            .unwrap();

        let result = feature
            .execute_openspec_manage(&json!({"action": "status"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("my-change"), "should list the change: {text}");
        let changes: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(changes[0]["state"].as_str(), Some("proposed"));
        assert_eq!(changes[0]["stage"].as_str(), Some("proposed"));
    }

    #[test]
    fn openspec_set_task_status_updates_tasks_file() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        let change_dir = repo.join("openspec/changes/task-write");
        fs::create_dir_all(&change_dir).unwrap();
        fs::write(change_dir.join("proposal.md"), "# Task Write\n").unwrap();
        fs::write(
            change_dir.join("tasks.md"),
            "# Tasks\n\n## 1. Runtime\n- [ ] 1.1 Pending task\n",
        )
        .unwrap();

        let result = feature
            .execute_openspec_manage(&json!({
                "action": "set_task_status",
                "change_name": "task-write",
                "group": "1. Runtime",
                "task_id": "1.1",
                "status": "done",
            }))
            .unwrap();

        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("status: pending -> done"), "{text}");
        let content = fs::read_to_string(change_dir.join("tasks.md")).unwrap();
        assert!(content.contains("- [x] 1.1 Pending task"));
    }

    #[test]
    fn openspec_add_spec() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // First propose
        feature
            .execute_openspec_manage(&json!({
                "action": "propose",
                "name": "spec-test",
                "title": "Spec Test",
                "intent": "Test specs",
            }))
            .unwrap();

        // Then add spec
        feature.execute_openspec_manage(&json!({
            "action": "add_spec",
            "change_name": "spec-test",
            "domain": "auth",
            "spec_content": "# auth\n\n### Requirement: Login works\n\n#### Scenario: Valid creds\n\nGiven valid credentials\nWhen login\nThen success\n",
        })).unwrap();

        // Verify via get
        let result = feature
            .execute_openspec_manage(&json!({
                "action": "get",
                "change_name": "spec-test",
            }))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("auth"), "should list spec domain: {text}");
        let change: Value = serde_json::from_str(text).unwrap();
        assert_eq!(change["state"].as_str(), Some("specced"));
        assert_eq!(change["stage"].as_str(), Some("specced"));
        assert_eq!(change["file_stage"].as_str(), Some("specced"));
    }

    #[test]
    fn openspec_status_bootstraps_legacy_change_into_opsx() {
        let (_dir, repo) = setup_test_repo();
        let change_dir = repo.join("openspec/changes/legacy-change");
        fs::create_dir_all(change_dir.join("specs")).unwrap();
        fs::write(change_dir.join("proposal.md"), "# Legacy\n").unwrap();
        fs::write(
            change_dir.join("specs/auth.md"),
            "# auth\n\n### Requirement: Login works\n\n#### Scenario: Valid creds\n\nGiven valid credentials\nWhen login\nThen success\n",
        )
        .unwrap();

        let feature = LifecycleFeature::new(&repo);
        let result = feature
            .execute_openspec_manage(&json!({"action": "status"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        let changes: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(changes[0]["name"].as_str(), Some("legacy-change"));
        assert_eq!(changes[0]["state"].as_str(), Some("specced"));
        assert_eq!(changes[0]["stage"].as_str(), Some("specced"));
        assert_eq!(changes[0]["file_stage"].as_str(), Some("specced"));
    }

    #[test]
    fn lifecycle_doctor_reports_openspec_state_drift() {
        let (_dir, repo) = setup_test_repo();
        let change_dir = repo.join("openspec/changes/drift-change");
        fs::create_dir_all(change_dir.join("specs")).unwrap();
        fs::write(change_dir.join("proposal.md"), "# Drift\n").unwrap();
        fs::write(
            change_dir.join("specs/auth.md"),
            "# auth\n\n### Requirement: Login works\n\n#### Scenario: Valid creds\n\nGiven valid credentials\nWhen login\nThen success\n",
        )
        .unwrap();
        fs::write(change_dir.join("tasks.md"), "- [ ] Wire implementation\n").unwrap();

        let feature = LifecycleFeature::new(&repo);
        feature
            .execute_openspec_manage(&json!({"action": "status"}))
            .unwrap();

        let result = feature
            .execute_lifecycle_doctor(&json!({
                "kinds": ["openspec_state_drift"],
                "node_id": "drift-change",
            }))
            .unwrap();

        assert_eq!(result.details["total"].as_u64(), Some(1));
        assert_eq!(
            result.details["findings"][0]["kind"].as_str(),
            Some("openspec_state_drift")
        );
    }

    fn write_archive_tx_for_test(repo: &std::path::Path, change: &str, phase: &str) {
        let tx = archive::OpenSpecArchiveTransaction {
            version: 1,
            op: "openspec_archive".to_string(),
            change: change.to_string(),
            from_state: "verifying".to_string(),
            to_state: "archived".to_string(),
            change_dir: repo
                .join("openspec/changes")
                .join(change)
                .to_string_lossy()
                .to_string(),
            archive_dir: repo
                .join("openspec/archive")
                .join(change)
                .to_string_lossy()
                .to_string(),
            phase: phase.to_string(),
        };
        archive::write_archive_tx(repo, &tx).unwrap();
    }

    #[test]
    fn archive_recovery_removes_stale_intent_before_content_move() {
        let (_dir, repo) = setup_test_repo();
        let change_dir = repo.join("openspec/changes/pending-archive");
        fs::create_dir_all(&change_dir).unwrap();
        fs::write(change_dir.join("proposal.md"), "# Pending\n").unwrap();
        write_archive_tx_for_test(&repo, "pending-archive", "intent_written");

        let feature = LifecycleFeature::new(&repo);
        let recovered =
            archive::recover_archive_transactions(&repo, feature.compatibility_opsx()).unwrap();

        assert!(recovered[0].contains("removed stale"));
        assert!(change_dir.exists());
        assert!(!archive::archive_tx_path(&repo, "pending-archive").exists());
    }

    #[test]
    fn archive_recovery_completes_content_moved_archive() {
        let (_dir, repo) = setup_test_repo();
        let archive_dir = repo.join("openspec/archive/crash-window");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(archive_dir.join("proposal.md"), "# Crash Window\n").unwrap();
        write_archive_tx_for_test(&repo, "crash-window", "content_moved");

        let feature = LifecycleFeature::new(&repo);
        feature
            .compatibility_opsx()
            .lock()
            .unwrap()
            .create_change("crash-window", "Crash Window", None)
            .unwrap();

        let recovered =
            archive::recover_archive_transactions(&repo, feature.compatibility_opsx()).unwrap();

        assert!(recovered[0].contains("completed interrupted archive"));
        assert!(!archive::archive_tx_path(&repo, "crash-window").exists());
        assert_eq!(
            sync::change_state(
                &feature.compatibility_opsx().lock().unwrap(),
                "crash-window"
            ),
            Some(OpsxChangeState::Archived)
        );
    }

    #[test]
    fn lifecycle_doctor_recovers_content_moved_archive_before_audit() {
        let (_dir, repo) = setup_test_repo();
        let archive_dir = repo.join("openspec/archive/crash-window");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(archive_dir.join("proposal.md"), "# Crash Window\n").unwrap();
        write_archive_tx_for_test(&repo, "crash-window", "content_moved");

        let feature = LifecycleFeature::new(&repo);
        feature
            .compatibility_opsx()
            .lock()
            .unwrap()
            .create_change("crash-window", "Crash Window", None)
            .unwrap();

        let result = feature
            .execute_lifecycle_doctor(&json!({
                "kinds": ["openspec_state_drift"],
                "node_id": "crash-window",
            }))
            .unwrap();

        assert_eq!(result.details["total"].as_u64(), Some(0));
        assert_eq!(result.details["recovered"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn archive_recovery_removes_journal_after_state_already_saved() {
        let (_dir, repo) = setup_test_repo();
        let archive_dir = repo.join("openspec/archive/state-saved");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(archive_dir.join("proposal.md"), "# State Saved\n").unwrap();
        write_archive_tx_for_test(&repo, "state-saved", "content_moved");

        let feature = LifecycleFeature::new(&repo);
        feature
            .compatibility_opsx()
            .lock()
            .unwrap()
            .create_change("state-saved", "State Saved", None)
            .unwrap();
        feature
            .compatibility_opsx()
            .lock()
            .unwrap()
            .force_transition_change("state-saved", OpsxChangeState::Archived, "test setup")
            .unwrap();
        let audit_len_before = feature
            .compatibility_opsx()
            .lock()
            .unwrap()
            .state()
            .audit_log
            .len();

        let recovered =
            archive::recover_archive_transactions(&repo, feature.compatibility_opsx()).unwrap();

        assert!(recovered[0].contains("completed interrupted archive"));
        assert!(!archive::archive_tx_path(&repo, "state-saved").exists());
        assert_eq!(
            feature
                .compatibility_opsx()
                .lock()
                .unwrap()
                .state()
                .audit_log
                .len(),
            audit_len_before
        );
    }

    #[test]
    fn archive_recovery_reports_conflict_when_both_dirs_exist() {
        let (_dir, repo) = setup_test_repo();
        fs::create_dir_all(repo.join("openspec/changes/conflict")).unwrap();
        fs::create_dir_all(repo.join("openspec/archive/conflict")).unwrap();
        write_archive_tx_for_test(&repo, "conflict", "content_moved");

        let feature = LifecycleFeature::new(&repo);
        let err = archive::recover_archive_transactions(&repo, feature.compatibility_opsx())
            .unwrap_err()
            .to_string();

        assert!(err.contains("both"), "unexpected error: {err}");
    }

    #[test]
    fn archive_recovery_reports_conflict_when_neither_dir_exists() {
        let (_dir, repo) = setup_test_repo();
        write_archive_tx_for_test(&repo, "missing", "content_moved");

        let feature = LifecycleFeature::new(&repo);
        let err = archive::recover_archive_transactions(&repo, feature.compatibility_opsx())
            .unwrap_err()
            .to_string();

        assert!(err.contains("neither"), "unexpected error: {err}");
    }

    #[test]
    fn lifecycle_doctor_reports_archived_content_state_drift() {
        let (_dir, repo) = setup_test_repo();
        let archive_dir = repo.join("openspec/archive/crash-window");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(archive_dir.join("proposal.md"), "# Crash Window\n").unwrap();

        let feature = LifecycleFeature::new(&repo);
        feature
            .compatibility_opsx()
            .lock()
            .unwrap()
            .create_change("crash-window", "Crash Window", None)
            .unwrap();

        let result = feature
            .execute_lifecycle_doctor(&json!({
                "kinds": ["openspec_state_drift"],
                "node_id": "crash-window",
            }))
            .unwrap();

        assert_eq!(result.details["total"].as_u64(), Some(1));
        assert!(
            result.details["findings"][0]["detail"]
                .as_str()
                .unwrap()
                .contains("archived on disk")
        );
    }

    #[test]
    fn openspec_full_fsm_path_requires_tests_before_archive() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        feature
            .execute_openspec_manage(&json!({
                "action": "propose",
                "name": "full-change",
                "title": "Full Change",
                "intent": "Exercise the full OpenSpec FSM",
            }))
            .unwrap();
        feature
            .execute_openspec_manage(&json!({
                "action": "add_spec",
                "change_name": "full-change",
                "domain": "core",
                "spec_content": "# core\n\n### Requirement: Flow works\n\n#### Scenario: Valid flow\n\nGiven setup\nWhen run\nThen success\n",
            }))
            .unwrap();
        let tasks_path = repo.join("openspec/changes/full-change/tasks.md");
        fs::write(&tasks_path, "- [ ] Write tests\n- [ ] Implement\n").unwrap();
        feature
            .execute_openspec_manage(&json!({
                "action": "register_tasks",
                "change_name": "full-change",
            }))
            .unwrap();

        let archive_err = feature
            .execute_openspec_manage(&json!({
                "action": "archive",
                "change_name": "full-change",
            }))
            .unwrap_err()
            .to_string();
        assert!(
            archive_err.contains("must be verifying before archive"),
            "unexpected archive error: {archive_err}"
        );

        feature
            .execute_openspec_manage(&json!({
                "action": "register_test_file",
                "change_name": "full-change",
                "path": "core/tests/full_change.rs",
            }))
            .unwrap();
        fs::write(&tasks_path, "- [x] Write tests\n- [x] Implement\n").unwrap();
        feature
            .execute_openspec_manage(&json!({
                "action": "register_tasks",
                "change_name": "full-change",
            }))
            .unwrap();

        let result = feature
            .execute_openspec_manage(&json!({
                "action": "get",
                "change_name": "full-change",
            }))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        let change: Value = serde_json::from_str(text).unwrap();
        assert_eq!(change["state"].as_str(), Some("verifying"));
        assert_eq!(change["stage"].as_str(), Some("verifying"));

        feature
            .execute_openspec_manage(&json!({
                "action": "archive",
                "change_name": "full-change",
            }))
            .unwrap();
        assert!(repo.join("openspec/archive/full-change").exists());
    }

    #[test]
    fn openspec_rejects_test_registration_before_tasks() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        feature
            .execute_openspec_manage(&json!({
                "action": "propose",
                "name": "no-plan",
                "title": "No Plan",
                "intent": "Try to skip planning",
            }))
            .unwrap();
        feature
            .execute_openspec_manage(&json!({
                "action": "add_spec",
                "change_name": "no-plan",
                "domain": "core",
                "spec_content": "# core\n\n### Requirement: Flow works\n\n#### Scenario: Valid flow\n\nGiven setup\nWhen run\nThen success\n",
            }))
            .unwrap();

        let err = feature
            .execute_openspec_manage(&json!({
                "action": "register_test_file",
                "change_name": "no-plan",
                "path": "core/tests/no_plan.rs",
            }))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("must be planned before registering test files"),
            "unexpected test registration error: {err}"
        );
    }

    #[test]
    fn implement_bridges_design_to_openspec() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // Remove open questions first — FSM requires no open questions for decided
        feature
            .execute_design_tree_update(&json!({
                "action": "remove_question",
                "node_id": "test-node",
                "question": "What about X?",
            }))
            .unwrap();

        // Set to decided first
        feature
            .execute_design_tree_update(&json!({
                "action": "set_status",
                "node_id": "test-node",
                "status": "decided",
            }))
            .unwrap();

        // Implement
        let result = feature
            .execute_design_tree_update(&json!({
                "action": "implement",
                "node_id": "test-node",
            }))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Scaffolded"), "{text}");
        assert!(text.contains("implementing"), "{text}");

        // OpenSpec change exists
        let result = feature
            .execute_openspec_manage(&json!({"action": "status"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("test-node"),
            "openspec should have the change: {text}"
        );
    }

    #[test]
    fn archived_nodes_are_hidden_from_active_views() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        feature
            .execute_design_tree_update(&json!({
                "action": "create",
                "node_id": "archive-me",
                "title": "Archive Me",
                "overview": "old work",
            }))
            .unwrap();

        feature
            .execute_design_tree_update(&json!({
                "action": "archive",
                "node_id": "archive-me",
                "archive_reason": "obsolete",
                "superseded_by": "replacement-node"
            }))
            .unwrap();

        let list = feature
            .execute_design_tree(&json!({"action": "list"}))
            .unwrap();
        let text = list.content[0].as_text().unwrap();
        assert!(
            !text.contains("archive-me"),
            "archived node leaked into list: {text}"
        );

        let ready = feature
            .execute_design_tree(&json!({"action": "ready"}))
            .unwrap();
        let ready_text = ready.content[0].as_text().unwrap();
        assert!(
            !ready_text.contains("archive-me"),
            "archived node leaked into ready: {ready_text}"
        );

        let node = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "archive-me"}))
            .unwrap();
        let node_text = node.content[0].as_text().unwrap();
        assert!(
            node_text.contains("\"status\": \"archived\""),
            "{node_text}"
        );
        assert!(
            node_text.contains("\"archive_reason\": \"obsolete\""),
            "{node_text}"
        );
        assert!(
            node_text.contains("\"superseded_by\": \"replacement-node\""),
            "{node_text}"
        );
    }

    #[test]
    fn archive_rejects_non_archived_descendants() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        feature
            .execute_design_tree_update(&json!({
                "action": "create",
                "node_id": "archive-parent",
                "title": "Archive Parent",
                "overview": "parent",
            }))
            .unwrap();

        feature
            .execute_design_tree_update(&json!({
                "action": "create",
                "node_id": "archive-child",
                "title": "Archive Child",
                "overview": "child",
                "parent": "archive-parent",
            }))
            .unwrap();

        let result = feature.execute_design_tree_update(&json!({
            "action": "archive",
            "node_id": "archive-parent",
            "archive_reason": "obsolete"
        }));
        assert!(result.is_err(), "archive should reject active descendants");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("non-archived descendants"), "{err}");
    }

    #[test]
    fn fsm_rejects_invalid_transition() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // test-node starts as "exploring" — try to jump to "implemented"
        let result = feature.execute_design_tree_update(&json!({
            "action": "set_status",
            "node_id": "test-node",
            "status": "implemented",
        }));
        assert!(result.is_err(), "FSM should reject exploring → implemented");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("invalid transition") || err.contains("cannot go from"),
            "error should mention invalid transition: {err}"
        );
    }

    #[test]
    fn fsm_allows_valid_transition() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // exploring → decided (valid, no open questions after removing them)
        feature
            .execute_design_tree_update(&json!({
                "action": "remove_question",
                "node_id": "test-node",
                "question": "What about X?",
            }))
            .unwrap();

        feature
            .execute_design_tree_update(&json!({
                "action": "set_status",
                "node_id": "test-node",
                "status": "decided",
            }))
            .unwrap();

        // Verify status changed
        let result = feature
            .execute_design_tree(&json!({"action": "node", "node_id": "test-node"}))
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("decided"), "should be decided: {text}");
    }

    #[test]
    fn fsm_blocks_decided_with_open_questions() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // test-node has open questions — try to decide
        let result = feature.execute_design_tree_update(&json!({
            "action": "set_status",
            "node_id": "test-node",
            "status": "decided",
        }));
        assert!(
            result.is_err(),
            "FSM should reject decided with open questions"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("open questions"),
            "error should mention open questions: {err}"
        );
    }

    #[test]
    fn create_registers_in_fsm() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        feature
            .execute_design_tree_update(&json!({
                "action": "create",
                "node_id": "fsm-test",
                "title": "FSM Test Node",
            }))
            .unwrap();

        // The node should be in the FSM — trying an invalid transition should fail
        let result = feature.execute_design_tree_update(&json!({
            "action": "set_status",
            "node_id": "fsm-test",
            "status": "implemented",
        }));
        assert!(
            result.is_err(),
            "seed → implemented should be rejected by FSM"
        );
    }
}

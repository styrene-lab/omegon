//! Lifecycle Feature — design-tree + openspec as a unified Feature.
//!
//! Provides:
//! - Tools: `design_tree` (query), `design_tree_update` (mutation),
//!   `openspec_manage` (lifecycle management)
//! - Commands: `/focus`, `/design`, `/unfocus`
//! - Context injection: focused design node + active openspec changes
//! - Event handling: refresh on TurnEnd

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult,
    ContextInjection, ContextProvider, ContextSignals, Feature,
    ToolDefinition, ToolResult, ContentBlock,
};

use crate::lifecycle::context::LifecycleContextProvider;
use crate::lifecycle::{design, spec, types::*};

/// The lifecycle Feature — wraps the LifecycleContextProvider and adds
/// tools + commands for design-tree and openspec operations.
///
/// The provider is behind RefCell because `Feature::execute()` takes `&self`
/// but mutations need `&mut`. The bus guarantees sequential delivery so
/// this is safe in practice.
pub struct LifecycleFeature {
    provider: Arc<Mutex<LifecycleContextProvider>>,
    repo_path: PathBuf,
    /// Counter for refresh throttling — only refresh every N turns.
    turn_counter: u32,
}

impl LifecycleFeature {
    pub fn new(repo_path: &std::path::Path) -> Self {
        let provider = LifecycleContextProvider::new(repo_path);
        Self {
            provider: Arc::new(Mutex::new(provider)),
            repo_path: repo_path.to_path_buf(),
            turn_counter: 0,
        }
    }

    /// Lock the provider for dashboard state extraction.
    pub fn provider(&self) -> std::sync::MutexGuard<'_, LifecycleContextProvider> {
        self.provider.lock().unwrap()
    }

    /// Get a shared handle to the provider for live dashboard updates.
    pub fn shared_provider(&self) -> Arc<Mutex<LifecycleContextProvider>> {
        Arc::clone(&self.provider)
    }

    // ── Tool dispatch ───────────────────────────────────────────────────

    fn execute_design_tree(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");
        let node_id = args["node_id"].as_str();
        let p = self.provider.lock().unwrap();

        match action {
            "list" => {
                let nodes = p.all_nodes();
                let list: Vec<Value> = nodes.values().map(|n| {
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
                }).collect();
                Ok(text_result(&serde_json::to_string_pretty(&list)?))
            }

            "node" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let node = p.get_node(id)
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
                    "children": children.iter().map(|c| json!({
                        "id": c.id,
                        "title": c.title,
                        "status": c.status.as_str(),
                    })).collect::<Vec<_>>(),
                });

                if let Some(ref s) = sections {
                    result["overview"] = json!(s.overview);
                    result["research"] = json!(s.research.iter().map(|r| json!({
                        "heading": r.heading,
                        "content": r.content,
                    })).collect::<Vec<_>>());
                    result["decisions"] = json!(s.decisions.iter().map(|d| json!({
                        "title": d.title,
                        "status": d.status,
                        "rationale": d.rationale,
                    })).collect::<Vec<_>>());
                    result["impl_file_scope"] = json!(s.impl_file_scope.iter().map(|f| json!({
                        "path": f.path,
                        "description": f.description,
                        "action": f.action,
                    })).collect::<Vec<_>>());
                    result["impl_constraints"] = json!(s.impl_constraints);
                }

                Ok(text_result(&serde_json::to_string_pretty(&result)?))
            }

            "frontier" => {
                let nodes = p.all_nodes();
                let frontier: Vec<Value> = nodes.values()
                    .filter(|n| !n.open_questions.is_empty())
                    .map(|n| json!({
                        "id": n.id,
                        "title": n.title,
                        "status": n.status.as_str(),
                        "open_questions": n.open_questions,
                    }))
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&frontier)?))
            }

            "children" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let children = design::get_children(p.all_nodes(), id);
                let list: Vec<Value> = children.iter().map(|c| json!({
                    "id": c.id,
                    "title": c.title,
                    "status": c.status.as_str(),
                })).collect();
                Ok(text_result(&serde_json::to_string_pretty(&list)?))
            }

            "dependencies" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let node = p.get_node(id)
                    .ok_or_else(|| anyhow::anyhow!("Node '{id}' not found"))?;
                let deps: Vec<Value> = node.dependencies.iter().filter_map(|dep_id| {
                    p.get_node(dep_id).map(|d| json!({
                        "id": d.id,
                        "title": d.title,
                        "status": d.status.as_str(),
                    }))
                }).collect();
                Ok(text_result(&serde_json::to_string_pretty(&deps)?))
            }

            "ready" => {
                let nodes = p.all_nodes();
                let ready: Vec<Value> = nodes.values()
                    .filter(|n| matches!(n.status, NodeStatus::Decided))
                    .filter(|n| n.dependencies.iter().all(|dep_id| {
                        nodes.get(dep_id).is_some_and(|d| matches!(d.status, NodeStatus::Implemented))
                    }))
                    .map(|n| json!({
                        "id": n.id,
                        "title": n.title,
                        "priority": n.priority,
                    }))
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&ready)?))
            }

            "blocked" => {
                let nodes = p.all_nodes();
                let blocked: Vec<Value> = nodes.values()
                    .filter(|n| {
                        matches!(n.status, NodeStatus::Blocked)
                            || n.dependencies.iter().any(|dep_id| {
                                nodes.get(dep_id).is_none_or(|d| !matches!(d.status, NodeStatus::Implemented))
                            })
                    })
                    .map(|n| {
                        let blockers: Vec<String> = n.dependencies.iter()
                            .filter(|dep_id| {
                                nodes.get(*dep_id).is_none_or(|d| !matches!(d.status, NodeStatus::Implemented))
                            })
                            .cloned()
                            .collect();
                        json!({
                            "id": n.id,
                            "title": n.title,
                            "status": n.status.as_str(),
                            "blocked_by": blockers,
                        })
                    })
                    .collect();
                Ok(text_result(&serde_json::to_string_pretty(&blocked)?))
            }

            _ => anyhow::bail!("Unknown action: {action}. Valid: list, node, frontier, children, dependencies, ready, blocked"),
        }
    }

    fn execute_design_tree_update(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");
        let node_id = args["node_id"].as_str();
        let docs_dir = self.repo_path.join("docs");
        // Helper macro-like pattern: read node data, drop borrow, then mutate
        let get_node_clone = |id: &str| -> anyhow::Result<DesignNode> {
            let p = self.provider.lock().unwrap();
            p.get_node(id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Node '{id}' not found"))
        };

        match action {
            "create" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let title = args["title"].as_str().ok_or_else(|| anyhow::anyhow!("title required"))?;
                let parent = args["parent"].as_str();
                let status = args["status"].as_str();
                let tags: Vec<String> = args["tags"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let overview = args["overview"].as_str().unwrap_or("");

                let node = design::create_node(&docs_dir, id, title, parent, status, &tags, overview)?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Created design node '{id}' at {}", node.file_path.display())))
            }

            "set_status" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let status_str = args["status"].as_str().ok_or_else(|| anyhow::anyhow!("status required"))?;
                let status = NodeStatus::parse(status_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid status: {status_str}"))?;

                let mut node = get_node_clone(id)?;
                design::update_node(&mut node, |n| { n.status = status; })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Set '{id}' status to {status_str}")))
            }

            "add_question" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let question = args["question"].as_str().ok_or_else(|| anyhow::anyhow!("question required"))?;

                let mut node = get_node_clone(id)?;
                design::update_node(&mut node, |n| {
                    n.open_questions.push(question.to_string());
                })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Added question to '{id}'")))
            }

            "remove_question" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let question = args["question"].as_str().ok_or_else(|| anyhow::anyhow!("question required"))?;

                let mut node = get_node_clone(id)?;
                design::update_node(&mut node, |n| {
                    n.open_questions.retain(|q| q != question);
                })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Removed question from '{id}'")))
            }

            "add_research" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let heading = args["heading"].as_str().ok_or_else(|| anyhow::anyhow!("heading required"))?;
                let content = args["content"].as_str().ok_or_else(|| anyhow::anyhow!("content required"))?;

                let node = get_node_clone(id)?;
                let node = &node;
                design::add_research(node, heading, content)?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Added research '{heading}' to '{id}'")))
            }

            "add_decision" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let title = args["decision_title"].as_str().ok_or_else(|| anyhow::anyhow!("decision_title required"))?;
                let status = args["decision_status"].as_str().unwrap_or("exploring");
                let rationale = args["rationale"].as_str().unwrap_or("");

                let node = get_node_clone(id)?;
                let node = &node;
                design::add_decision(node, title, status, rationale)?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Added decision '{title}' to '{id}'")))
            }

            "add_dependency" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let target = args["target_id"].as_str().ok_or_else(|| anyhow::anyhow!("target_id required"))?;

                let mut node = get_node_clone(id)?;
                design::update_node(&mut node, |n| {
                    if !n.dependencies.contains(&target.to_string()) {
                        n.dependencies.push(target.to_string());
                    }
                })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Added dependency '{id}' → '{target}'")))
            }

            "add_related" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let target = args["target_id"].as_str().ok_or_else(|| anyhow::anyhow!("target_id required"))?;

                let mut node = get_node_clone(id)?;
                design::update_node(&mut node, |n| {
                    if !n.related.contains(&target.to_string()) {
                        n.related.push(target.to_string());
                    }
                })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Added related '{id}' ↔ '{target}'")))
            }

            "add_impl_notes" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let node = get_node_clone(id)?;
                let node = &node;

                let file_scope: Vec<FileScope> = args["file_scope"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| {
                        Some(FileScope {
                            path: v["path"].as_str()?.to_string(),
                            description: v["description"].as_str().unwrap_or("").to_string(),
                            action: v["action"].as_str().map(String::from),
                        })
                    }).collect())
                    .unwrap_or_default();

                let constraints: Vec<String> = args["constraints"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                design::add_impl_notes(node, &file_scope, &constraints)?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Added implementation notes to '{id}'")))
            }

            "branch" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let question = args["question"].as_str().ok_or_else(|| anyhow::anyhow!("question required"))?;
                let child_id = args["child_id"].as_str().ok_or_else(|| anyhow::anyhow!("child_id required"))?;
                let child_title = args["child_title"].as_str().unwrap_or(question);

                // Create child node
                design::create_node(&docs_dir, child_id, child_title, Some(id), None, &[], "")?;

                // Remove question from parent
                let mut parent_node = get_node_clone(id)?;
                design::update_node(&mut parent_node, |n| {
                    n.open_questions.retain(|q| q != question);
                })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Branched '{child_id}' from '{id}', removed question")))
            }

            "focus" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                if self.provider.lock().unwrap().get_node(id).is_none() {
                    anyhow::bail!("Node '{id}' not found");
                }
                self.provider.lock().unwrap().set_focus(Some(id.to_string()));
                Ok(text_result(&format!("Focused on design node '{id}'")))
            }

            "unfocus" => {
                self.provider.lock().unwrap().set_focus(None);
                Ok(text_result("Cleared design focus"))
            }

            "implement" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let mut node = get_node_clone(id)?;
                if !matches!(node.status, NodeStatus::Decided) {
                    anyhow::bail!("Node '{id}' must be in 'decided' status to implement (current: {})", node.status.as_str());
                }

                // Scaffold an OpenSpec change from the design node
                let change_name = id;
                let title = node.title.clone();
                let sections = design::read_node_sections(&node);
                let intent = sections.as_ref()
                    .map(|s| s.overview.clone())
                    .unwrap_or_else(|| format!("Implement {title}"));

                let change = spec::propose_change(&self.repo_path, change_name, &title, &intent)?;

                // Update the node to reference the change
                design::update_node(&mut node, |n| {
                    n.openspec_change = Some(change_name.to_string());
                    n.status = NodeStatus::Implementing;
                })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!(
                    "Scaffolded OpenSpec change '{change_name}' at {}\nNode '{id}' → implementing",
                    change.path.display()
                )))
            }

            "set_priority" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let priority = args["priority"].as_u64()
                    .ok_or_else(|| anyhow::anyhow!("priority required (1-5)"))?;
                if !(1..=5).contains(&priority) {
                    anyhow::bail!("Priority must be 1-5, got {priority}");
                }

                let mut node = get_node_clone(id)?;
                design::update_node(&mut node, |n| { n.priority = Some(priority as u8); })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Set '{id}' priority to {priority}")))
            }

            "set_issue_type" => {
                let id = node_id.ok_or_else(|| anyhow::anyhow!("node_id required"))?;
                let type_str = args["issue_type"].as_str().ok_or_else(|| anyhow::anyhow!("issue_type required"))?;
                let issue_type = IssueType::parse(type_str)
                    .ok_or_else(|| anyhow::anyhow!("Invalid issue_type: {type_str}"))?;

                let mut node = get_node_clone(id)?;
                design::update_node(&mut node, |n| { n.issue_type = Some(issue_type); })?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Set '{id}' issue_type to {type_str}")))
            }

            _ => anyhow::bail!("Unknown action: {action}"),
        }
    }

    fn execute_openspec_manage(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let action = args["action"].as_str().unwrap_or("");

        match action {
            "status" => {
                self.provider.lock().unwrap().refresh();
                let p = self.provider.lock().unwrap();
                let changes = p.changes();
                if changes.is_empty() {
                    return Ok(text_result("No active OpenSpec changes."));
                }
                let list: Vec<Value> = changes.iter().map(|c| json!({
                    "name": c.name,
                    "stage": c.stage.as_str(),
                    "has_proposal": c.has_proposal,
                    "has_specs": c.has_specs,
                    "has_tasks": c.has_tasks,
                    "total_tasks": c.total_tasks,
                    "done_tasks": c.done_tasks,
                })).collect();
                Ok(text_result(&serde_json::to_string_pretty(&list)?))
            }

            "get" => {
                let name = args["change_name"].as_str().ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                let change = spec::get_change(&self.repo_path, name)
                    .ok_or_else(|| anyhow::anyhow!("Change '{name}' not found"))?;

                let result = json!({
                    "name": change.name,
                    "stage": change.stage.as_str(),
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
                let name = args["name"].as_str()
                    .or_else(|| args["change_name"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("name required"))?;
                let title = args["title"].as_str().ok_or_else(|| anyhow::anyhow!("title required"))?;
                let intent = args["intent"].as_str().ok_or_else(|| anyhow::anyhow!("intent required"))?;

                let change = spec::propose_change(&self.repo_path, name, title, intent)?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Proposed change '{name}' at {}", change.path.display())))
            }

            "add_spec" => {
                let name = args["change_name"].as_str().ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                let domain = args["domain"].as_str().ok_or_else(|| anyhow::anyhow!("domain required"))?;
                let content = args["spec_content"].as_str().ok_or_else(|| anyhow::anyhow!("spec_content required"))?;

                let path = spec::add_spec(&self.repo_path, name, domain, content)?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Added spec '{domain}' to '{name}' at {}", path.display())))
            }

            "archive" => {
                let name = args["change_name"].as_str().ok_or_else(|| anyhow::anyhow!("change_name required"))?;
                spec::archive_change(&self.repo_path, name)?;
                self.provider.lock().unwrap().refresh();
                Ok(text_result(&format!("Archived change '{name}'")))
            }

            _ => anyhow::bail!("Unknown action: {action}. Valid: status, get, propose, add_spec, archive"),
        }
    }
}

#[async_trait]
impl Feature for LifecycleFeature {
    fn name(&self) -> &str {
        "lifecycle"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "design_tree".into(),
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
            },
            ToolDefinition {
                name: "design_tree_update".into(),
                label: "design_tree_update".into(),
                description: "Mutate the design tree: create nodes, change status, add questions/research/decisions, branch, set focus, implement.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["create", "set_status", "add_question", "remove_question", "add_research", "add_decision", "add_dependency", "add_related", "add_impl_notes", "branch", "focus", "unfocus", "implement", "set_priority", "set_issue_type"]
                        },
                        "node_id": { "type": "string", "description": "Target node ID" },
                        "title": { "type": "string" },
                        "parent": { "type": "string" },
                        "status": { "type": "string" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "overview": { "type": "string" },
                        "question": { "type": "string" },
                        "heading": { "type": "string" },
                        "content": { "type": "string" },
                        "decision_title": { "type": "string" },
                        "decision_status": { "type": "string" },
                        "rationale": { "type": "string" },
                        "target_id": { "type": "string" },
                        "child_id": { "type": "string" },
                        "child_title": { "type": "string" },
                        "file_scope": { "type": "array", "items": { "type": "object", "properties": { "path": { "type": "string" }, "description": { "type": "string" }, "action": { "type": "string" } }, "required": ["path", "description"] } },
                        "constraints": { "type": "array", "items": { "type": "string" } },
                        "priority": { "type": "number" },
                        "issue_type": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
            ToolDefinition {
                name: "openspec_manage".into(),
                label: "openspec_manage".into(),
                description: "Manage OpenSpec changes: list status, get details, propose changes, add specs, archive.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["status", "get", "propose", "add_spec", "archive"],
                            "description": "Lifecycle action"
                        },
                        "change_name": { "type": "string" },
                        "name": { "type": "string", "description": "Change name for propose" },
                        "title": { "type": "string", "description": "Change title for propose" },
                        "intent": { "type": "string", "description": "Change intent for propose" },
                        "domain": { "type": "string", "description": "Spec domain (for add_spec)" },
                        "spec_content": { "type": "string", "description": "Spec markdown (for add_spec)" }
                    },
                    "required": ["action"]
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            "design_tree" => self.execute_design_tree(&args),
            "design_tree_update" => self.execute_design_tree_update(&args),
            "openspec_manage" => self.execute_openspec_manage(&args),
            _ => anyhow::bail!("Unknown tool: {tool_name}"),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![
            CommandDefinition {
                name: "focus".into(),
                description: "Focus on a design node (inject its context)".into(),
                subcommands: self.provider.lock().unwrap().all_nodes().keys().cloned().collect(),
            },
            CommandDefinition {
                name: "unfocus".into(),
                description: "Clear design node focus".into(),
                subcommands: vec![],
            },
            CommandDefinition {
                name: "design".into(),
                description: "Show design tree summary".into(),
                subcommands: vec!["list".into(), "frontier".into(), "ready".into()],
            },
        ]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        match name {
            "focus" => {
                let id = args.trim();
                if id.is_empty() {
                    let p = self.provider.lock().unwrap();
                    if let Some(focused) = p.focused_node_id() {
                        return CommandResult::Display(format!("Currently focused on: {focused}"));
                    }
                    return CommandResult::Display("No node focused. Usage: /focus <node-id>".into());
                }
                let display = {
                    let p = self.provider.lock().unwrap();
                    let Some(node) = p.get_node(id) else {
                        return CommandResult::Display(format!("Node '{id}' not found"));
                    };
                    format!("Focused → {} {} — {}", node.status.icon(), node.id, node.title)
                };
                self.provider.lock().unwrap().set_focus(Some(id.to_string()));
                CommandResult::Display(display)
            }

            "unfocus" => {
                self.provider.lock().unwrap().set_focus(None);
                CommandResult::Display("Design focus cleared".into())
            }

            "design" => {
                let sub = args.trim();
                let p = self.provider.lock().unwrap();
                let nodes = p.all_nodes();
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
                if let Some(focused) = p.focused_node_id() {
                    lines.push(format!("  Focused: {focused}"));
                }

                CommandResult::Display(lines.join("\n"))
            }

            _ => CommandResult::NotHandled,
        }
    }

    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        self.provider.lock().unwrap().provide_context(signals)
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::SessionStart { .. } => {
                // Check Vault health if configured — with a short timeout
                // to avoid blocking the event loop.
                let mut requests = vec![];
                if std::env::var("VAULT_ADDR").is_ok() || self.repo_path.join(".omegon/vault.json").exists() {
                    match std::process::Command::new("vault")
                        .args(["status", "-format=json"])
                        .env("VAULT_CLIENT_TIMEOUT", "5")
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn()
                        .and_then(|child| child.wait_with_output())
                    {
                        Ok(out) => {
                            let body = String::from_utf8_lossy(&out.stdout);
                            let sealed = serde_json::from_str::<Value>(&body)
                                .ok()
                                .and_then(|v| v["sealed"].as_bool())
                                .unwrap_or(true);
                            if sealed {
                                requests.push(BusRequest::Notify {
                                    message: "Vault is sealed — secrets from Vault unavailable. Use /vault unseal".into(),
                                    level: omegon_traits::NotifyLevel::Warning,
                                });
                            }
                        }
                        Err(_) => {
                            // vault CLI not available or unreachable — silent skip
                        }
                    }
                }
                requests
            }
            BusEvent::TurnEnd { .. } => {
                self.turn_counter += 1;
                // Refresh every 5 turns to pick up external changes
                if self.turn_counter.is_multiple_of(5) {
                    self.provider.lock().unwrap().refresh();
                }
                vec![]
            }
            _ => vec![],
        }
    }
}

fn text_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text { text: text.to_string() }],
        details: json!(null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    #[test]
    fn feature_provides_tools() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LifecycleFeature::new(dir.path());
        let tools = feature.tools();
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "design_tree"));
        assert!(tools.iter().any(|t| t.name == "design_tree_update"));
        assert!(tools.iter().any(|t| t.name == "openspec_manage"));
    }

    #[test]
    fn feature_provides_commands() {
        let dir = tempfile::tempdir().unwrap();
        let feature = LifecycleFeature::new(dir.path());
        let commands = feature.commands();
        assert!(commands.iter().any(|c| c.name == "focus"));
        assert!(commands.iter().any(|c| c.name == "design"));
    }

    #[test]
    fn design_tree_list() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        let result = feature.execute_design_tree(&json!({"action": "list"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("test-node"), "should list the node: {text}");
    }

    #[test]
    fn design_tree_node() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        let result = feature.execute_design_tree(&json!({"action": "node", "node_id": "test-node"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Test Node"), "should show title: {text}");
        assert!(text.contains("What about X"), "should show questions: {text}");
    }

    #[test]
    fn design_tree_create() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        let result = feature.execute_design_tree_update(&json!({
            "action": "create",
            "node_id": "new-node",
            "title": "New Node",
            "parent": "test-node",
            "tags": ["new"],
        })).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Created"), "{text}");

        // Verify it's readable
        let result = feature.execute_design_tree(&json!({"action": "node", "node_id": "new-node"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("New Node"), "{text}");
        assert!(text.contains("test-node"), "should show parent: {text}");
    }

    #[test]
    fn design_tree_set_status() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        feature.execute_design_tree_update(&json!({
            "action": "set_status",
            "node_id": "test-node",
            "status": "decided",
        })).unwrap();

        let result = feature.execute_design_tree(&json!({"action": "node", "node_id": "test-node"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("decided"), "should show new status: {text}");
    }

    #[test]
    fn design_tree_add_decision() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        feature.execute_design_tree_update(&json!({
            "action": "add_decision",
            "node_id": "test-node",
            "decision_title": "Use approach A",
            "decision_status": "decided",
            "rationale": "Because it's simpler",
        })).unwrap();

        let result = feature.execute_design_tree(&json!({"action": "node", "node_id": "test-node"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Use approach A"), "should show decision: {text}");
    }

    #[test]
    fn design_tree_branch() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);
        feature.execute_design_tree_update(&json!({
            "action": "branch",
            "node_id": "test-node",
            "question": "What about X?",
            "child_id": "child-node",
            "child_title": "Child from question",
        })).unwrap();

        // Child exists
        let result = feature.execute_design_tree(&json!({"action": "node", "node_id": "child-node"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Child from question"), "{text}");

        // Question removed from parent
        let result = feature.execute_design_tree(&json!({"action": "node", "node_id": "test-node"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(!text.contains("What about X"), "question should be removed from parent: {text}");
    }

    #[test]
    fn focus_and_unfocus() {
        let (_dir, repo) = setup_test_repo();
        let mut feature = LifecycleFeature::new(&repo);

        let result = feature.handle_command("focus", "test-node");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("Focused")));
        assert_eq!(feature.provider.lock().unwrap().focused_node_id().map(String::from), Some("test-node".to_string()));

        let result = feature.handle_command("unfocus", "");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("cleared")));
        assert!(feature.provider.lock().unwrap().focused_node_id().is_none());
    }

    #[test]
    fn openspec_propose_and_status() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        feature.execute_openspec_manage(&json!({
            "action": "propose",
            "name": "my-change",
            "title": "My Change",
            "intent": "Do the thing",
        })).unwrap();

        let result = feature.execute_openspec_manage(&json!({"action": "status"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("my-change"), "should list the change: {text}");
    }

    #[test]
    fn openspec_add_spec() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // First propose
        feature.execute_openspec_manage(&json!({
            "action": "propose",
            "name": "spec-test",
            "title": "Spec Test",
            "intent": "Test specs",
        })).unwrap();

        // Then add spec
        feature.execute_openspec_manage(&json!({
            "action": "add_spec",
            "change_name": "spec-test",
            "domain": "auth",
            "spec_content": "# auth\n\n### Requirement: Login works\n\n#### Scenario: Valid creds\n\nGiven valid credentials\nWhen login\nThen success\n",
        })).unwrap();

        // Verify via get
        let result = feature.execute_openspec_manage(&json!({
            "action": "get",
            "change_name": "spec-test",
        })).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("auth"), "should list spec domain: {text}");
    }

    #[test]
    fn implement_bridges_design_to_openspec() {
        let (_dir, repo) = setup_test_repo();
        let feature = LifecycleFeature::new(&repo);

        // Set to decided first
        feature.execute_design_tree_update(&json!({
            "action": "set_status",
            "node_id": "test-node",
            "status": "decided",
        })).unwrap();

        // Implement
        let result = feature.execute_design_tree_update(&json!({
            "action": "implement",
            "node_id": "test-node",
        })).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Scaffolded"), "{text}");
        assert!(text.contains("implementing"), "{text}");

        // OpenSpec change exists
        let result = feature.execute_openspec_manage(&json!({"action": "status"})).unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("test-node"), "openspec should have the change: {text}");
    }
}

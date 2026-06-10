//! Lifecycle mutation service.
//!
//! Tool adapters parse provider-specific JSON and render `ToolResult`s. This
//! service owns small, testable lifecycle mutations and their backing-store
//! coordination.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use omegon_opsx::{JsonFileStore, Lifecycle as OpsxLifecycle, NodeState};

use super::context::LifecycleContextProvider;
use super::design;
use super::types::{DesignNode, FileScope, NodeStatus};

#[derive(Clone)]
pub struct LifecycleMutationService {
    repo_path: PathBuf,
    provider: Arc<Mutex<LifecycleContextProvider>>,
    opsx: Arc<Mutex<OpsxLifecycle<JsonFileStore>>>,
}

#[derive(Debug, Clone)]
pub struct CreateDesignNodeRequest {
    pub id: String,
    pub title: String,
    pub parent: Option<String>,
    pub status: Option<String>,
    pub tags: Vec<String>,
    pub overview: String,
}

#[derive(Debug, Clone)]
pub struct SetDesignNodeStatusRequest {
    pub id: String,
    pub status: NodeStatus,
    pub archive_reason: Option<String>,
    pub superseded_by: Option<String>,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SetDesignNodeStatusResult {
    pub node_id: String,
    pub node_title: String,
    pub status: NodeStatus,
}

#[derive(Debug, Clone)]
pub struct UpdateDesignNodeQuestionRequest {
    pub id: String,
    pub question: String,
}

#[derive(Debug, Clone)]
pub struct AddDesignNodeResearchRequest {
    pub id: String,
    pub heading: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AddDesignNodeDecisionRequest {
    pub id: String,
    pub title: String,
    pub status: String,
    pub rationale: String,
}

#[derive(Debug, Clone)]
pub struct AddDesignNodeLinkRequest {
    pub id: String,
    pub target_id: String,
}

#[derive(Debug, Clone)]
pub struct AddDesignNodeImplNotesRequest {
    pub id: String,
    pub file_scope: Vec<FileScope>,
    pub constraints: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BranchDesignNodeQuestionRequest {
    pub parent_id: String,
    pub question: String,
    pub child_id: String,
    pub child_title: String,
}

impl LifecycleMutationService {
    pub fn new(
        repo_path: PathBuf,
        provider: Arc<Mutex<LifecycleContextProvider>>,
        opsx: Arc<Mutex<OpsxLifecycle<JsonFileStore>>>,
    ) -> Self {
        Self {
            repo_path,
            provider,
            opsx,
        }
    }

    pub fn create_design_node(
        &self,
        req: CreateDesignNodeRequest,
    ) -> anyhow::Result<DesignNode> {
        {
            let mut opsx = self.opsx.lock().unwrap();
            // Parent validation is advisory here because markdown parent
            // references are not yet enforced by omegon-opsx.
            let _ = opsx.create_node(&req.id, &req.title, None);
            if let Some(status_str) = req.status.as_deref()
                && let Some(target) = NodeState::parse(status_str)
                && target != NodeState::Seed
            {
                let _ = opsx.force_transition_node(
                    &req.id,
                    target,
                    "initial status on create",
                );
            }
        }

        let docs_dir = self.repo_path.join("docs");
        let node = design::create_node(
            &docs_dir,
            &req.id,
            &req.title,
            req.parent.as_deref(),
            req.status.as_deref(),
            &req.tags,
            &req.overview,
        )?;
        self.provider.lock().unwrap().refresh();
        Ok(node)
    }
    pub fn set_design_node_status(
        &self,
        req: SetDesignNodeStatusRequest,
    ) -> anyhow::Result<SetDesignNodeStatusResult> {
        let mut node = self.get_node_clone(&req.id)?;
        let node_title = node.title.clone();
        let opsx_target = NodeState::parse(req.status.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid status for FSM: {}", req.status.as_str()))?;

        {
            let mut opsx = self.opsx.lock().unwrap();
            if opsx.get_node(&req.id).is_none() {
                bootstrap_node_to_opsx(&mut opsx, &node);
            }
            opsx.transition_node(&req.id, opsx_target)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }

        design::update_node(&mut node, |n| {
            n.status = req.status;
            if matches!(req.status, NodeStatus::Archived) {
                n.archive_reason = req.archive_reason.clone();
                n.superseded_by = req.superseded_by.clone();
                n.archived_at = req.archived_at.clone();
            } else {
                n.archive_reason = None;
                n.superseded_by = None;
                n.archived_at = None;
            }
        })?;
        self.provider.lock().unwrap().refresh();

        Ok(SetDesignNodeStatusResult {
            node_id: req.id,
            node_title,
            status: req.status,
        })
    }

    pub fn add_design_node_question(
        &self,
        req: UpdateDesignNodeQuestionRequest,
    ) -> anyhow::Result<()> {
        let mut node = self.get_node_clone(&req.id)?;
        design::update_node(&mut node, |n| {
            n.open_questions.push(req.question.clone());
        })?;
        self.provider.lock().unwrap().refresh();
        Ok(())
    }

    pub fn remove_design_node_question(
        &self,
        req: UpdateDesignNodeQuestionRequest,
    ) -> anyhow::Result<()> {
        let mut node = self.get_node_clone(&req.id)?;
        design::update_node(&mut node, |n| {
            n.open_questions.retain(|q| q != &req.question);
        })?;
        self.provider.lock().unwrap().refresh();
        Ok(())
    }

    pub fn add_design_node_research(
        &self,
        req: AddDesignNodeResearchRequest,
    ) -> anyhow::Result<()> {
        let node = self.get_node_clone(&req.id)?;
        design::add_research(&node, &req.heading, &req.content)?;
        self.provider.lock().unwrap().refresh();
        Ok(())
    }

    pub fn add_design_node_decision(
        &self,
        req: AddDesignNodeDecisionRequest,
    ) -> anyhow::Result<()> {
        let node = self.get_node_clone(&req.id)?;
        design::add_decision(&node, &req.title, &req.status, &req.rationale)?;
        self.provider.lock().unwrap().refresh();
        Ok(())
    }

    pub fn add_design_node_dependency(
        &self,
        req: AddDesignNodeLinkRequest,
    ) -> anyhow::Result<()> {
        let mut node = self.get_node_clone(&req.id)?;
        design::update_node(&mut node, |n| {
            if !n.dependencies.contains(&req.target_id) {
                n.dependencies.push(req.target_id.clone());
            }
        })?;
        self.provider.lock().unwrap().refresh();
        Ok(())
    }

    pub fn add_design_node_related(
        &self,
        req: AddDesignNodeLinkRequest,
    ) -> anyhow::Result<()> {
        let mut node = self.get_node_clone(&req.id)?;
        design::update_node(&mut node, |n| {
            if !n.related.contains(&req.target_id) {
                n.related.push(req.target_id.clone());
            }
        })?;
        self.provider.lock().unwrap().refresh();
        Ok(())
    }

    pub fn add_design_node_impl_notes(
        &self,
        req: AddDesignNodeImplNotesRequest,
    ) -> anyhow::Result<()> {
        let node = self.get_node_clone(&req.id)?;
        design::add_impl_notes(&node, &req.file_scope, &req.constraints)?;
        self.provider.lock().unwrap().refresh();
        Ok(())
    }

    pub fn branch_design_node_question(
        &self,
        req: BranchDesignNodeQuestionRequest,
    ) -> anyhow::Result<DesignNode> {
        let docs_dir = self.repo_path.join("docs");
        let child = design::create_node(
            &docs_dir,
            &req.child_id,
            &req.child_title,
            Some(&req.parent_id),
            None,
            &[],
            "",
        )?;

        let mut parent_node = self.get_node_clone(&req.parent_id)?;
        design::update_node(&mut parent_node, |n| {
            n.open_questions.retain(|q| q != &req.question);
        })?;
        self.provider.lock().unwrap().refresh();
        Ok(child)
    }

    fn get_node_clone(&self, id: &str) -> anyhow::Result<DesignNode> {
        self.provider
            .lock()
            .unwrap()
            .get_node(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Node '{id}' not found"))
    }
}

fn bootstrap_node_to_opsx(opsx: &mut OpsxLifecycle<JsonFileStore>, node: &DesignNode) {
    let current_opsx = NodeState::parse(node.status.as_str()).unwrap_or(NodeState::Seed);
    let _ = opsx.create_node(&node.id, &node.title, None);
    if current_opsx != NodeState::Seed {
        let _ = opsx.force_transition_node(&node.id, current_opsx, "bootstrap sync from markdown");
    }
    for q in &node.open_questions {
        let _ = opsx.add_question(&node.id, q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_design_node_writes_markdown_and_opsx_state() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo.clone(), Arc::clone(&provider), Arc::clone(&opsx));

        let node = service
            .create_design_node(CreateDesignNodeRequest {
                id: "new-node".to_string(),
                title: "New Node".to_string(),
                parent: None,
                status: Some("decided".to_string()),
                tags: vec!["test".to_string()],
                overview: "overview".to_string(),
            })
            .unwrap();

        assert!(node.file_path.exists());
        assert!(provider.lock().unwrap().get_node("new-node").is_some());
        let opsx = opsx.lock().unwrap();
        assert!(opsx.get_node("new-node").is_some());
        assert_eq!(opsx.get_node("new-node").unwrap().state, NodeState::Decided);
    }

    #[test]
    fn set_design_node_status_updates_markdown_and_opsx_state() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo, Arc::clone(&provider), Arc::clone(&opsx));
        service
            .create_design_node(CreateDesignNodeRequest {
                id: "new-node".to_string(),
                title: "New Node".to_string(),
                parent: None,
                status: None,
                tags: vec![],
                overview: "overview".to_string(),
            })
            .unwrap();

        let result = service
            .set_design_node_status(SetDesignNodeStatusRequest {
                id: "new-node".to_string(),
                status: NodeStatus::Exploring,
                archive_reason: None,
                superseded_by: None,
                archived_at: None,
            })
            .unwrap();

        assert_eq!(result.node_title, "New Node");
        assert_eq!(provider.lock().unwrap().get_node("new-node").unwrap().status, NodeStatus::Exploring);
        assert_eq!(opsx.lock().unwrap().get_node("new-node").unwrap().state, NodeState::Exploring);
    }

    #[test]
    fn question_mutations_update_markdown_and_refresh_provider() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo, Arc::clone(&provider), Arc::clone(&opsx));
        service
            .create_design_node(CreateDesignNodeRequest {
                id: "new-node".to_string(),
                title: "New Node".to_string(),
                parent: None,
                status: None,
                tags: vec![],
                overview: "overview".to_string(),
            })
            .unwrap();

        service
            .add_design_node_question(UpdateDesignNodeQuestionRequest {
                id: "new-node".to_string(),
                question: "What next?".to_string(),
            })
            .unwrap();
        assert_eq!(provider.lock().unwrap().get_node("new-node").unwrap().open_questions, vec!["What next?"]);

        service
            .remove_design_node_question(UpdateDesignNodeQuestionRequest {
                id: "new-node".to_string(),
                question: "What next?".to_string(),
            })
            .unwrap();
        assert!(provider.lock().unwrap().get_node("new-node").unwrap().open_questions.is_empty());
    }

    #[test]
    fn add_research_updates_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo, Arc::clone(&provider), Arc::clone(&opsx));
        service
            .create_design_node(CreateDesignNodeRequest {
                id: "new-node".to_string(),
                title: "New Node".to_string(),
                parent: None,
                status: None,
                tags: vec![],
                overview: "overview".to_string(),
            })
            .unwrap();

        service
            .add_design_node_research(AddDesignNodeResearchRequest {
                id: "new-node".to_string(),
                heading: "Finding".to_string(),
                content: "Evidence.".to_string(),
            })
            .unwrap();

        let node = provider.lock().unwrap().get_node("new-node").cloned().unwrap();
        let sections = design::read_node_sections(&node).unwrap();
        assert_eq!(sections.research.len(), 1);
        assert_eq!(sections.research[0].heading, "Finding");
        assert_eq!(sections.research[0].content, "Evidence.");
    }

    #[test]
    fn add_decision_updates_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo, Arc::clone(&provider), Arc::clone(&opsx));
        service
            .create_design_node(CreateDesignNodeRequest {
                id: "new-node".to_string(),
                title: "New Node".to_string(),
                parent: None,
                status: None,
                tags: vec![],
                overview: "overview".to_string(),
            })
            .unwrap();

        service
            .add_design_node_decision(AddDesignNodeDecisionRequest {
                id: "new-node".to_string(),
                title: "Choose Path".to_string(),
                status: "decided".to_string(),
                rationale: "Evidence supports it.".to_string(),
            })
            .unwrap();

        let node = provider.lock().unwrap().get_node("new-node").cloned().unwrap();
        let sections = design::read_node_sections(&node).unwrap();
        assert_eq!(sections.decisions.len(), 1);
        assert_eq!(sections.decisions[0].title, "Choose Path");
        assert_eq!(sections.decisions[0].status, "decided");
        assert_eq!(sections.decisions[0].rationale, "Evidence supports it.");
    }

    #[test]
    fn link_mutations_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo, Arc::clone(&provider), Arc::clone(&opsx));
        service
            .create_design_node(CreateDesignNodeRequest {
                id: "new-node".to_string(),
                title: "New Node".to_string(),
                parent: None,
                status: None,
                tags: vec![],
                overview: "overview".to_string(),
            })
            .unwrap();

        for _ in 0..2 {
            service
                .add_design_node_dependency(AddDesignNodeLinkRequest {
                    id: "new-node".to_string(),
                    target_id: "dep".to_string(),
                })
                .unwrap();
            service
                .add_design_node_related(AddDesignNodeLinkRequest {
                    id: "new-node".to_string(),
                    target_id: "rel".to_string(),
                })
                .unwrap();
        }

        let node = provider.lock().unwrap().get_node("new-node").cloned().unwrap();
        assert_eq!(node.dependencies, vec!["dep"]);
        assert_eq!(node.related, vec!["rel"]);
    }

    #[test]
    fn add_impl_notes_updates_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo, Arc::clone(&provider), Arc::clone(&opsx));
        service
            .create_design_node(CreateDesignNodeRequest {
                id: "new-node".to_string(),
                title: "New Node".to_string(),
                parent: None,
                status: None,
                tags: vec![],
                overview: "overview".to_string(),
            })
            .unwrap();

        service
            .add_design_node_impl_notes(AddDesignNodeImplNotesRequest {
                id: "new-node".to_string(),
                file_scope: vec![FileScope {
                    path: "src/lib.rs".to_string(),
                    description: "Update logic".to_string(),
                    action: Some("modified".to_string()),
                }],
                constraints: vec!["Keep behavior stable".to_string()],
            })
            .unwrap();

        let node = provider.lock().unwrap().get_node("new-node").cloned().unwrap();
        let sections = design::read_node_sections(&node).unwrap();
        assert_eq!(sections.impl_file_scope.len(), 1);
        assert_eq!(sections.impl_file_scope[0].path, "src/lib.rs");
        assert_eq!(sections.impl_constraints, vec!["Keep behavior stable"]);
    }

    #[test]
    fn branch_design_node_creates_child_and_removes_question() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        let provider = Arc::new(Mutex::new(LifecycleContextProvider::new(&repo)));
        let opsx = Arc::new(Mutex::new(
            OpsxLifecycle::load(JsonFileStore::new(&repo)).unwrap(),
        ));
        let service = LifecycleMutationService::new(repo, Arc::clone(&provider), Arc::clone(&opsx));
        service
            .create_design_node(CreateDesignNodeRequest {
                id: "parent".to_string(),
                title: "Parent".to_string(),
                parent: None,
                status: None,
                tags: vec![],
                overview: "overview".to_string(),
            })
            .unwrap();
        service
            .add_design_node_question(UpdateDesignNodeQuestionRequest {
                id: "parent".to_string(),
                question: "Which path?".to_string(),
            })
            .unwrap();

        let child = service
            .branch_design_node_question(BranchDesignNodeQuestionRequest {
                parent_id: "parent".to_string(),
                question: "Which path?".to_string(),
                child_id: "child".to_string(),
                child_title: "Child".to_string(),
            })
            .unwrap();

        assert_eq!(child.parent.as_deref(), Some("parent"));
        let provider = provider.lock().unwrap();
        assert!(provider.get_node("parent").unwrap().open_questions.is_empty());
        assert_eq!(provider.get_node("child").unwrap().parent.as_deref(), Some("parent"));
    }
}

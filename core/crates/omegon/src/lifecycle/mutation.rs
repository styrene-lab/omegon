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
use super::types::{DesignNode, NodeStatus};

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
        let mut node = self
            .provider
            .lock()
            .unwrap()
            .get_node(&req.id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", req.id))?;
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
}

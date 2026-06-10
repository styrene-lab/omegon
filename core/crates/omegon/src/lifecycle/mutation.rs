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
use super::types::DesignNode;

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
}

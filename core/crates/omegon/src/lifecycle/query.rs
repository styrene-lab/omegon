//! Design-tree query policy.
//!
//! These helpers keep ready/blocked/frontier selection out of tool adapters so
//! UI, API, and tool surfaces can share one interpretation of design state.

use std::collections::HashMap;

use super::design;
use super::types::{DesignNode, NodeStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildNode {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyNode {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyNode {
    pub id: String,
    pub title: String,
    pub priority: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub blocked_by: Vec<String>,
}

pub fn frontier(nodes: &HashMap<String, DesignNode>) -> Vec<FrontierNode> {
    let mut result: Vec<_> = nodes
        .values()
        .filter(|node| !is_archived(node))
        .filter(|node| !node.open_questions.is_empty())
        .map(|node| FrontierNode {
            id: node.id.clone(),
            title: node.title.clone(),
            status: node.status.as_str().to_string(),
            open_questions: node.open_questions.clone(),
        })
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

pub fn children(nodes: &HashMap<String, DesignNode>, node_id: &str) -> Vec<ChildNode> {
    let mut result: Vec<_> = design::get_children(nodes, node_id)
        .iter()
        .filter(|node| !is_archived(node))
        .map(|node| ChildNode {
            id: node.id.clone(),
            title: node.title.clone(),
            status: node.status.as_str().to_string(),
        })
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

pub fn dependencies(nodes: &HashMap<String, DesignNode>, node: &DesignNode) -> Vec<DependencyNode> {
    node.dependencies
        .iter()
        .filter_map(|dep_id| {
            nodes.get(dep_id).map(|dep| DependencyNode {
                id: dep.id.clone(),
                title: dep.title.clone(),
                status: dep.status.as_str().to_string(),
            })
        })
        .collect()
}

pub fn ready(nodes: &HashMap<String, DesignNode>) -> Vec<ReadyNode> {
    let mut result: Vec<_> = nodes
        .values()
        .filter(|node| !is_archived(node))
        .filter(|node| matches!(node.status, NodeStatus::Decided))
        .filter(|node| {
            node.dependencies.iter().all(|dep_id| {
                nodes
                    .get(dep_id)
                    .is_some_and(|dep| matches!(dep.status, NodeStatus::Implemented))
            })
        })
        .map(|node| ReadyNode {
            id: node.id.clone(),
            title: node.title.clone(),
            priority: node.priority,
        })
        .collect();
    result.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
    result
}

pub fn blocked(nodes: &HashMap<String, DesignNode>) -> Vec<BlockedNode> {
    let mut result: Vec<_> = nodes
        .values()
        .filter(|node| {
            matches!(node.status, NodeStatus::Blocked)
                || node.dependencies.iter().any(|dep_id| {
                    nodes
                        .get(dep_id)
                        .is_none_or(|dep| !matches!(dep.status, NodeStatus::Implemented))
                })
        })
        .map(|node| {
            let blocked_by = node
                .dependencies
                .iter()
                .filter(|dep_id| {
                    nodes
                        .get(*dep_id)
                        .is_none_or(|dep| !matches!(dep.status, NodeStatus::Implemented))
                })
                .cloned()
                .collect();
            BlockedNode {
                id: node.id.clone(),
                title: node.title.clone(),
                status: node.status.as_str().to_string(),
                blocked_by,
            }
        })
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

pub fn is_archived(node: &DesignNode) -> bool {
    matches!(node.status, NodeStatus::Archived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn node(id: &str, status: NodeStatus) -> DesignNode {
        DesignNode {
            id: id.to_string(),
            title: id.to_string(),
            status,
            parent: None,
            tags: vec![],
            dependencies: vec![],
            related: vec![],
            open_questions: vec![],
            branches: vec![],
            openspec_change: None,
            issue_type: None,
            priority: None,
            archive_reason: None,
            superseded_by: None,
            archived_at: None,
            file_path: PathBuf::from(format!("docs/{id}.md")),
        }
    }

    #[test]
    fn ready_requires_decided_status_and_implemented_dependencies() {
        let mut nodes = HashMap::new();
        let dep = node("dep", NodeStatus::Implemented);
        let mut ready_node = node("ready", NodeStatus::Decided);
        ready_node.dependencies.push("dep".to_string());
        let mut blocked_node = node("blocked", NodeStatus::Decided);
        blocked_node.dependencies.push("missing".to_string());
        nodes.insert(dep.id.clone(), dep);
        nodes.insert(ready_node.id.clone(), ready_node);
        nodes.insert(blocked_node.id.clone(), blocked_node);

        let ready = ready(&nodes);

        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "ready");
    }

    #[test]
    fn blocked_reports_missing_or_unimplemented_dependencies() {
        let mut nodes = HashMap::new();
        let dep = node("dep", NodeStatus::Exploring);
        let mut blocked_node = node("blocked", NodeStatus::Decided);
        blocked_node.dependencies = vec!["dep".to_string(), "missing".to_string()];
        nodes.insert(dep.id.clone(), dep);
        nodes.insert(blocked_node.id.clone(), blocked_node);

        let blocked = blocked(&nodes);

        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].blocked_by, vec!["dep", "missing"]);
    }
}

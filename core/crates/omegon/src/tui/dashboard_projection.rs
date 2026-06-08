//! Shared dashboard semantic projection types.
//!
//! These structs describe dashboard content without binding it to Ratatui tree,
//! layout, or frame state. The current TUI dashboard state projects into this
//! shape before rendering; ACP or future clients can derive protocol DTOs from
//! the same semantic surface later.

use super::dashboard::{
    ChangeSummary, DegradedNodeSummary, FocusedNodeSummary, NodeSummary, StatusCounts,
};

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardProjection {
    pub focused_node: Option<FocusedNodeProjection>,
    pub active_changes: Vec<ChangeProjection>,
    pub status_counts: StatusCounts,
    pub implementing_nodes: Vec<NodeProjection>,
    pub actionable_nodes: Vec<NodeProjection>,
    pub all_nodes: Vec<NodeProjection>,
    pub degraded_nodes: Vec<DegradedNodeProjection>,
    pub session: DashboardSessionProjection,
    pub context: DashboardContextProjection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardSessionProjection {
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardContextProjection {
    pub used_pct: f32,
    pub window_k: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeProjection {
    pub id: String,
    pub title: String,
    pub status: String,
    pub open_questions: usize,
    pub parent: Option<String>,
    pub priority: Option<u8>,
    pub issue_type: Option<String>,
    pub openspec_change: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FocusedNodeProjection {
    pub id: String,
    pub title: String,
    pub status: String,
    pub open_questions: usize,
    pub assumptions: usize,
    pub decisions: usize,
    pub readiness: f32,
    pub openspec_change: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DegradedNodeProjection {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeProjection {
    pub name: String,
    pub stage: String,
    pub done_tasks: usize,
    pub total_tasks: usize,
}

pub trait ProjectDashboardSurface {
    fn project_dashboard_surface(&self) -> DashboardProjection;
}

impl From<&NodeSummary> for NodeProjection {
    fn from(node: &NodeSummary) -> Self {
        Self {
            id: node.id.clone(),
            title: node.title.clone(),
            status: format!("{:?}", node.status).to_ascii_lowercase(),
            open_questions: node.open_questions,
            parent: node.parent.clone(),
            priority: node.priority,
            issue_type: node
                .issue_type
                .map(|kind| format!("{:?}", kind).to_ascii_lowercase()),
            openspec_change: node.openspec_change.clone(),
        }
    }
}

impl From<&FocusedNodeSummary> for FocusedNodeProjection {
    fn from(node: &FocusedNodeSummary) -> Self {
        Self {
            id: node.id.clone(),
            title: node.title.clone(),
            status: format!("{:?}", node.status).to_ascii_lowercase(),
            open_questions: node.open_questions,
            assumptions: node.assumptions,
            decisions: node.decisions,
            readiness: node.readiness,
            openspec_change: node.openspec_change.clone(),
        }
    }
}

impl From<&DegradedNodeSummary> for DegradedNodeProjection {
    fn from(node: &DegradedNodeSummary) -> Self {
        Self {
            id: node.id.clone(),
            title: node.title.clone(),
            file_path: node.file_path.clone(),
            reason: node.reason.clone(),
        }
    }
}

impl From<&ChangeSummary> for ChangeProjection {
    fn from(change: &ChangeSummary) -> Self {
        Self {
            name: change.name.clone(),
            stage: change.stage.clone(),
            done_tasks: change.done_tasks,
            total_tasks: change.total_tasks,
        }
    }
}

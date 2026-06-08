//! TUI backing-state adapter for the shared dashboard surface projection.

use super::dashboard::{
    ChangeSummary, DashboardState, DegradedNodeSummary, FocusedNodeSummary, NodeSummary,
    StatusCounts,
};
use crate::lifecycle::types::{IssueType, NodeStatus};
use crate::surfaces::dashboard::{
    ChangeProjection, DashboardContextProjection, DashboardProjection, DashboardSessionProjection,
    DegradedNodeProjection, FocusedNodeProjection, NodeProjection, ProjectDashboardSurface,
    StatusCountsProjection,
};

fn project_node_status(status: NodeStatus) -> &'static str {
    status.as_str()
}

fn project_issue_type(issue_type: IssueType) -> &'static str {
    match issue_type {
        IssueType::Epic => "epic",
        IssueType::Feature => "feature",
        IssueType::Task => "task",
        IssueType::Bug => "bug",
        IssueType::Chore => "chore",
    }
}

impl From<&StatusCounts> for StatusCountsProjection {
    fn from(counts: &StatusCounts) -> Self {
        Self {
            total: counts.total,
            implementing: counts.implementing,
            decided: counts.decided,
            exploring: counts.exploring,
            implemented: counts.implemented,
            blocked: counts.blocked,
            deferred: counts.deferred,
            open_questions: counts.open_questions,
        }
    }
}

impl From<&NodeSummary> for NodeProjection {
    fn from(node: &NodeSummary) -> Self {
        Self {
            id: node.id.clone(),
            title: node.title.clone(),
            status: project_node_status(node.status).to_string(),
            open_questions: node.open_questions,
            parent: node.parent.clone(),
            priority: node.priority,
            issue_type: node
                .issue_type
                .map(|kind| project_issue_type(kind).to_string()),
            openspec_change: node.openspec_change.clone(),
        }
    }
}

impl From<&FocusedNodeSummary> for FocusedNodeProjection {
    fn from(node: &FocusedNodeSummary) -> Self {
        Self {
            id: node.id.clone(),
            title: node.title.clone(),
            status: project_node_status(node.status).to_string(),
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

impl ProjectDashboardSurface for DashboardState {
    fn project_dashboard_surface(&self) -> DashboardProjection {
        DashboardProjection {
            focused_node: self.focused_node.as_ref().map(Into::into),
            active_changes: self.active_changes.iter().map(Into::into).collect(),
            status_counts: (&self.status_counts).into(),
            implementing_nodes: self.implementing_nodes.iter().map(Into::into).collect(),
            actionable_nodes: self.actionable_nodes.iter().map(Into::into).collect(),
            all_nodes: self.all_nodes.iter().map(Into::into).collect(),
            degraded_nodes: self.degraded_nodes.iter().map(Into::into).collect(),
            session: DashboardSessionProjection {
                turns: self.turns,
                tool_calls: self.tool_calls,
                compactions: self.compactions,
            },
            context: DashboardContextProjection {
                used_pct: self.context_used_pct,
                window_k: self.context_window_k,
            },
        }
    }
}

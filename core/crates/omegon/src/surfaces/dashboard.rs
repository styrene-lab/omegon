//! Shared dashboard semantic projection types.
//!
//! These structs describe dashboard content without binding it to Ratatui tree,
//! layout, frame state, or the TUI dashboard backing structs.

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardProjection {
    pub focused_node: Option<FocusedNodeProjection>,
    pub active_changes: Vec<ChangeProjection>,
    pub status_counts: StatusCountsProjection,
    pub implementing_nodes: Vec<NodeProjection>,
    pub actionable_nodes: Vec<NodeProjection>,
    pub all_nodes: Vec<NodeProjection>,
    pub degraded_nodes: Vec<DegradedNodeProjection>,
    pub session: DashboardSessionProjection,
    pub context: DashboardContextProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusCountsProjection {
    pub total: usize,
    pub implementing: usize,
    pub decided: usize,
    pub exploring: usize,
    pub implemented: usize,
    pub blocked: usize,
    pub deferred: usize,
    pub open_questions: usize,
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

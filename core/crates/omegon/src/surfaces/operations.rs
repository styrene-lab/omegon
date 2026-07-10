//! Renderer-neutral operation projections for delegate/cleave child work.
//!
//! These DTOs separate operation state from specific renderers. TUI Workbench,
//! transcript milestones, ACP, and future dashboards should consume these
//! projections instead of inferring status from raw tool-call or decomposition
//! event text.

use crate::features::cleave::{CleaveChildFailureKind, CleaveProgress};
use crate::features::delegate::{DelegateChildFailureKind, DelegateProgress};
use crate::surfaces::conversation::ToolActivitySummary;
use omegon_traits::{OperationKind, OperationRef};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationMilestoneProjection {
    pub icon: &'static str,
    pub text: String,
}

impl OperationMilestoneProjection {
    pub fn started(operation: &OperationRef, child_count: usize) -> Self {
        match operation.kind {
            OperationKind::Cleave => Self {
                icon: "↯",
                text: format!("Cleave: {child_count} children dispatched"),
            },
            OperationKind::Delegate => {
                let label = operation.id.as_deref().unwrap_or("subagent");
                Self {
                    icon: "⇉",
                    text: format!("Delegate: {label} started"),
                }
            }
        }
    }

    pub fn child_completed(operation: &OperationRef, label: &str, success: bool) -> Self {
        let icon = if success { "✓" } else { "✗" };
        match operation.kind {
            OperationKind::Cleave => Self {
                icon,
                text: format!("Child '{label}' completed"),
            },
            OperationKind::Delegate => Self {
                icon,
                text: format!("Delegate: {label} completed"),
            },
        }
    }

    pub fn completed(operation: &OperationRef, merged: bool) -> Self {
        let status = if merged {
            "merged"
        } else {
            "completed (no merge)"
        };
        let label = match operation.kind {
            OperationKind::Cleave => "Cleave",
            OperationKind::Delegate => "Delegate",
        };
        Self {
            icon: "↯",
            text: format!("{label} {status}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationWorkbenchProjection {
    pub operation: OperationRef,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub pending_results: usize,
    pub children: Vec<OperationChildRow>,
}

impl OperationWorkbenchProjection {
    pub fn to_status_details(&self, active: bool) -> Value {
        json!({
            "active": active,
            "running": self.running,
            "completed": self.completed,
            "failed": self.failed,
            "pending_results": self.pending_results,
            "task_count": self.children.len(),
            "child_count": self.children.len(),
            "operation": {
                "kind": match self.operation.kind {
                    OperationKind::Delegate => "delegate",
                    OperationKind::Cleave => "cleave",
                },
                "id": self.operation.id.as_deref(),
            },
            "children": self.children.iter().map(OperationChildRow::to_status_details).collect::<Vec<_>>(),
        })
    }

    pub fn from_delegate(progress: &DelegateProgress) -> Self {
        Self {
            operation: OperationRef::delegate("delegate"),
            running: progress.running,
            completed: progress.completed,
            failed: progress.failed,
            pending_results: progress.pending_results,
            children: progress
                .children
                .iter()
                .map(OperationChildRow::from_delegate_child)
                .collect(),
        }
    }

    pub fn from_cleave(progress: &CleaveProgress) -> Self {
        Self {
            operation: OperationRef::cleave(
                (!progress.run_id.is_empty()).then_some(progress.run_id.clone()),
            ),
            running: progress
                .children
                .iter()
                .filter(|child| child.status == "running")
                .count(),
            completed: progress.completed,
            failed: progress.failed,
            pending_results: 0,
            children: progress
                .children
                .iter()
                .map(OperationChildRow::from_cleave_child)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationChildRow {
    pub operation_kind: OperationKind,
    pub id: String,
    pub label: String,
    pub status: OperationChildStatus,
    pub status_label: String,
    pub result_viewed: bool,
    pub last_activity: Option<OperationActivity>,
    pub progress: Option<OperationChildProgress>,
    pub result_summary: Option<String>,
    pub failure: Option<OperationFailure>,
}

impl OperationChildRow {
    pub fn to_status_details(&self) -> Value {
        json!({
            "task_id": self.id,
            "label": self.label,
            "status": self.status.as_str(),
            "status_label": self.status_label,
            "result_viewed": self.result_viewed,
            "result_ready": !self.result_viewed && !matches!(self.status, OperationChildStatus::Running | OperationChildStatus::Queued | OperationChildStatus::Starting | OperationChildStatus::Waiting),
            "last_tool": self.last_activity.as_ref().map(|activity| activity.label.as_str()),
            "last_tool_args_summary": self.last_activity.as_ref().and_then(|activity| activity.args_summary.as_deref()),
            "last_turn": self.last_activity.as_ref().and_then(|activity| activity.turn),
            "result_summary": self.result_summary.as_deref(),
            "tasks_done": self.progress.as_ref().map(|progress| progress.done).unwrap_or(0),
            "tasks_total": self.progress.as_ref().map(|progress| progress.total).unwrap_or(0),
            "failure": self.failure.as_ref().map(|failure| json!({
                "kind": failure.kind.as_str(),
                "message": failure.message.as_deref(),
                "recoverable": failure.recoverable,
            })),
        })
    }

    fn from_cleave_child(child: &crate::features::cleave::ChildProgress) -> Self {
        let status = OperationChildStatus::from_cleave_status(&child.status);
        Self {
            operation_kind: OperationKind::Cleave,
            id: child.label.clone(),
            label: child.label.clone(),
            status,
            status_label: child.status.clone(),
            result_viewed: true,
            last_activity: child
                .last_tool_activity
                .clone()
                .or_else(|| {
                    child
                        .last_tool
                        .as_ref()
                        .map(|tool| ToolActivitySummary::new(tool.clone(), None))
                })
                .map(|activity| OperationActivity {
                    kind: OperationActivityKind::Tool,
                    label: activity.raw_name,
                    args_summary: activity.args_summary,
                    turn: child.last_turn,
                }),
            progress: (!child.tasks.is_empty()).then_some(OperationChildProgress {
                done: child.tasks_done,
                total: child.tasks.len(),
            }),
            result_summary: None,
            failure: match status {
                OperationChildStatus::Failed | OperationChildStatus::TimedOut => {
                    let kind = child
                        .failure_kind
                        .map(OperationFailureKind::from_cleave_child_failure_kind)
                        .unwrap_or_else(|| match child.status.as_str() {
                            "upstream_exhausted" => OperationFailureKind::ModelError,
                            _ => OperationFailureKind::Unknown,
                        });
                    Some(OperationFailure {
                        kind,
                        message: None,
                        recoverable: matches!(
                            kind,
                            OperationFailureKind::IdleTimeout
                                | OperationFailureKind::TimedOut
                                | OperationFailureKind::ModelError
                                | OperationFailureKind::ToolExecutionFailed
                        ),
                    })
                }
                _ => None,
            },
        }
    }

    fn from_delegate_child(child: &crate::features::delegate::DelegateProgressChild) -> Self {
        let status = OperationChildStatus::from_delegate_status(&child.status);
        let failure = match status {
            OperationChildStatus::Failed | OperationChildStatus::TimedOut => {
                Some(OperationFailure::from_delegate_failure(
                    child.failure_kind,
                    child.result_summary.clone(),
                ))
            }
            _ => None,
        };
        Self {
            operation_kind: OperationKind::Delegate,
            id: child.task_id.clone(),
            label: child.label.clone(),
            status,
            status_label: child.status.clone(),
            result_viewed: child.result_viewed,
            last_activity: child
                .last_tool_activity
                .clone()
                .or_else(|| {
                    child
                        .last_tool
                        .as_ref()
                        .map(|tool| ToolActivitySummary::new(tool.clone(), None))
                })
                .map(|activity| OperationActivity {
                    kind: OperationActivityKind::Tool,
                    label: activity.raw_name,
                    args_summary: activity.args_summary,
                    turn: child.last_turn,
                }),
            progress: (!child.tasks.is_empty()).then_some(OperationChildProgress {
                done: child.tasks_done,
                total: child.tasks.len(),
            }),
            result_summary: child.result_summary.clone(),
            failure,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationChildStatus {
    Queued,
    Starting,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
}

impl OperationChildStatus {
    pub fn from_cleave_status(status: &str) -> Self {
        match status {
            "pending" => Self::Queued,
            "running" => Self::Running,
            "completed" | "merged_after_failure" => Self::Succeeded,
            "failed" | "upstream_exhausted" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "timed_out" | "timeout" | "idle_timeout" => Self::TimedOut,
            _ => Self::Unknown,
        }
    }

    pub fn from_delegate_status(status: &str) -> Self {
        match status {
            "running" => Self::Running,
            "completed" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "timed_out" | "timeout" | "idle_timeout" => Self::TimedOut,
            "queued" => Self::Queued,
            "starting" => Self::Starting,
            "waiting" => Self::Waiting,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed out",
            Self::Unknown => "unknown",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Succeeded => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }

    pub fn is_failure(self) -> bool {
        matches!(self, Self::Failed | Self::TimedOut)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationActivity {
    pub kind: OperationActivityKind,
    pub label: String,
    pub args_summary: Option<String>,
    pub turn: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationActivityKind {
    Tool,
    Message,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationChildProgress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationFailure {
    pub kind: OperationFailureKind,
    pub message: Option<String>,
    pub recoverable: bool,
}

impl OperationFailure {
    fn from_delegate_failure(
        source_kind: Option<DelegateChildFailureKind>,
        summary: Option<String>,
    ) -> Self {
        let kind = source_kind
            .and_then(|kind| match kind {
                DelegateChildFailureKind::Unknown => None,
                known => Some(OperationFailureKind::from_delegate_child_failure_kind(
                    known,
                )),
            })
            .or_else(|| summary.as_deref().map(OperationFailureKind::from_message))
            .unwrap_or(OperationFailureKind::Unknown);
        Self {
            kind,
            message: summary,
            recoverable: matches!(
                kind,
                OperationFailureKind::IdleTimeout
                    | OperationFailureKind::TimedOut
                    | OperationFailureKind::ToolExecutionFailed
                    | OperationFailureKind::ModelError
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationFailureKind {
    IdleTimeout,
    TimedOut,
    ProcessExit,
    ModelError,
    ToolPermissionDenied,
    ToolExecutionFailed,
    SandboxViolation,
    MergeConflict,
    CancelledByOperator,
    DuplicateTask,
    Unknown,
}

impl OperationFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdleTimeout => "idle_timeout",
            Self::TimedOut => "timed_out",
            Self::ProcessExit => "process_exit",
            Self::ModelError => "model_error",
            Self::ToolPermissionDenied => "tool_permission_denied",
            Self::ToolExecutionFailed => "tool_execution_failed",
            Self::SandboxViolation => "sandbox_violation",
            Self::MergeConflict => "merge_conflict",
            Self::CancelledByOperator => "cancelled_by_operator",
            Self::DuplicateTask => "duplicate_task",
            Self::Unknown => "unknown",
        }
    }

    fn from_cleave_child_failure_kind(kind: CleaveChildFailureKind) -> Self {
        match kind {
            CleaveChildFailureKind::ChildProcessExit => Self::ProcessExit,
            CleaveChildFailureKind::IdleTimeout => Self::IdleTimeout,
            CleaveChildFailureKind::WallTimeout => Self::TimedOut,
            CleaveChildFailureKind::MergeConflict => Self::MergeConflict,
            CleaveChildFailureKind::ScopeViolation => Self::SandboxViolation,
            CleaveChildFailureKind::UpstreamExhausted => Self::ModelError,
            CleaveChildFailureKind::ValidationFailed => Self::ToolExecutionFailed,
            CleaveChildFailureKind::Unknown => Self::Unknown,
        }
    }

    fn from_delegate_child_failure_kind(kind: DelegateChildFailureKind) -> Self {
        match kind {
            DelegateChildFailureKind::MissingLocalModel
            | DelegateChildFailureKind::MissingCredential
            | DelegateChildFailureKind::ProviderStartup => Self::ModelError,
            DelegateChildFailureKind::WorkspaceStartup => Self::ProcessExit,
            DelegateChildFailureKind::Unknown => Self::Unknown,
        }
    }

    fn from_message(message: &str) -> Self {
        let lower = message.to_ascii_lowercase();
        if lower.contains("idle timeout") || lower.contains("no output") {
            Self::IdleTimeout
        } else if lower.contains("wall-clock timeout")
            || lower.contains("timed out")
            || lower.contains("timeout")
        {
            Self::TimedOut
        } else if lower.contains("sandbox") {
            Self::SandboxViolation
        } else if lower.contains("permission") || lower.contains("denied") {
            Self::ToolPermissionDenied
        } else if lower.contains("model") || lower.contains("provider") {
            Self::ModelError
        } else if lower.contains("merge conflict") {
            Self::MergeConflict
        } else if lower.contains("cancel") {
            Self::CancelledByOperator
        } else if lower.contains("duplicate") {
            Self::DuplicateTask
        } else if lower.contains("exit") || lower.contains("process") {
            Self::ProcessExit
        } else if lower.contains("tool") || lower.contains("failed") {
            Self::ToolExecutionFailed
        } else {
            Self::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child_agent::ChildTaskItem;
    use crate::features::cleave::{CleaveChildFailureKind, CleaveProgress};
    use crate::features::delegate::{DelegateProgress, DelegateProgressChild};

    fn delegate_child(status: &str) -> DelegateProgressChild {
        DelegateProgressChild {
            task_id: "delegate_1".into(),
            label: "delegate_1".into(),
            status: status.into(),
            result_viewed: !matches!(status, "completed" | "failed" | "cancelled"),
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            started_at: None,
            completed_at: None,
            result_summary: None,
            failure_kind: None,
            tasks: Vec::new(),
            tasks_done: 0,
            route_decision: None,
        }
    }

    #[test]
    fn cancelled_delegate_child_maps_to_terminal_non_failure_status() {
        let mut child = delegate_child("cancelled");
        child.result_summary = Some("operator stopped task".into());
        let progress = DelegateProgress {
            active: false,
            running: 0,
            completed: 0,
            failed: 0,
            pending_results: 0,
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        let row = &projection.children[0];
        assert_eq!(row.status, OperationChildStatus::Cancelled);
        assert!(row.status.is_terminal());
        assert!(!row.status.is_failure());
        assert!(row.failure.is_none());
    }

    #[test]
    fn delegate_progress_maps_to_operation_projection_counts() {
        let progress = DelegateProgress {
            active: true,
            running: 2,
            completed: 1,
            failed: 1,
            pending_results: 0,
            children: vec![delegate_child("running")],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        assert_eq!(projection.operation.kind, OperationKind::Delegate);
        assert_eq!(projection.running, 2);
        assert_eq!(projection.completed, 1);
        assert_eq!(projection.failed, 1);
        assert_eq!(projection.children.len(), 1);
        assert_eq!(projection.children[0].status, OperationChildStatus::Running);
    }

    #[test]
    fn delegate_child_maps_task_progress_and_last_tool() {
        let mut child = delegate_child("running");
        child.last_tool = Some("bash".into());
        child.last_tool_activity = Some(ToolActivitySummary::new(
            "bash",
            Some("cargo test -p omegon".into()),
        ));
        child.last_turn = Some(3);
        child.tasks_done = 1;
        child.tasks = vec![
            ChildTaskItem {
                description: "inspect".into(),
                done: true,
            },
            ChildTaskItem {
                description: "report".into(),
                done: false,
            },
        ];
        let progress = DelegateProgress {
            active: true,
            running: 1,
            completed: 0,
            failed: 0,
            pending_results: 0,
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        let row = &projection.children[0];
        assert_eq!(row.last_activity.as_ref().unwrap().label, "bash");
        assert_eq!(
            row.last_activity.as_ref().unwrap().args_summary.as_deref(),
            Some("cargo test -p omegon")
        );
        assert_eq!(row.last_activity.as_ref().unwrap().turn, Some(3));
        assert_eq!(row.progress.as_ref().unwrap().done, 1);
        assert_eq!(row.progress.as_ref().unwrap().total, 2);
    }

    #[test]
    fn delegate_unknown_failure_kind_falls_back_to_summary_classification() {
        let mut child = delegate_child("failed");
        child.failure_kind = Some(DelegateChildFailureKind::Unknown);
        child.result_summary = Some("Delegate idle timeout — no output for 120s".into());
        let progress = DelegateProgress {
            active: false,
            running: 0,
            completed: 0,
            failed: 1,
            pending_results: 0,
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        assert_eq!(
            projection.children[0].failure.as_ref().unwrap().kind,
            OperationFailureKind::IdleTimeout
        );
    }

    #[test]
    fn delegate_typed_failure_kind_overrides_summary_classification() {
        let mut child = delegate_child("failed");
        child.failure_kind = Some(DelegateChildFailureKind::MissingCredential);
        child.result_summary = Some("Delegate idle timeout — no output for 120s".into());
        let progress = DelegateProgress {
            active: false,
            running: 0,
            completed: 0,
            failed: 1,
            pending_results: 0,
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        assert_eq!(
            projection.children[0].failure.as_ref().unwrap().kind,
            OperationFailureKind::ModelError
        );
    }

    #[test]
    fn delegate_failed_child_maps_failure_summary_when_available() {
        let mut child = delegate_child("failed");
        child.result_summary = Some("Delegate idle timeout — no output for 120s".into());
        let progress = DelegateProgress {
            active: false,
            running: 0,
            completed: 0,
            failed: 1,
            pending_results: 0,
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        let failure = projection.children[0]
            .failure
            .as_ref()
            .expect("failure projection");
        assert_eq!(failure.kind, OperationFailureKind::IdleTimeout);
        assert_eq!(
            failure.message.as_deref(),
            Some("Delegate idle timeout — no output for 120s")
        );
        assert!(failure.recoverable);
    }
    fn cleave_child(label: &str, status: &str) -> crate::features::cleave::ChildProgress {
        crate::features::cleave::ChildProgress {
            label: label.into(),
            status: status.into(),
            failure_kind: None,
            duration_secs: None,
            supervision_mode: None,
            pid: None,
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
            tasks_done: 0,
            started_at: None,
            last_activity_at: None,
            tokens_in: 0,
            tokens_out: 0,
            runtime: None,
        }
    }

    #[test]
    fn pending_cleave_children_are_queued_not_running() {
        let progress = CleaveProgress {
            active: true,
            run_id: "smoke".into(),
            total_children: 2,
            completed: 0,
            failed: 0,
            children: vec![
                cleave_child("research/sources", "pending"),
                cleave_child("outline/structure", "pending"),
            ],
            total_tokens_in: 0,
            total_tokens_out: 0,
        };

        let projection = OperationWorkbenchProjection::from_cleave(&progress);
        assert_eq!(projection.running, 0);
        assert_eq!(projection.completed, 0);
        assert_eq!(projection.failed, 0);
        assert!(
            projection
                .children
                .iter()
                .all(|child| child.status == OperationChildStatus::Queued)
        );
    }

    #[test]
    fn terminal_cleave_projection_has_no_running_children() {
        let mut completed = cleave_child("research/sources", "completed");
        completed.tasks = vec![
            ChildTaskItem {
                description: "collect".into(),
                done: true,
            },
            ChildTaskItem {
                description: "cite".into(),
                done: true,
            },
        ];
        completed.tasks_done = 2;
        let mut failed = cleave_child("review/claims", "failed");
        failed.failure_kind = Some(CleaveChildFailureKind::ValidationFailed);
        failed.tasks = vec![
            ChildTaskItem {
                description: "check".into(),
                done: true,
            },
            ChildTaskItem {
                description: "fix".into(),
                done: false,
            },
        ];
        failed.tasks_done = 1;
        let progress = CleaveProgress {
            active: false,
            run_id: "cleave-docs-research".into(),
            total_children: 2,
            completed: 1,
            failed: 1,
            children: vec![completed, failed],
            total_tokens_in: 0,
            total_tokens_out: 0,
        };

        let projection = OperationWorkbenchProjection::from_cleave(&progress);
        assert_eq!(projection.running, 0);
        assert_eq!(projection.completed, 1);
        assert_eq!(projection.failed, 1);
        assert_eq!(
            projection.children[0].status,
            OperationChildStatus::Succeeded
        );
        assert_eq!(projection.children[0].progress.as_ref().unwrap().done, 2);
        assert_eq!(projection.children[1].status, OperationChildStatus::Failed);
        assert_eq!(projection.children[1].progress.as_ref().unwrap().done, 1);
    }

    #[test]
    fn operation_failure_kind_classifies_delegate_timeout_and_policy_messages() {
        let cases = [
            (
                "Delegate wall-clock timeout after 1s",
                OperationFailureKind::TimedOut,
            ),
            (
                "Delegate idle timeout — no output for 120s",
                OperationFailureKind::IdleTimeout,
            ),
            (
                "tool permission denied by operator",
                OperationFailureKind::ToolPermissionDenied,
            ),
            (
                "provider model overloaded",
                OperationFailureKind::ModelError,
            ),
            (
                "sandbox violation: path denied",
                OperationFailureKind::SandboxViolation,
            ),
            (
                "merge conflict in child worktree",
                OperationFailureKind::MergeConflict,
            ),
            (
                "Delegate task cancelled",
                OperationFailureKind::CancelledByOperator,
            ),
            (
                "duplicate delegate task rejected",
                OperationFailureKind::DuplicateTask,
            ),
            (
                "child process exited with status 1",
                OperationFailureKind::ProcessExit,
            ),
            (
                "tool execution failed",
                OperationFailureKind::ToolExecutionFailed,
            ),
        ];

        for (message, expected) in cases {
            assert_eq!(
                OperationFailureKind::from_message(message),
                expected,
                "message: {message}"
            );
        }
    }

    #[test]
    fn delegate_timeout_failures_map_to_recoverable_projection_failures() {
        let mut wall = delegate_child("failed");
        wall.result_summary = Some("Delegate wall-clock timeout after 1s".into());
        let mut idle = delegate_child("failed");
        idle.result_summary = Some("Delegate idle timeout — no output for 120s".into());
        let progress = DelegateProgress {
            active: false,
            running: 0,
            completed: 0,
            failed: 2,
            pending_results: 0,
            children: vec![wall, idle],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        assert_eq!(
            projection.children[0].failure.as_ref().unwrap().kind,
            OperationFailureKind::TimedOut
        );
        assert!(projection.children[0].failure.as_ref().unwrap().recoverable);
        assert_eq!(
            projection.children[1].failure.as_ref().unwrap().kind,
            OperationFailureKind::IdleTimeout
        );
        assert!(projection.children[1].failure.as_ref().unwrap().recoverable);
    }

    #[test]
    fn delegate_success_result_summary_survives_status_details() {
        let mut child = delegate_child("completed");
        child.result_summary = Some("patched two files".into());
        let progress = DelegateProgress {
            active: false,
            running: 0,
            completed: 1,
            failed: 0,
            pending_results: 0,
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        let details = projection.to_status_details(false);
        assert_eq!(details["child_count"], 1);
        assert_eq!(details["task_count"], 1);
        assert_eq!(details["children"][0]["status"], "completed");
        assert_eq!(
            details["children"][0]["result_summary"],
            "patched two files"
        );
        assert!(details["children"][0]["failure"].is_null());
    }

    #[test]
    fn delegate_failure_details_keep_summary_and_failure_separate() {
        let mut child = delegate_child("failed");
        child.failure_kind = Some(DelegateChildFailureKind::MissingCredential);
        child.result_summary = Some("provider auth unavailable".into());
        let progress = DelegateProgress {
            active: false,
            running: 0,
            completed: 0,
            failed: 1,
            pending_results: 0,
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        let details = projection.to_status_details(false);
        assert_eq!(
            details["children"][0]["result_summary"],
            "provider auth unavailable"
        );
        assert_eq!(details["children"][0]["failure"]["kind"], "model_error");
        assert_eq!(
            details["children"][0]["failure"]["message"],
            "provider auth unavailable"
        );
        assert_eq!(details["children"][0]["failure"]["recoverable"], true);
    }

    #[test]
    fn operation_milestone_delegate_started() {
        let milestone =
            OperationMilestoneProjection::started(&OperationRef::delegate("delegate_1"), 1);
        assert_eq!(milestone.icon, "⇉");
        assert_eq!(milestone.text, "Delegate: delegate_1 started");
    }

    #[test]
    fn operation_milestone_cleave_started() {
        let milestone = OperationMilestoneProjection::started(&OperationRef::cleave(None), 2);
        assert_eq!(milestone.icon, "↯");
        assert_eq!(milestone.text, "Cleave: 2 children dispatched");
    }

    #[test]
    fn operation_milestone_delegate_child_completed() {
        let milestone = OperationMilestoneProjection::child_completed(
            &OperationRef::delegate("delegate_1"),
            "delegate_1",
            true,
        );
        assert_eq!(milestone.icon, "✓");
        assert_eq!(milestone.text, "Delegate: delegate_1 completed");
    }

    #[test]
    fn operation_milestone_cleave_completed_preserves_merge_status() {
        let milestone = OperationMilestoneProjection::completed(&OperationRef::cleave(None), true);
        assert_eq!(milestone.icon, "↯");
        assert_eq!(milestone.text, "Cleave merged");
    }

    #[test]
    fn cleave_typed_failure_kind_maps_to_operation_failure() {
        for (source, expected, recoverable) in [
            (
                CleaveChildFailureKind::UpstreamExhausted,
                OperationFailureKind::ModelError,
                true,
            ),
            (
                CleaveChildFailureKind::MergeConflict,
                OperationFailureKind::MergeConflict,
                false,
            ),
            (
                CleaveChildFailureKind::ScopeViolation,
                OperationFailureKind::SandboxViolation,
                false,
            ),
            (
                CleaveChildFailureKind::IdleTimeout,
                OperationFailureKind::IdleTimeout,
                true,
            ),
            (
                CleaveChildFailureKind::WallTimeout,
                OperationFailureKind::TimedOut,
                true,
            ),
        ] {
            let mut child = cleave_child("alpha", "failed");
            child.failure_kind = Some(source);
            let projection = OperationWorkbenchProjection::from_cleave(&CleaveProgress {
                active: true,
                run_id: "run-typed".into(),
                total_children: 1,
                completed: 0,
                failed: 1,
                children: vec![child],
                total_tokens_in: 0,
                total_tokens_out: 0,
            });
            let failure = projection.children[0].failure.as_ref().expect("failure");
            assert_eq!(failure.kind, expected);
            assert_eq!(failure.recoverable, recoverable, "{source:?}");
        }
    }

    #[test]
    fn cleave_legacy_upstream_exhausted_status_maps_to_model_failure() {
        let child = cleave_child("alpha", "upstream_exhausted");
        let projection = OperationWorkbenchProjection::from_cleave(&CleaveProgress {
            active: true,
            run_id: "run-legacy".into(),
            total_children: 1,
            completed: 0,
            failed: 1,
            children: vec![child],
            total_tokens_in: 0,
            total_tokens_out: 0,
        });
        let failure = projection.children[0].failure.as_ref().expect("failure");
        assert_eq!(failure.kind, OperationFailureKind::ModelError);
        assert!(failure.recoverable);
    }

    #[test]
    fn operation_child_status_exposes_canonical_labels() {
        assert_eq!(OperationChildStatus::Queued.label(), "queued");
        assert_eq!(OperationChildStatus::Running.label(), "running");
        assert_eq!(OperationChildStatus::Succeeded.label(), "completed");
        assert_eq!(OperationChildStatus::TimedOut.label(), "timed out");
        assert!(OperationChildStatus::Succeeded.is_terminal());
        assert!(OperationChildStatus::Failed.is_terminal());
        assert!(!OperationChildStatus::Running.is_terminal());
        assert!(OperationChildStatus::Failed.is_failure());
        assert!(OperationChildStatus::TimedOut.is_failure());
        assert!(!OperationChildStatus::Cancelled.is_failure());
    }

    #[test]
    fn cleave_progress_maps_to_operation_projection() {
        let mut alpha = cleave_child("alpha", "completed");
        alpha.last_tool = Some("bash".into());
        alpha.last_turn = Some(2);
        alpha.tasks_done = 1;
        alpha.tasks = vec![ChildTaskItem {
            description: "validate".into(),
            done: true,
        }];
        let beta = cleave_child("beta", "failed");

        let projection = OperationWorkbenchProjection::from_cleave(&CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            total_children: 2,
            completed: 1,
            failed: 1,
            children: vec![alpha, beta],
            total_tokens_in: 0,
            total_tokens_out: 0,
        });

        assert_eq!(projection.operation.kind, OperationKind::Cleave);
        assert_eq!(projection.operation.id.as_deref(), Some("run-1"));
        assert_eq!(projection.running, 0);
        assert_eq!(projection.completed, 1);
        assert_eq!(projection.failed, 1);
        assert_eq!(
            projection.children[0].status,
            OperationChildStatus::Succeeded
        );
        assert_eq!(
            projection.children[0].last_activity.as_ref().unwrap().label,
            "bash"
        );
        assert_eq!(projection.children[0].progress.as_ref().unwrap().done, 1);
        assert_eq!(projection.children[1].status, OperationChildStatus::Failed);
        assert!(projection.children[1].failure.is_some());
    }
}

//! Renderer-neutral operation projections for delegate/cleave child work.
//!
//! These DTOs separate operation state from specific renderers. TUI Workbench,
//! transcript milestones, ACP, and future dashboards should consume these
//! projections instead of inferring status from raw tool-call or decomposition
//! event text.

use crate::features::delegate::DelegateProgress;
use omegon_traits::{OperationKind, OperationRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationWorkbenchProjection {
    pub operation: OperationRef,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub children: Vec<OperationChildRow>,
}

impl OperationWorkbenchProjection {
    pub fn from_delegate(progress: &DelegateProgress) -> Self {
        Self {
            operation: OperationRef::delegate("delegate"),
            running: progress.running,
            completed: progress.completed,
            failed: progress.failed,
            children: progress
                .children
                .iter()
                .map(OperationChildRow::from_delegate_child)
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
    pub last_activity: Option<OperationActivity>,
    pub progress: Option<OperationChildProgress>,
    pub failure: Option<OperationFailure>,
}

impl OperationChildRow {
    fn from_delegate_child(child: &crate::features::delegate::DelegateProgressChild) -> Self {
        let status = OperationChildStatus::from_delegate_status(&child.status);
        let failure = match status {
            OperationChildStatus::Failed | OperationChildStatus::TimedOut => Some(
                OperationFailure::from_delegate_summary(child.result_summary.clone()),
            ),
            _ => None,
        };
        Self {
            operation_kind: OperationKind::Delegate,
            id: child.task_id.clone(),
            label: child.label.clone(),
            status,
            status_label: child.status.clone(),
            last_activity: child.last_tool.as_ref().map(|tool| OperationActivity {
                kind: OperationActivityKind::Tool,
                label: tool.clone(),
                turn: child.last_turn,
            }),
            progress: (!child.tasks.is_empty()).then_some(OperationChildProgress {
                done: child.tasks_done,
                total: child.tasks.len(),
            }),
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
    fn from_delegate_status(status: &str) -> Self {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationActivity {
    pub kind: OperationActivityKind,
    pub label: String,
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
    fn from_delegate_summary(summary: Option<String>) -> Self {
        let kind = summary
            .as_deref()
            .map(OperationFailureKind::from_message)
            .unwrap_or(OperationFailureKind::Unknown);
        Self {
            kind,
            message: summary,
            recoverable: matches!(
                kind,
                OperationFailureKind::IdleTimeout | OperationFailureKind::ToolExecutionFailed
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationFailureKind {
    IdleTimeout,
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
    fn from_message(message: &str) -> Self {
        let lower = message.to_ascii_lowercase();
        if lower.contains("idle timeout") || lower.contains("no output") {
            Self::IdleTimeout
        } else if lower.contains("permission") || lower.contains("denied") {
            Self::ToolPermissionDenied
        } else if lower.contains("model") || lower.contains("provider") {
            Self::ModelError
        } else if lower.contains("sandbox") {
            Self::SandboxViolation
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
    use crate::cleave::progress::ChildTaskItem;
    use crate::features::delegate::{DelegateProgress, DelegateProgressChild};

    fn delegate_child(status: &str) -> DelegateProgressChild {
        DelegateProgressChild {
            task_id: "delegate_1".into(),
            label: "delegate_1".into(),
            status: status.into(),
            last_tool: None,
            last_turn: None,
            started_at: None,
            completed_at: None,
            result_summary: None,
            tasks: Vec::new(),
            tasks_done: 0,
        }
    }

    #[test]
    fn delegate_progress_maps_to_operation_projection_counts() {
        let progress = DelegateProgress {
            active: true,
            running: 2,
            completed: 1,
            failed: 1,
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
            children: vec![child],
        };

        let projection = OperationWorkbenchProjection::from_delegate(&progress);
        let row = &projection.children[0];
        assert_eq!(row.last_activity.as_ref().unwrap().label, "bash");
        assert_eq!(row.last_activity.as_ref().unwrap().turn, Some(3));
        assert_eq!(row.progress.as_ref().unwrap().done, 1);
        assert_eq!(row.progress.as_ref().unwrap().total, 2);
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
}

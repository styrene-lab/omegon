//! Renderer-neutral live activity surface projection.
//!
//! The activity surface is a transient, bounded view into work that is
//! happening right now: the current tool card plus active delegate/cleave
//! operation progress. It is intentionally not a durable audit log; transcript
//! segments remain the durable history.

use crate::features::cleave::CleaveProgress;
use crate::features::delegate::DelegateProgress;
use crate::surfaces::layout::UiPresentationLevel;
use crate::surfaces::operations::OperationWorkbenchProjection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySurfaceProjection {
    pub entries: Vec<ActivityEntryProjection>,
}

impl ActivitySurfaceProjection {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn has_tool(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.kind, ActivityEntryKind::Tool))
    }

    pub fn operation_episode_id(&self) -> Option<String> {
        self.entries.iter().find_map(|entry| {
            let operation = entry.operation.as_ref()?;
            let kind = match operation.operation.kind {
                omegon_traits::OperationKind::Delegate => "delegate",
                omegon_traits::OperationKind::Cleave => "cleave",
            };
            let id = operation.operation.id.as_deref().unwrap_or("active");
            Some(format!("{kind}:{id}"))
        })
    }

    pub fn has_operation(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.kind, ActivityEntryKind::Operation))
    }

    pub fn for_level(
        level: UiPresentationLevel,
        tools: Vec<ActivityToolProjection>,
        cleave: Option<&CleaveProgress>,
        delegate: Option<&DelegateProgress>,
    ) -> Self {
        let mut projection = Self::from_parts(tools, cleave, delegate);
        if level == UiPresentationLevel::Om {
            projection.entries = select_primary_om_entry(projection.entries);
        }
        projection
    }

    pub fn from_parts(
        tools: Vec<ActivityToolProjection>,
        cleave: Option<&CleaveProgress>,
        delegate: Option<&DelegateProgress>,
    ) -> Self {
        let mut entries = Vec::new();
        for tool in tools {
            entries.push(ActivityEntryProjection {
                kind: ActivityEntryKind::Tool,
                tool: Some(tool),
                operation: None,
            });
        }
        if let Some(cleave) = cleave.filter(|progress| progress.active) {
            entries.push(ActivityEntryProjection {
                kind: ActivityEntryKind::Operation,
                tool: None,
                operation: Some(OperationWorkbenchProjection::from_cleave(cleave)),
            });
        } else if let Some(delegate) =
            delegate.filter(|progress| progress.active || progress.running > 0)
        {
            entries.push(ActivityEntryProjection {
                kind: ActivityEntryKind::Operation,
                tool: None,
                operation: Some(OperationWorkbenchProjection::from_delegate(delegate)),
            });
        }
        Self { entries }
    }
}

fn select_primary_om_entry(entries: Vec<ActivityEntryProjection>) -> Vec<ActivityEntryProjection> {
    let preferred = entries
        .iter()
        .position(|entry| {
            entry.tool.as_ref().is_some_and(|tool| {
                matches!(
                    tool.status,
                    ActivityToolStatus::Error | ActivityToolStatus::Cancelled
                )
            })
        })
        .or_else(|| {
            entries
                .iter()
                .position(|entry| matches!(entry.kind, ActivityEntryKind::Operation))
        })
        .or_else(|| {
            entries.iter().position(|entry| {
                entry.tool.as_ref().is_some_and(|tool| {
                    matches!(tool.status, ActivityToolStatus::Running)
                })
            })
        })
        .or_else(|| (!entries.is_empty()).then_some(0));
    preferred
        .and_then(|index| entries.into_iter().nth(index))
        .into_iter()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntryProjection {
    pub kind: ActivityEntryKind,
    pub tool: Option<ActivityToolProjection>,
    pub operation: Option<OperationWorkbenchProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityEntryKind {
    Tool,
    Operation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityToolProjection {
    /// Stable episode identity used to hand live work off to durable history.
    pub episode_id: String,
    /// Canonical evidence segment represented by this activity row.
    pub segment_id: String,
    pub mode: ActivityToolMode,
    pub status: ActivityToolStatus,
    pub name: String,
    pub args_summary: Option<String>,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityToolStatus {
    Running,
    Complete,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityToolMode {
    Live,
    Detail,
}

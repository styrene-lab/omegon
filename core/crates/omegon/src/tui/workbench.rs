//! Workbench snapshot surface rendering and hint policy.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::{dashboard, theme};
use crate::features::delegate::DelegateProgress;
use crate::surfaces::operations::{
    OperationChildRow, OperationChildStatus, OperationWorkbenchProjection,
};
use omegon_traits::{PlanLaneProjection, PlanWorkstreamProjection};

pub fn workbench_snapshot_height(snapshot: &PlanDisplaySnapshot, width: u16) -> u16 {
    if width == 0 || snapshot.items.is_empty() {
        return 0;
    }
    let item_count = snapshot.items.len() as u16;
    // Rule/header + every task row. The frame-level layout budget decides how much
    // can actually be shown; do not discard useful plan content here.
    1u16.saturating_add(item_count).max(2)
}

pub fn active_plan_workspace_context_height(state: &WorkbenchState) -> u16 {
    u16::from(state.active.is_some() && state.workspace.has_visible_context())
}

pub fn workbench_preferred_height(state: &WorkbenchState, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    if let Some(active) = state.active.as_ref() {
        workbench_snapshot_height(active, width)
            .saturating_add(active_plan_workspace_context_height(state))
    } else if !state.workstreams.is_empty() || state.workspace.has_visible_context() {
        1
    } else {
        0
    }
}

pub fn activity_preferred_height(
    projection: &crate::surfaces::activity::ActivitySurfaceProjection,
    width: u16,
) -> u16 {
    if width == 0 || projection.is_empty() {
        return 0;
    }
    let tool_count = projection
        .entries
        .iter()
        .filter(|entry| entry.tool.is_some())
        .count() as u16;
    let wants_tool_detail = projection.entries.iter().any(|entry| {
        entry
            .tool
            .as_ref()
            .is_some_and(|tool| should_render_activity_tool_detail(tool, 3))
    });
    let operation_rows = projection
        .entries
        .iter()
        .filter_map(|entry| entry.operation.as_ref())
        .map(|operation| 1u16.saturating_add(operation.children.len() as u16))
        .max()
        .unwrap_or(0);
    match (projection.has_tool(), projection.has_operation()) {
        (true, true) => {
            let tool_height = if wants_tool_detail {
                4
            } else {
                tool_count.max(1)
            };
            tool_height.saturating_add(operation_rows)
        }
        (true, false) if wants_tool_detail => 4,
        (true, false) => tool_count.max(1),
        (false, true) => operation_rows,
        (false, false) => 0,
    }
}

#[derive(Clone, Default)]
pub struct WorkbenchState {
    pub active: Option<PlanDisplaySnapshot>,
    pub workstreams: Vec<WorkstreamSummary>,
    pub workspace: WorkbenchWorkspaceContext,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkbenchWorkspaceContext {
    pub repo: Option<String>,
    pub dir: String,
    pub git_branch: Option<String>,
}

impl WorkbenchWorkspaceContext {
    pub fn has_visible_context(&self) -> bool {
        self.repo.as_ref().is_some_and(|value| !value.is_empty())
            || !self.dir.is_empty()
            || self
                .git_branch
                .as_ref()
                .is_some_and(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkstreamSummary {
    pub id: String,
    pub title: String,
    pub status: WorkstreamStatus,
    pub completed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkstreamStatus {
    Active,
    Paused,
    PendingApproval,
    Waiting,
    Blocked,
    Complete,
}

impl WorkstreamStatus {
    pub fn from_label(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "paused" | "backgrounded" | "detached" => Self::Paused,
            "pending" | "pending_approval" | "review_required" => Self::PendingApproval,
            "blocked" => Self::Blocked,
            "complete" | "completed" => Self::Complete,
            _ => Self::Waiting,
        }
    }
}

impl WorkstreamSummary {
    pub fn from_projection(value: &PlanWorkstreamProjection) -> Option<Self> {
        let id = value.id.trim().to_string();
        if id.is_empty() {
            return None;
        }
        Some(Self {
            id: id.clone(),
            title: if value.title.trim().is_empty() {
                id
            } else {
                value.title.trim().to_string()
            },
            status: WorkstreamStatus::from_label(&value.status),
            completed: value.progress.completed,
            total: value.progress.total,
        })
    }
}

impl WorkbenchState {
    pub fn from_plan_projection(projection: &omegon_traits::PlanSurfaceProjection) -> Self {
        Self {
            active: projection
                .active
                .as_ref()
                .and_then(PlanDisplaySnapshot::from_plan_lane_projection),
            workstreams: projection
                .workstreams
                .iter()
                .filter_map(WorkstreamSummary::from_projection)
                .collect(),
            workspace: WorkbenchWorkspaceContext::default(),
        }
    }

    pub fn from_plan_lane_projection(lane: &PlanLaneProjection) -> Self {
        Self {
            active: PlanDisplaySnapshot::from_plan_lane_projection(lane),
            ..Self::default()
        }
    }
    pub fn merge_workstreams(&mut self, incoming: Vec<WorkstreamSummary>) {
        for stream in incoming {
            if let Some(existing) = self
                .workstreams
                .iter_mut()
                .find(|existing| existing.id == stream.id)
            {
                *existing = stream;
            } else {
                self.workstreams.push(stream);
            }
        }
        self.workstreams.sort_by(|a, b| a.id.cmp(&b.id));
    }

    pub fn merge_workstream_projection(
        &mut self,
        projection: &omegon_traits::PlanSurfaceProjection,
    ) {
        let incoming = projection
            .workstreams
            .iter()
            .filter_map(WorkstreamSummary::from_projection)
            .collect();
        self.merge_workstreams(incoming);
    }

    pub fn is_workstream_only_projection(
        projection: &omegon_traits::PlanSurfaceProjection,
    ) -> bool {
        projection.active.is_none()
            && !projection.workstreams.is_empty()
            && projection.completed_session.is_none()
            && projection.reconciliation_issues.is_empty()
            && projection.promotion_nudges.is_empty()
            && projection.resume_candidates.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDisplaySnapshot {
    pub mode: String,
    pub completed: usize,
    pub total: usize,
    pub items: Vec<PlanDisplayItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDisplayItem {
    pub status: PlanDisplayStatus,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDisplayRow {
    pub text: String,
    pub status: Option<PlanDisplayStatus>,
    pub kind: PlanDisplayRowKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanDisplayRowKind {
    NextAction,
    Normal,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanDisplayStatus {
    Done,
    Active,
    Skipped,
    Todo,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SlimTurnState {
    #[default]
    Ready,
    RequestingProvider,
    OpeningStream,
    UpstreamRetrying(String),
    Thinking,
    Responding,
    Tool(String),
    Interrupting,
    InterruptedKept,
    AbortedForgotten,
    Finished(&'static str),
}

impl WorkstreamStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::PendingApproval => "pending",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Complete => "complete",
        }
    }
}

impl WorkstreamSummary {
    fn progress(&self) -> String {
        if self.total > 0 {
            format!("{}/{}", self.completed, self.total)
        } else {
            "—".to_string()
        }
    }
}

impl SlimTurnState {
    pub fn label(&self) -> String {
        match self {
            Self::Ready => "ready".to_string(),
            Self::RequestingProvider => "waiting: provider request".to_string(),
            Self::OpeningStream => "waiting: stream open".to_string(),
            Self::UpstreamRetrying(detail) => format!("retrying upstream {detail}"),
            Self::Thinking => "streaming thinking".to_string(),
            Self::Responding => "streaming answer".to_string(),
            Self::Tool(name) => format!("running {name}"),
            Self::Interrupting => "interrupting".to_string(),
            Self::InterruptedKept => "interrupted · kept".to_string(),
            Self::AbortedForgotten => "aborted · forgotten".to_string(),
            Self::Finished(reason) => format!("turn {reason}"),
        }
    }
}

pub fn upstream_retry_hint(message: &str) -> Option<String> {
    if !(message.contains("— retrying") || message.starts_with("Retrying")) {
        return None;
    }
    let attempt = message
        .split("attempt ")
        .nth(1)
        .and_then(|rest| rest.split([',', ')']).next())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let delay = message
        .split("delay ")
        .nth(1)
        .and_then(|rest| rest.split("ms").next())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (attempt, delay) {
        (Some(attempt), Some(delay)) => Some(format!("attempt {attempt} · {delay}ms")),
        (Some(attempt), None) => Some(format!("attempt {attempt}")),
        _ => Some("active".to_string()),
    }
}

impl PlanDisplayStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Active => "active",
            Self::Skipped => "skipped",
            Self::Todo => "todo",
        }
    }

    fn from_label(value: &str) -> Self {
        match value {
            "done" | "completed" => Self::Done,
            "active" | "in_progress" | "executing" => Self::Active,
            "skipped" | "skip" => Self::Skipped,
            _ => Self::Todo,
        }
    }

    fn from_work_item_status(value: crate::conversation::WorkItemStatus) -> Self {
        match value {
            crate::conversation::WorkItemStatus::Done => Self::Done,
            crate::conversation::WorkItemStatus::Active => Self::Active,
            crate::conversation::WorkItemStatus::Skipped => Self::Skipped,
            crate::conversation::WorkItemStatus::Pending => Self::Todo,
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Active => "next",
            Self::Skipped => "skip",
            Self::Todo => "todo",
        }
    }

    fn glyph(self) -> char {
        match self {
            Self::Done => '●',
            Self::Active => '◐',
            Self::Skipped => '⊘',
            Self::Todo => '○',
        }
    }

    fn row_style(self, t: &dyn theme::Theme, bg: ratatui::style::Color) -> Style {
        let color = match self {
            Self::Active => t.fg(),
            Self::Todo => t.muted(),
            Self::Done | Self::Skipped => t.dim(),
        };
        Style::default().fg(color).bg(bg)
    }
}

impl PlanDisplayRowKind {
    fn style(self, status: Option<PlanDisplayStatus>, t: &dyn theme::Theme, bg: Color) -> Style {
        match self {
            Self::NextAction => Style::default().fg(t.accent_muted()).bg(bg),
            Self::Overflow => Style::default().fg(t.dim()).bg(bg),
            Self::Normal => status
                .map(|status| status.row_style(t, bg))
                .unwrap_or_else(|| Style::default().fg(t.dim()).bg(bg)),
        }
    }
}

impl PlanDisplaySnapshot {
    pub fn from_plan_lane_projection(lane: &PlanLaneProjection) -> Option<Self> {
        if lane.progress.total == 0 || lane.items.is_empty() {
            return None;
        }
        Some(Self {
            mode: lane.mode.clone(),
            completed: lane.progress.completed,
            total: lane.progress.total,
            items: lane
                .items
                .iter()
                .map(|item| PlanDisplayItem {
                    status: PlanDisplayStatus::from_label(&item.status),
                    description: item.label.clone(),
                })
                .collect(),
        })
    }

    pub fn from_legacy_text(text: &str) -> Option<Self> {
        if text.lines().next() == Some("Plan cleared") {
            return None;
        }
        let mut mode = "unknown".to_string();
        let mut completed = 0usize;
        let mut total = 0usize;
        let mut items = Vec::new();
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("Plan mode:") {
                mode = value
                    .split_whitespace()
                    .next()
                    .unwrap_or("unknown")
                    .to_string();
            } else if let Some(value) = line.strip_prefix("Progress:") {
                if let Some((done, count)) = value.trim().split_once('/') {
                    completed = done.trim().parse().unwrap_or(0);
                    total = count.trim().parse().unwrap_or(0);
                }
            } else if let Some((description, status)) = legacy_plan_item(line) {
                items.push(PlanDisplayItem {
                    status,
                    description,
                });
            }
        }
        if total == 0 {
            total = items.len();
        }
        (!items.is_empty()).then_some(Self {
            mode,
            completed,
            total,
            items,
        })
    }

    pub fn summary(&self) -> String {
        let percent = self.progress_percent();
        format!(
            "plan {} · {}/{} · {percent}%",
            self.mode, self.completed, self.total
        )
    }

    fn progress_percent(&self) -> usize {
        self.completed
            .saturating_mul(100)
            .checked_div(self.total)
            .unwrap_or(0)
            .min(100)
    }

    fn progress_percent_label(&self) -> String {
        format!("{}%", self.progress_percent())
    }

    pub fn system_notification_text(&self, heading: &str) -> String {
        let mut lines = vec![
            heading.to_string(),
            format!("Plan mode: {}", self.mode),
            format!("Progress: {}/{}", self.completed, self.total),
            String::new(),
        ];
        for (idx, item) in self.items.iter().enumerate() {
            lines.push(format!(
                "{}. {} {}",
                idx + 1,
                item.status.glyph(),
                item.description
            ));
        }
        lines.join("\n")
    }

    pub fn is_complete(&self) -> bool {
        self.mode == "complete" || self.completed >= self.total
    }

    pub fn hint_state(&self, plan_area_height: u16) -> SlimPlanHintState {
        if self.is_complete() {
            SlimPlanHintState::Complete
        } else {
            SlimPlanHintState::Active {
                next_visible: self.next_item_visible(plan_area_height),
            }
        }
    }

    fn next_item_visible(&self, plan_area_height: u16) -> bool {
        let max_items = plan_area_height.saturating_sub(1) as usize;
        if max_items == 0 {
            return false;
        }
        let hidden = self.items.len().saturating_sub(max_items);
        let visible_items = if hidden > 0 {
            max_items.saturating_sub(1)
        } else {
            max_items
        };
        let visible = prioritized_plan_item_indices(&self.items, visible_items, hidden > 0);
        self.items
            .iter()
            .position(|item| matches!(item.status, PlanDisplayStatus::Todo))
            .is_some_and(|idx| visible.contains(&idx))
    }
}

pub fn active_workbench_snapshot(
    live_snapshot: Option<&PlanDisplaySnapshot>,
    _legacy_plan_text: Option<&str>,
) -> Option<PlanDisplaySnapshot> {
    // Only the live PlanUpdated projection may drive the Workbench. Legacy
    // transcript text is durable history, not active state; falling back to it
    // resurrects old unfinished plans after branch/session/task changes.
    live_snapshot
        .filter(|snapshot| !snapshot.is_complete())
        .cloned()
}

pub fn workbench_rows(
    snapshot: &PlanDisplaySnapshot,
    width: u16,
    height: u16,
) -> Vec<PlanDisplayRow> {
    let max_items = height.saturating_sub(1) as usize;
    if max_items == 0 {
        return Vec::new();
    }
    let hidden = snapshot.items.len().saturating_sub(max_items);
    let visible_items = if hidden > 0 {
        max_items.saturating_sub(1)
    } else {
        max_items
    };
    let visible = prioritized_plan_item_indices(&snapshot.items, visible_items, hidden > 0);
    let hidden_count = snapshot.items.len().saturating_sub(visible.len());
    let text_budget = width.saturating_sub(2) as usize;
    let mut rows = Vec::new();
    for idx in visible {
        let item = &snapshot.items[idx];
        let line = if item.status == PlanDisplayStatus::Active {
            format!(
                "▶ next  {}/{}  {}",
                idx + 1,
                snapshot.total,
                item.description
            )
        } else {
            let label = item.status.short_label();
            let glyph = item.status.glyph();
            format!("{glyph} {label:<4} {:>2}  {}", idx + 1, item.description)
        };
        rows.push(PlanDisplayRow {
            text: crate::util::truncate(&line, text_budget),
            status: Some(item.status),
            kind: if item.status == PlanDisplayStatus::Active {
                PlanDisplayRowKind::NextAction
            } else {
                PlanDisplayRowKind::Normal
            },
        });
    }
    if hidden_count > 0 {
        rows.push(PlanDisplayRow {
            text: format!("⋯ {hidden_count} hidden"),
            status: None,
            kind: PlanDisplayRowKind::Overflow,
        });
    }
    rows
}

fn prioritized_plan_item_indices(
    items: &[PlanDisplayItem],
    visible_items: usize,
    overflowing: bool,
) -> Vec<usize> {
    if visible_items == 0 {
        return Vec::new();
    }
    if !overflowing {
        return (0..items.len().min(visible_items)).collect();
    }

    let mut selected = Vec::new();
    for status in [
        PlanDisplayStatus::Active,
        PlanDisplayStatus::Todo,
        PlanDisplayStatus::Skipped,
        PlanDisplayStatus::Done,
    ] {
        for (idx, item) in items.iter().enumerate() {
            if item.status == status && !selected.contains(&idx) {
                selected.push(idx);
                if selected.len() == visible_items {
                    selected.sort_unstable();
                    return selected;
                }
            }
        }
    }
    selected.sort_unstable();
    selected
}

fn legacy_plan_item(raw: &str) -> Option<(String, PlanDisplayStatus)> {
    let trimmed = raw.trim_start();
    if !trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }

    if let Some((_, rest)) = trimmed.split_once('●') {
        Some((rest.trim().to_string(), PlanDisplayStatus::Done))
    } else if let Some((_, rest)) = trimmed.split_once('◐') {
        Some((rest.trim().to_string(), PlanDisplayStatus::Active))
    } else if let Some((_, rest)) = trimmed.split_once('⊘') {
        Some((rest.trim().to_string(), PlanDisplayStatus::Skipped))
    } else if let Some((_, rest)) = trimmed.split_once('○') {
        Some((rest.trim().to_string(), PlanDisplayStatus::Todo))
    } else {
        let (_, text) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
        Some((text.trim().to_string(), PlanDisplayStatus::Todo))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlimPlanHintState {
    None,
    Active { next_visible: bool },
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlimPlanContext {
    pub active: bool,
    pub tracked: bool,
    pub openspec_changes: usize,
    pub focused_design: bool,
}

impl SlimPlanContext {
    pub fn from_dashboard(
        active: bool,
        active_changes: &[dashboard::ChangeSummary],
        focused_node: Option<&dashboard::FocusedNodeSummary>,
    ) -> Self {
        Self {
            active,
            tracked: active || !active_changes.is_empty() || focused_node.is_some(),
            openspec_changes: active_changes.len(),
            focused_design: focused_node.is_some(),
        }
    }

    pub fn labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        labels.push(
            if self.active {
                "active plan"
            } else {
                "no active plan"
            }
            .to_string(),
        );
        if self.tracked {
            labels.push("tracked".to_string());
        }
        if self.openspec_changes > 0 {
            labels.push(format!("OpenSpec×{}", self.openspec_changes));
        }
        if self.focused_design {
            labels.push("design-linked".to_string());
        }
        labels
    }
}

pub fn slim_completed_plan_hint_available(completed_plan_history_available: bool) -> bool {
    completed_plan_history_available
}

pub fn slim_operator_hint(
    pending_permission: bool,
    pending_operator_wait: bool,
    terminal_copy_mode: bool,
    plan_state: SlimPlanHintState,
    plan_context: &SlimPlanContext,
) -> String {
    if pending_permission {
        "permission · y once · Shift+A always · n deny".to_string()
    } else if pending_operator_wait {
        "manual wait · Enter done · Esc cancel".to_string()
    } else if terminal_copy_mode {
        "mouse passthrough · terminal drag selects · Ctrl+Shift+T restores app mouse".to_string()
    } else {
        match plan_state {
            SlimPlanHintState::Active { next_visible: true } => {
                let mut labels = vec!["plan active".to_string()];
                labels.extend(plan_context.labels());
                labels.join(" · ")
            }
            SlimPlanHintState::Active {
                next_visible: false,
            } => {
                let mut labels = vec!["plan active".to_string(), "next below".to_string()];
                labels.extend(plan_context.labels());
                labels.join(" · ")
            }
            SlimPlanHintState::Complete => "plan complete · history available".to_string(),
            SlimPlanHintState::None => "transcript live".to_string(),
        }
    }
}

pub fn render_workbench_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    state: &WorkbenchState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if let Some(snapshot) = state.active.as_ref() {
        let context_height = active_plan_workspace_context_height(state).min(area.height);
        if context_height > 0 {
            render_workspace_context_panel(
                Rect::new(area.x, area.y, area.width, context_height),
                frame,
                t,
                state,
            );
        }
        let plan_area = Rect::new(
            area.x,
            area.y.saturating_add(context_height),
            area.width,
            area.height.saturating_sub(context_height),
        );
        render_active_workbench_panel(plan_area, frame, t, snapshot, state.workstreams.len());
    } else if !state.workstreams.is_empty() {
        render_workstream_summary(area, frame, t, state.workstreams.as_slice(), t.surface_bg());
    } else {
        render_workspace_context_panel(area, frame, t, state);
    }
}

pub fn render_activity_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    conversation: &crate::tui::conversation::ConversationView,
    projection: &crate::surfaces::activity::ActivitySurfaceProjection,
) {
    if area.width == 0 || area.height == 0 || projection.is_empty() {
        return;
    }

    let tools = projection
        .entries
        .iter()
        .filter_map(|entry| entry.tool.as_ref())
        .collect::<Vec<_>>();
    let operation = projection
        .entries
        .iter()
        .find_map(|entry| entry.operation.as_ref());

    match (tools.as_slice(), operation) {
        ([tool], Some(operation)) => {
            let render_detail = should_render_activity_tool_detail(tool, area.height);
            let tool_height = if render_detail { area.height.min(4) } else { 1 };
            let tool_area = Rect::new(area.x, area.y, area.width, tool_height);
            if render_detail {
                if let Some(segment) = conversation.tool_segment_by_id(&tool.segment_id) {
                    crate::tui::segment_detail::render_tool_card(
                        tool_area,
                        frame.buffer_mut(),
                        t,
                        segment,
                        activity_tool_mode(tool.mode),
                    );
                } else {
                    render_activity_tool_rows(tool_area, frame, t, std::slice::from_ref(tool));
                }
            } else {
                render_activity_tool_rows(tool_area, frame, t, std::slice::from_ref(tool));
            }
            if area.height > tool_height {
                let op_area = Rect::new(
                    area.x,
                    area.y.saturating_add(tool_height),
                    area.width,
                    area.height.saturating_sub(tool_height),
                );
                render_operation_workbench_panel(op_area, frame, t, operation);
            }
        }
        ([tool], None) if should_render_activity_tool_detail(tool, area.height) => {
            if let Some(segment) = conversation.tool_segment_by_id(&tool.segment_id) {
                crate::tui::segment_detail::render_tool_card(
                    area,
                    frame.buffer_mut(),
                    t,
                    segment,
                    activity_tool_mode(tool.mode),
                );
            }
        }
        ([tool], None) => {
            render_activity_tool_rows(area, frame, t, std::slice::from_ref(tool));
        }
        ([], Some(operation)) => render_operation_workbench_panel(area, frame, t, operation),
        ([], None) => {}
        (tools, operation) => {
            let tool_rows = if operation.is_some() {
                area.height.saturating_sub(3).max(1)
            } else {
                area.height
            };
            let tool_area = Rect::new(area.x, area.y, area.width, tool_rows);
            render_activity_tool_rows(tool_area, frame, t, tools);
            if let Some(operation) = operation
                && area.height > tool_rows
            {
                let op_area = Rect::new(
                    area.x,
                    area.y.saturating_add(tool_rows),
                    area.width,
                    area.height.saturating_sub(tool_rows),
                );
                render_operation_workbench_panel(op_area, frame, t, operation);
            }
        }
    }
}

fn should_render_activity_tool_detail(
    tool: &crate::surfaces::activity::ActivityToolProjection,
    height: u16,
) -> bool {
    height >= 3
        && match tool.mode {
            crate::surfaces::activity::ActivityToolMode::Detail => true,
            crate::surfaces::activity::ActivityToolMode::Live => matches!(
                tool.status,
                crate::surfaces::activity::ActivityToolStatus::Running
                    | crate::surfaces::activity::ActivityToolStatus::Error
            ),
        }
}

fn render_activity_tool_rows(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    tools: &[&crate::surfaces::activity::ActivityToolProjection],
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bg = t.surface_bg();
    let max_rows = area.height as usize;
    let lines = tools
        .iter()
        .take(max_rows)
        .map(|tool| {
            Line::from(Span::styled(
                activity_tool_row_text(tool, area.width),
                activity_tool_status_style(tool.status, t, bg),
            ))
        })
        .collect::<Vec<_>>();
    Paragraph::new(lines)
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, frame.buffer_mut());
}

fn activity_tool_row_text(
    tool: &crate::surfaces::activity::ActivityToolProjection,
    width: u16,
) -> String {
    let name = tool.name.as_str();
    let args = tool.args_summary.as_deref().unwrap_or("");
    let result = tool.result_summary.as_deref().unwrap_or("");
    let state = match tool.status {
        crate::surfaces::activity::ActivityToolStatus::Running => "run",
        crate::surfaces::activity::ActivityToolStatus::Complete => "done",
        crate::surfaces::activity::ActivityToolStatus::Error => "fail",
        crate::surfaces::activity::ActivityToolStatus::Cancelled => "stop",
    };
    let detail = [args, result]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let text = if detail.is_empty() {
        format!("{state:<4} {name}")
    } else {
        format!("{state:<4} {name} · {detail}")
    };
    crate::util::truncate(&text, width.saturating_sub(1) as usize)
}

fn activity_tool_status_style(
    status: crate::surfaces::activity::ActivityToolStatus,
    t: &dyn theme::Theme,
    bg: Color,
) -> Style {
    let fg = match status {
        crate::surfaces::activity::ActivityToolStatus::Running => t.accent_muted(),
        crate::surfaces::activity::ActivityToolStatus::Complete => t.muted(),
        crate::surfaces::activity::ActivityToolStatus::Error => t.error(),
        crate::surfaces::activity::ActivityToolStatus::Cancelled => t.dim(),
    };
    Style::default().fg(fg).bg(bg)
}

fn activity_tool_mode(
    mode: crate::surfaces::activity::ActivityToolMode,
) -> crate::tui::segment_detail::ToolDetailMode {
    match mode {
        crate::surfaces::activity::ActivityToolMode::Live => {
            crate::tui::segment_detail::ToolDetailMode::Live
        }
        crate::surfaces::activity::ActivityToolMode::Detail => {
            crate::tui::segment_detail::ToolDetailMode::Detail
        }
    }
}

fn render_workspace_context_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    state: &WorkbenchState,
) {
    let bg = t.surface_bg();
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(
        " workbench",
        Style::default().fg(t.accent_muted()).bg(bg),
    ));

    if let Some(repo) = state
        .workspace
        .repo
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        spans.push(Span::styled(" · ", Style::default().fg(t.dim()).bg(bg)));
        spans.push(Span::styled(
            format!(
                "{} {repo}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Repo)
            ),
            Style::default().fg(t.muted()).bg(bg),
        ));
    }
    let show_dir = !state.workspace.dir.is_empty()
        && state.workspace.repo.as_deref() != Some(state.workspace.dir.as_str());
    if show_dir {
        spans.push(Span::styled(" · ", Style::default().fg(t.dim()).bg(bg)));
        spans.push(Span::styled(
            format!(
                "{} {}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Directory),
                state.workspace.dir
            ),
            Style::default().fg(t.muted()).bg(bg),
        ));
    }
    if let Some(branch) = state
        .workspace
        .git_branch
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        spans.push(Span::styled(" · ", Style::default().fg(t.dim()).bg(bg)));
        spans.push(Span::styled(
            format!(
                "{} {branch}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Branch)
            ),
            Style::default().fg(t.muted()).bg(bg),
        ));
    }

    if state.workspace.has_visible_context() {
        let used = spans.iter().map(|span| span.width()).sum::<usize>() as u16;
        if area.width > used.saturating_add(1) {
            spans.push(Span::styled(
                format!(
                    " {}",
                    "─".repeat(area.width.saturating_sub(used + 1) as usize)
                ),
                Style::default().fg(t.border_dim()).bg(bg),
            ));
        }
    }

    Paragraph::new(Line::from(spans))
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, frame.buffer_mut());
}

fn render_active_workbench_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    snapshot: &PlanDisplaySnapshot,
    _workstream_count: usize,
) {
    let bg = t.surface_bg();
    let mut lines: Vec<Line<'_>> = Vec::new();
    let heading = snapshot.summary();
    let rule_width = area.width.saturating_sub(heading.len() as u16 + 4) as usize;
    lines.push(Line::from(vec![
        Span::styled("─ ", Style::default().fg(t.border_dim()).bg(bg)),
        Span::styled("plan ", Style::default().fg(t.dim()).bg(bg)),
        Span::styled(
            snapshot.mode.clone(),
            Style::default()
                .fg(t.accent_muted())
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(t.dim()).bg(bg)),
        Span::styled(
            format!("{}/{}", snapshot.completed, snapshot.total),
            Style::default().fg(t.muted()).bg(bg),
        ),
        Span::styled(" · ", Style::default().fg(t.dim()).bg(bg)),
        Span::styled(
            snapshot.progress_percent_label(),
            Style::default().fg(t.muted()).bg(bg),
        ),
        Span::styled(
            format!(" {}", "─".repeat(rule_width)),
            Style::default().fg(t.border_dim()).bg(bg),
        ),
    ]));

    for row in workbench_rows(snapshot, area.width, area.height) {
        let style = row.kind.style(row.status, t, bg);
        lines.push(Line::from(Span::styled(row.text, style)));
    }

    Paragraph::new(lines)
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, frame.buffer_mut());
}

fn render_delegate_workbench_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    progress: &DelegateProgress,
) {
    let projection = OperationWorkbenchProjection::from_delegate(progress);
    render_operation_workbench_panel(area, frame, t, &projection);
}

fn render_operation_workbench_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    projection: &OperationWorkbenchProjection,
) {
    let bg = t.surface_bg();
    let kind = match projection.operation.kind {
        omegon_traits::OperationKind::Delegate => "delegate",
        omegon_traits::OperationKind::Cleave => "cleave",
    };
    let mut lines = vec![workbench_rule_line(
        t,
        bg,
        if projection.pending_results > 0 {
            format!(
                "{kind} running {} · done {} · failed {} · pending results {}",
                projection.running,
                projection.completed,
                projection.failed,
                projection.pending_results
            )
        } else {
            format!(
                "{kind} running {} · done {} · failed {}",
                projection.running, projection.completed, projection.failed
            )
        },
        area.width,
    )];
    let max_rows = area.height.saturating_sub(1) as usize;
    let visible = prioritized_operation_child_indices(&projection.children, max_rows);
    let hidden_count = projection.children.len().saturating_sub(visible.len());
    for idx in visible {
        let child = &projection.children[idx];
        let text = operation_worker_chrome_line(child, area.width);
        lines.push(Line::from(Span::styled(
            text,
            operation_worker_status_style(child.status, t, bg),
        )));
    }
    if hidden_count > 0 && lines.len() < area.height as usize {
        lines.push(Line::from(Span::styled(
            format!("⋯ {hidden_count} hidden"),
            Style::default().fg(t.dim()).bg(bg),
        )));
    }
    Paragraph::new(lines)
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, frame.buffer_mut());
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkerChromeRowProjection {
    state: String,
    label: String,
    status: String,
    tool: Option<String>,
    detail: Option<String>,
}

impl WorkerChromeRowProjection {
    fn from_operation_child(child: &OperationChildRow) -> Self {
        let task_progress = child
            .progress
            .as_ref()
            .map(|progress| format!("tasks {}/{}", progress.done, progress.total));
        let result_hint = if !child.result_viewed
            && !matches!(
                child.status,
                OperationChildStatus::Running
                    | OperationChildStatus::Queued
                    | OperationChildStatus::Starting
                    | OperationChildStatus::Waiting
            ) {
            Some(format!(
                "result ready: delegate_result {{\"task_id\": \"{}\"}}",
                child.id
            ))
        } else {
            None
        };
        let failure = child
            .failure
            .as_ref()
            .and_then(|failure| failure.message.as_deref())
            .map(str::to_string);
        let detail = [task_progress, result_hint, failure]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let last_tool = child.last_activity.as_ref().filter(|activity| {
            matches!(
                activity.kind,
                crate::surfaces::operations::OperationActivityKind::Tool
            )
        });
        Self::new(
            &child.label,
            operation_child_workbench_status_label(child.status, child.operation_kind),
            last_tool,
            (!detail.is_empty()).then_some(detail),
        )
    }

    fn new(
        label: &str,
        status: &str,
        last_tool: Option<&crate::surfaces::operations::OperationActivity>,
        detail: Option<String>,
    ) -> Self {
        let glyphs = crate::tui::glyphs::glyphs();
        let state_glyph = glyphs.tool_state(crate::tui::glyphs::tool_state_role_for_status(status));
        let tool = last_tool.map(|activity| {
            let identity = crate::surfaces::conversation::tool_visual_identity(
                &activity.label,
                activity.args_summary.as_deref(),
            );
            let category = glyphs.tool_category(
                crate::tui::glyphs::tool_category_role_for_identity(&identity),
            );
            format!("{category} {}", identity.label)
        });
        Self {
            state: state_glyph.to_string(),
            label: label.to_string(),
            status: status.to_string(),
            tool,
            detail,
        }
    }

    fn inline_row(&self) -> crate::surfaces::inline::InlineRow<String> {
        crate::surfaces::inline::InlineRow::new(
            vec![
                crate::surfaces::inline::InlineCell::new(
                    format!("{} {}", self.state, self.label),
                    crate::surfaces::inline::InlineCellRole::Status,
                ),
                crate::surfaces::inline::InlineCell::new(
                    self.status.clone(),
                    crate::surfaces::inline::InlineCellRole::Value,
                ),
            ],
            self.tool
                .iter()
                .cloned()
                .chain(self.detail.iter().cloned())
                .map(|cell| {
                    crate::surfaces::inline::InlineCell::new(
                        cell,
                        crate::surfaces::inline::InlineCellRole::Metadata,
                    )
                })
                .collect(),
        )
    }
}

fn prioritized_operation_child_indices(
    children: &[OperationChildRow],
    max_rows: usize,
) -> Vec<usize> {
    if max_rows == 0 || children.is_empty() {
        return Vec::new();
    }
    let visible_rows = if children.len() > max_rows {
        max_rows.saturating_sub(1)
    } else {
        max_rows
    };
    if visible_rows == 0 {
        return Vec::new();
    }

    let mut indices = Vec::new();
    let passes: &[fn(&OperationChildRow) -> bool] = &[
        |child| {
            matches!(
                child.status,
                OperationChildStatus::Running | OperationChildStatus::Starting
            )
        },
        |child| !child.result_viewed && child.status.is_terminal(),
        |child| {
            matches!(
                child.status,
                OperationChildStatus::Queued | OperationChildStatus::Waiting
            )
        },
        |child| child.status.is_terminal(),
        |_| true,
    ];
    for pass in passes {
        for (idx, child) in children.iter().enumerate() {
            if indices.len() >= visible_rows {
                break;
            }
            if !indices.contains(&idx) && pass(child) {
                indices.push(idx);
            }
        }
    }
    // Keep priority order rather than spawn order: constrained Workbench space
    // should surface active/result-ready delegates before older low-signal rows.
    indices
}

fn operation_child_workbench_status_label(
    status: OperationChildStatus,
    kind: omegon_traits::OperationKind,
) -> &'static str {
    match (kind, status) {
        (omegon_traits::OperationKind::Delegate, OperationChildStatus::Running) => "proceeding",
        (omegon_traits::OperationKind::Delegate, OperationChildStatus::Succeeded) => "done",
        _ => status.label(),
    }
}

fn operation_worker_chrome_line(child: &OperationChildRow, width: u16) -> String {
    let row = WorkerChromeRowProjection::from_operation_child(child).inline_row();
    crate::tui::inline_render::render_inline_text_row(&row, width.saturating_sub(1))
}

fn operation_worker_status_style(
    status: OperationChildStatus,
    t: &dyn theme::Theme,
    bg: ratatui::style::Color,
) -> Style {
    match status {
        OperationChildStatus::Succeeded => Style::default().fg(t.success()).bg(bg),
        OperationChildStatus::Running | OperationChildStatus::Starting => {
            Style::default().fg(t.warning()).bg(bg)
        }
        OperationChildStatus::Failed | OperationChildStatus::TimedOut => {
            Style::default().fg(t.error()).bg(bg)
        }
        _ => Style::default().fg(t.accent_muted()).bg(bg),
    }
}

fn worker_chrome_line(
    label: &str,
    status: &str,
    last_tool: Option<&crate::surfaces::operations::OperationActivity>,
    task_progress: &str,
    width: u16,
) -> String {
    let detail =
        (!task_progress.is_empty()).then(|| task_progress.trim_start_matches(" · ").to_string());
    let row = WorkerChromeRowProjection::new(label, status, last_tool, detail).inline_row();
    crate::tui::inline_render::render_inline_text_row(&row, width.saturating_sub(1))
}

fn project_worker_chrome_row(
    label: &str,
    status: &str,
    last_tool: Option<&crate::surfaces::operations::OperationActivity>,
    task_progress: &str,
) -> crate::surfaces::inline::InlineRow<String> {
    let detail =
        (!task_progress.is_empty()).then(|| task_progress.trim_start_matches(" · ").to_string());
    WorkerChromeRowProjection::new(label, status, last_tool, detail).inline_row()
}

fn workbench_rule_line<'a>(
    t: &dyn theme::Theme,
    bg: ratatui::style::Color,
    summary: String,
    width: u16,
) -> Line<'a> {
    crate::tui::horizontal_line::horizontal_line(
        crate::tui::horizontal_line::HorizontalLineSpec::title(summary)
            .with_title_emphasis(crate::tui::horizontal_line::LineEmphasis::Strong),
        width,
        t,
        bg,
    )
}

fn worker_status_style(status: &str, t: &dyn theme::Theme, bg: ratatui::style::Color) -> Style {
    let fg = match status {
        "completed" | "merged_after_failure" => t.success(),
        "running" => t.warning(),
        "failed" | "upstream_exhausted" => t.error(),
        _ => t.accent_muted(),
    };
    Style::default().fg(fg).bg(bg)
}

fn render_workstream_summary(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    workstreams: &[WorkstreamSummary],
    bg: ratatui::style::Color,
) {
    if workstreams.is_empty() {
        return;
    }
    let mut text = format!(" workstreams×{}", workstreams.len());
    if let Some(first) = workstreams.first() {
        text.push_str(&format!(
            " · {} {} {}",
            first.status.label(),
            first.progress(),
            first.title
        ));
    }
    Paragraph::new(Line::from(Span::styled(
        crate::util::truncate(&text, area.width as usize),
        Style::default().fg(t.accent_muted()).bg(bg),
    )))
    .style(Style::default().bg(bg))
    .render(area, frame.buffer_mut());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_worker_projection_maps_structured_child_row() {
        let child = OperationChildRow {
            operation_kind: omegon_traits::OperationKind::Delegate,
            id: "task-1".into(),
            label: "delegate-1".into(),
            route_decision: None,
            status: OperationChildStatus::Running,
            status_label: "running".into(),
            last_activity: Some(crate::surfaces::operations::OperationActivity {
                kind: crate::surfaces::operations::OperationActivityKind::Tool,
                label: "bash".into(),
                args_summary: Some("cargo test -p omegon".into()),
                turn: Some(3),
            }),
            progress: Some(crate::surfaces::operations::OperationChildProgress {
                done: 1,
                total: 4,
            }),
            result_summary: None,
            failure: None,
            result_viewed: true,
        };

        let row = WorkerChromeRowProjection::from_operation_child(&child);
        assert_eq!(row.label, "delegate-1");
        assert_eq!(row.status, "proceeding");
        assert!(
            row.tool
                .as_deref()
                .is_some_and(|tool| tool.contains("cargo")),
            "{row:?}"
        );
        assert_eq!(row.detail.as_deref(), Some("tasks 1/4"));
    }

    #[test]
    fn delegate_completed_worker_projection_uses_done_label() {
        let child = OperationChildRow {
            operation_kind: omegon_traits::OperationKind::Delegate,
            id: "task-2".into(),
            label: "verify/tests".into(),
            route_decision: None,
            status: OperationChildStatus::Succeeded,
            status_label: "completed".into(),
            last_activity: None,
            progress: None,
            result_summary: Some("validated".into()),
            failure: None,
            result_viewed: false,
        };

        let row = WorkerChromeRowProjection::from_operation_child(&child);

        assert_eq!(row.label, "verify/tests");
        assert_eq!(row.status, "done");
        assert!(
            row.detail
                .as_deref()
                .is_some_and(|detail| detail
                    .contains("result ready: delegate_result {\"task_id\": \"task-2\"}")),
            "unviewed terminal delegate should expose tool-call-shaped result detail: {row:?}"
        );
    }

    #[test]
    fn constrained_delegate_rows_prefer_active_and_unviewed_terminal_results() {
        let children = vec![
            OperationChildRow {
                operation_kind: omegon_traits::OperationKind::Delegate,
                id: "old-done".into(),
                label: "old-done".into(),
                route_decision: None,
                status: OperationChildStatus::Succeeded,
                status_label: "completed".into(),
                result_viewed: true,
                last_activity: None,
                progress: None,
                result_summary: None,
                failure: None,
            },
            OperationChildRow {
                operation_kind: omegon_traits::OperationKind::Delegate,
                id: "running".into(),
                label: "running".into(),
                route_decision: None,
                status: OperationChildStatus::Running,
                status_label: "running".into(),
                result_viewed: true,
                last_activity: None,
                progress: None,
                result_summary: None,
                failure: None,
            },
            OperationChildRow {
                operation_kind: omegon_traits::OperationKind::Delegate,
                id: "ready".into(),
                label: "ready".into(),
                route_decision: None,
                status: OperationChildStatus::Succeeded,
                status_label: "completed".into(),
                result_viewed: false,
                last_activity: None,
                progress: None,
                result_summary: Some("ready".into()),
                failure: None,
            },
            OperationChildRow {
                operation_kind: omegon_traits::OperationKind::Delegate,
                id: "queued".into(),
                label: "queued".into(),
                route_decision: None,
                status: OperationChildStatus::Queued,
                status_label: "queued".into(),
                result_viewed: true,
                last_activity: None,
                progress: None,
                result_summary: None,
                failure: None,
            },
        ];

        let visible = prioritized_operation_child_indices(&children, 3);

        assert_eq!(visible, vec![1, 2]);
    }

    #[test]
    fn hidden_delegate_rows_render_hidden_count() {
        let child = |id: &str| OperationChildRow {
            operation_kind: omegon_traits::OperationKind::Delegate,
            id: id.into(),
            label: id.into(),
            route_decision: None,
            status: OperationChildStatus::Succeeded,
            status_label: "completed".into(),
            result_viewed: true,
            last_activity: None,
            progress: None,
            result_summary: None,
            failure: None,
        };
        let projection = OperationWorkbenchProjection {
            operation: omegon_traits::OperationRef::delegate("delegate"),
            running: 0,
            completed: 3,
            failed: 0,
            pending_results: 0,
            children: vec![child("done-1"), child("done-2"), child("done-3")],
        };
        let backend = ratatui::backend::TestBackend::new(80, 3);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_operation_workbench_panel(
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                    &projection,
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..3 {
            for x in 0..80 {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }

        assert!(rendered.contains("⋯ 2 hidden"), "{rendered}");
    }

    #[test]
    fn operation_worker_projection_combines_progress_and_failure_detail() {
        let child = OperationChildRow {
            operation_kind: omegon_traits::OperationKind::Delegate,
            id: "task-2".into(),
            label: "delegate-2".into(),
            route_decision: None,
            status: OperationChildStatus::Failed,
            status_label: "failed".into(),
            last_activity: None,
            progress: Some(crate::surfaces::operations::OperationChildProgress {
                done: 2,
                total: 5,
            }),
            result_summary: None,
            failure: Some(crate::surfaces::operations::OperationFailure {
                kind: crate::surfaces::operations::OperationFailureKind::ToolExecutionFailed,
                message: Some("validator failed".into()),
                recoverable: true,
            }),
            result_viewed: true,
        };

        let row = WorkerChromeRowProjection::from_operation_child(&child);
        assert_eq!(row.status, "failed");
        assert!(row.tool.is_none());
        assert_eq!(row.detail.as_deref(), Some("tasks 2/5 · validator failed"));
    }

    #[test]
    fn worker_chrome_projection_separates_identity_from_metadata() {
        let activity = crate::surfaces::operations::OperationActivity {
            kind: crate::surfaces::operations::OperationActivityKind::Tool,
            label: "bash".into(),
            args_summary: Some("cargo test -p omegon".into()),
            turn: None,
        };
        let row =
            project_worker_chrome_row("delegate-1", "running", Some(&activity), " · tasks 1/3");

        assert_eq!(row.left.len(), 2);
        assert!(row.left[0].text.contains("delegate-1"));
        assert_eq!(row.left[1].text, "running");
        assert_eq!(row.right.len(), 2);
        assert!(row.right[0].text.contains("cargo"));
        assert_eq!(row.right[1].text, "tasks 1/3");
    }

    #[test]
    fn worker_chrome_line_preserves_right_metadata_when_truncated() {
        let rendered = worker_chrome_line(
            "very-long-worker-label-that-will-overflow",
            "running",
            Some(&crate::surfaces::operations::OperationActivity {
                kind: crate::surfaces::operations::OperationActivityKind::Tool,
                label: "bash".into(),
                args_summary: Some("cargo test -p omegon".into()),
                turn: None,
            }),
            " · tasks 1/3",
            42,
        );

        assert!(rendered.contains("tasks 1/3"), "{rendered}");
    }

    #[test]
    fn workbench_height_includes_workspace_context_without_active_work() {
        let state = WorkbenchState {
            workspace: WorkbenchWorkspaceContext {
                repo: Some("omegon-secundus".to_string()),
                dir: "omegon-secundus".to_string(),
                git_branch: Some("feature/ui-improvements-polish".to_string()),
            },
            ..WorkbenchState::default()
        };

        assert_eq!(workbench_preferred_height(&state, 120), 1);
    }

    #[test]
    fn workbench_height_expands_for_all_plan_items() {
        let snapshot = PlanDisplaySnapshot {
            mode: "executing".into(),
            completed: 1,
            total: 8,
            items: (0..8)
                .map(|index| PlanDisplayItem {
                    status: if index == 1 {
                        PlanDisplayStatus::Active
                    } else {
                        PlanDisplayStatus::Todo
                    },
                    description: format!("Task {index}"),
                })
                .collect(),
        };

        assert_eq!(workbench_snapshot_height(&snapshot, 120), 9);
    }

    #[test]
    fn workbench_height_stacks_workspace_context_above_active_plan() {
        let state = WorkbenchState {
            active: Some(PlanDisplaySnapshot {
                mode: "executing".into(),
                completed: 0,
                total: 1,
                items: vec![PlanDisplayItem {
                    status: PlanDisplayStatus::Active,
                    description: "Patch layout".into(),
                }],
            }),
            workspace: WorkbenchWorkspaceContext {
                repo: Some("omegon-secundus".to_string()),
                dir: "omegon-secundus".to_string(),
                git_branch: Some("feature/ui-improvements-polish".to_string()),
            },
            ..WorkbenchState::default()
        };

        assert_eq!(workbench_preferred_height(&state, 120), 3);
    }

    #[test]
    fn active_plan_without_workspace_context_uses_full_height() {
        let state = WorkbenchState {
            active: Some(PlanDisplaySnapshot {
                mode: "executing".into(),
                completed: 0,
                total: 2,
                items: vec![
                    PlanDisplayItem {
                        status: PlanDisplayStatus::Active,
                        description: "Inspect layout".into(),
                    },
                    PlanDisplayItem {
                        status: PlanDisplayStatus::Todo,
                        description: "Patch layout".into(),
                    },
                ],
            }),
            ..WorkbenchState::default()
        };
        let backend = ratatui::backend::TestBackend::new(80, 3);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_workbench_panel(frame.area(), frame, &super::super::theme::Alpharius, &state)
            })
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Inspect layout"), "{rendered}");
        assert!(rendered.contains("Patch layout"), "{rendered}");
    }

    #[test]
    fn workbench_state_projects_typed_plan_lane_without_json() {
        let lane = PlanLaneProjection {
            plan_id: "session:current".into(),
            mode: "executing".into(),
            guidance: "keep working".into(),
            status: "active".into(),
            scope: "session".into(),
            source: "session".into(),
            progress: omegon_traits::PlanProgressProjection {
                completed: 1,
                total: 2,
            },
            items: vec![
                omegon_traits::PlanItemProjection {
                    label: "done task".into(),
                    status: "done".into(),
                    ..Default::default()
                },
                omegon_traits::PlanItemProjection {
                    label: "active task".into(),
                    status: "active".into(),
                    ..Default::default()
                },
            ],
        };

        let projection = omegon_traits::PlanSurfaceProjection {
            active: Some(lane),
            workstreams: vec![omegon_traits::PlanWorkstreamProjection {
                id: "openspec:demo".into(),
                title: "demo change".into(),
                status: "paused".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 3,
                    total: 5,
                },
            }],
            ..Default::default()
        };

        let state = WorkbenchState::from_plan_projection(&projection);
        let active = state.active.expect("active lane should project");
        assert_eq!(active.mode, "executing");
        assert_eq!(active.completed, 1);
        assert_eq!(active.total, 2);
        assert_eq!(active.items.len(), 2);
        assert_eq!(active.items[0].status, PlanDisplayStatus::Done);
        assert_eq!(active.items[1].status, PlanDisplayStatus::Active);
        assert_eq!(active.items[1].description, "active task");
        assert_eq!(state.workstreams.len(), 1);
        assert_eq!(state.workstreams[0].id, "openspec:demo");
        assert_eq!(state.workstreams[0].status, WorkstreamStatus::Paused);
        assert_eq!(state.workstreams[0].progress(), "3/5");
    }

    #[test]
    fn workbench_state_ignores_empty_typed_plan_lane() {
        let lane = PlanLaneProjection {
            plan_id: "session:current".into(),
            mode: "off".into(),
            guidance: "none".into(),
            status: "detached".into(),
            scope: "session".into(),
            source: "session".into(),
            progress: omegon_traits::PlanProgressProjection {
                completed: 0,
                total: 0,
            },
            items: Vec::new(),
        };

        let state = WorkbenchState::from_plan_lane_projection(&lane);
        assert!(state.active.is_none());
        assert!(state.workstreams.is_empty());
    }

    #[test]
    fn workspace_context_uses_tui_workspace_glyphs() {
        let repo = "repo-name";
        let cwd = "nested-dir";
        let branch = "branch/name";
        let state = WorkbenchState {
            workspace: WorkbenchWorkspaceContext {
                repo: Some(repo.to_string()),
                dir: cwd.to_string(),
                git_branch: Some(branch.to_string()),
            },
            ..WorkbenchState::default()
        };

        let backend = ratatui::backend::TestBackend::new(120, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_workspace_context_panel(
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                    &state,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for x in 0..120 {
            text.push_str(buf[(x, 0)].symbol());
        }

        assert!(
            text.contains(&format!(
                "{} {repo}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Repo)
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "{} {cwd}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Directory)
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "{} {branch}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Branch)
            )),
            "{text}"
        );
        assert!(!text.contains(&format!("repo {repo}")), "{text}");
        assert!(!text.contains(&format!("dir {cwd}")), "{text}");
        assert!(!text.contains(&format!("git {branch}")), "{text}");
    }

    #[test]
    fn workspace_context_omits_duplicate_dir_when_it_matches_repo() {
        let repo = "same-name";
        let branch = "branch/name";
        let state = WorkbenchState {
            workspace: WorkbenchWorkspaceContext {
                repo: Some(repo.to_string()),
                dir: repo.to_string(),
                git_branch: Some(branch.to_string()),
            },
            ..WorkbenchState::default()
        };

        let backend = ratatui::backend::TestBackend::new(120, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_workspace_context_panel(
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                    &state,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for x in 0..120 {
            text.push_str(buf[(x, 0)].symbol());
        }

        assert!(
            text.contains(&format!(
                "{} {repo}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Repo)
            )),
            "{text}"
        );
        assert!(
            !text.contains(&format!(
                "{} {repo}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Directory)
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "{} {branch}",
                crate::tui::glyphs::glyphs()
                    .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Branch)
            )),
            "{text}"
        );
    }

    #[test]
    fn activity_tool_detail_requires_room_and_active_or_detail_status() {
        let running = crate::surfaces::activity::ActivityToolProjection {
            episode_id: "turn:1".to_string(),
            segment_id: "tool-1".to_string(),
            mode: crate::surfaces::activity::ActivityToolMode::Live,
            status: crate::surfaces::activity::ActivityToolStatus::Running,
            name: "bash".to_string(),
            args_summary: None,
            result_summary: None,
        };
        let mut complete = running.clone();
        complete.status = crate::surfaces::activity::ActivityToolStatus::Complete;
        let mut error = running.clone();
        error.status = crate::surfaces::activity::ActivityToolStatus::Error;
        error.mode = crate::surfaces::activity::ActivityToolMode::Detail;

        assert!(should_render_activity_tool_detail(&running, 3));
        assert!(!should_render_activity_tool_detail(&running, 2));
        assert!(!should_render_activity_tool_detail(&complete, 4));
        assert!(should_render_activity_tool_detail(&error, 4));
    }

    #[test]
    fn empty_workbench_without_workspace_context_has_no_height() {
        assert_eq!(
            workbench_preferred_height(&WorkbenchState::default(), 120),
            0
        );
    }
}

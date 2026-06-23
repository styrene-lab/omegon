//! Workbench snapshot surface rendering and hint policy.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::{dashboard, theme};
use crate::features::cleave::CleaveProgress;
use crate::features::delegate::DelegateProgress;
use crate::surfaces::operations::{
    OperationChildRow, OperationChildStatus, OperationWorkbenchProjection,
};

pub fn workbench_snapshot_height(snapshot: &PlanDisplaySnapshot, width: u16) -> u16 {
    if width == 0 || snapshot.items.is_empty() {
        return 0;
    }
    let item_count = snapshot.items.len() as u16;
    // Rule/header + compact task rows. Keep the pinned plan compact so it stays
    // adjacent to the composer without crowding the bottom interaction band.
    (1 + item_count.min(4)).clamp(2, 5)
}

pub fn workbench_preferred_height(state: &WorkbenchState, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    if let Some(active) = state.active.as_ref() {
        workbench_snapshot_height(active, width)
    } else if state.cleave.as_ref().is_some_and(|p| p.active)
        || state
            .delegate
            .as_ref()
            .is_some_and(|p| p.active || p.running > 0)
    {
        5
    } else if !state.workstreams.is_empty() || state.workspace.has_visible_context() {
        1
    } else {
        0
    }
}

#[derive(Clone, Default)]
pub struct WorkbenchState {
    pub active: Option<PlanDisplaySnapshot>,
    pub cleave: Option<CleaveProgress>,
    pub delegate: Option<DelegateProgress>,
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
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        let id = value.get("id")?.as_str()?.trim().to_string();
        if id.is_empty() {
            return None;
        }
        let title = value
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&id)
            .to_string();
        let status = value
            .get("status")
            .and_then(|v| v.as_str())
            .map(WorkstreamStatus::from_label)
            .unwrap_or(WorkstreamStatus::Waiting);
        let completed = value.get("completed").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let total = value.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        Some(Self {
            id,
            title,
            status,
            completed,
            total,
        })
    }
}

impl WorkbenchState {
    pub fn from_plan_update_json(value: serde_json::Value) -> Self {
        let workstreams = value
            .get("workstreams")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(WorkstreamSummary::from_json)
                    .collect()
            })
            .unwrap_or_default();
        Self {
            active: PlanDisplaySnapshot::from_json(value),
            workstreams,
            ..Self::default()
        }
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

    fn glyph(self) -> char {
        match self {
            Self::Done => '●',
            Self::Active => '◐',
            Self::Skipped => '⊘',
            Self::Todo => '○',
        }
    }

    fn style(self, t: &dyn theme::Theme, bg: ratatui::style::Color) -> Style {
        let color = match self {
            Self::Done => t.success(),
            Self::Active => t.warning(),
            Self::Skipped => t.dim(),
            Self::Todo => t.accent_muted(),
        };
        let style = Style::default().fg(color).bg(bg);
        if matches!(self, Self::Done | Self::Active) {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }
}

impl PlanDisplaySnapshot {
    pub fn from_json(value: serde_json::Value) -> Option<Self> {
        let mode = value.get("mode")?.as_str()?.to_string();
        let total = value.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if total == 0 {
            return None;
        }
        let completed = value.get("completed").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let items = value
            .get("items")?
            .as_array()?
            .iter()
            .filter_map(|item| {
                let description = item.get("description")?.as_str()?.trim();
                if description.is_empty() {
                    return None;
                }
                let status = item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(PlanDisplayStatus::from_label)
                    .unwrap_or(PlanDisplayStatus::Todo);
                Some(PlanDisplayItem {
                    status,
                    description: description.to_string(),
                })
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            return None;
        }
        Some(Self {
            mode,
            completed,
            total,
            items,
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
        format!("plan {}/{} · {}", self.completed, self.total, self.mode)
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
        let label = item.status.label();
        let glyph = item.status.glyph();
        let line = format!("{}. {glyph} {label:<7} {}", idx + 1, item.description);
        rows.push(PlanDisplayRow {
            text: crate::util::truncate(&line, text_budget),
            status: Some(item.status),
        });
    }
    if hidden_count > 0 {
        rows.push(PlanDisplayRow {
            text: format!("+{hidden_count} more"),
            status: None,
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
        "permission · y once · a always · n deny".to_string()
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
        render_active_workbench_panel(area, frame, t, snapshot, state.workstreams.len());
    } else if let Some(cleave) = state.cleave.as_ref().filter(|p| p.active) {
        render_operation_workbench_panel(
            area,
            frame,
            t,
            &OperationWorkbenchProjection::from_cleave(cleave),
        );
    } else if let Some(delegate) = state
        .delegate
        .as_ref()
        .filter(|p| p.active || p.running > 0)
    {
        render_delegate_workbench_panel(area, frame, t, delegate);
    } else if !state.workstreams.is_empty() {
        render_workstream_summary(area, frame, t, state.workstreams.as_slice(), t.surface_bg());
    } else {
        render_workspace_context_panel(area, frame, t, state);
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
    workstream_count: usize,
) {
    let bg = t.surface_bg();
    let mut lines: Vec<Line<'_>> = Vec::new();
    let mut summary = snapshot.summary();
    if workstream_count > 0 {
        summary.push_str(&format!(" · workstreams×{workstream_count}"));
    }
    let rule_width = area.width.saturating_sub(summary.len() as u16 + 4) as usize;
    lines.push(Line::from(vec![
        Span::styled("─ ", Style::default().fg(t.border_dim()).bg(bg)),
        Span::styled(
            summary,
            Style::default()
                .fg(t.accent_muted())
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", "─".repeat(rule_width)),
            Style::default().fg(t.border_dim()).bg(bg),
        ),
    ]));

    for row in workbench_rows(snapshot, area.width, area.height) {
        let style = row
            .status
            .map(|status| status.style(t, bg))
            .unwrap_or_else(|| Style::default().fg(t.dim()).bg(bg));
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
    for child in projection.children.iter().take(max_rows) {
        let text = operation_worker_chrome_line(child, area.width);
        lines.push(Line::from(Span::styled(
            text,
            operation_worker_status_style(child.status, t, bg),
        )));
    }
    Paragraph::new(lines)
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, frame.buffer_mut());
}

fn operation_worker_chrome_line(child: &OperationChildRow, width: u16) -> String {
    let task_progress = child
        .progress
        .as_ref()
        .map(|progress| format!(" · tasks {}/{}", progress.done, progress.total))
        .unwrap_or_default();
    let result_hint = if !child.result_viewed
        && !matches!(
            child.status,
            OperationChildStatus::Running
                | OperationChildStatus::Queued
                | OperationChildStatus::Starting
                | OperationChildStatus::Waiting
        ) {
        format!(" · result ready: /delegate result {}", child.id)
    } else {
        String::new()
    };
    let failure = child
        .failure
        .as_ref()
        .and_then(|failure| failure.message.as_deref())
        .map(|message| format!(" · {message}"))
        .unwrap_or_default();
    let last_tool = child.last_activity.as_ref().filter(|activity| {
        matches!(
            activity.kind,
            crate::surfaces::operations::OperationActivityKind::Tool
        )
    });
    worker_chrome_line(
        &child.label,
        child.status.label(),
        last_tool,
        &format!("{task_progress}{result_hint}{failure}"),
        width,
    )
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
    let row = project_worker_chrome_row(label, status, last_tool, task_progress);
    crate::tui::inline_render::render_inline_text_row(&row, width.saturating_sub(1))
}

fn project_worker_chrome_row(
    label: &str,
    status: &str,
    last_tool: Option<&crate::surfaces::operations::OperationActivity>,
    task_progress: &str,
) -> crate::surfaces::inline::InlineRow<String> {
    let glyphs = crate::tui::glyphs::glyphs();
    let state = glyphs.tool_state(crate::tui::glyphs::tool_state_role_for_status(status));
    let tool = last_tool.map(|activity| {
        let identity = crate::surfaces::conversation::tool_visual_identity(
            &activity.label,
            activity.args_summary.as_deref(),
        );
        let category = glyphs.tool_category(crate::tui::glyphs::tool_category_role_for_identity(
            &identity,
        ));
        format!("{category} {}", identity.label)
    });
    crate::surfaces::inline::InlineRow::new(
        vec![
            crate::surfaces::inline::InlineCell::new(
                format!("{state} {label}"),
                crate::surfaces::inline::InlineCellRole::Status,
            ),
            crate::surfaces::inline::InlineCell::new(
                status.to_string(),
                crate::surfaces::inline::InlineCellRole::Value,
            ),
        ],
        tool.into_iter()
            .chain(
                (!task_progress.is_empty())
                    .then(|| task_progress.trim_start_matches(" · ").to_string()),
            )
            .map(|cell| {
                crate::surfaces::inline::InlineCell::new(
                    cell,
                    crate::surfaces::inline::InlineCellRole::Metadata,
                )
            })
            .collect(),
    )
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
    fn empty_workbench_without_workspace_context_has_no_height() {
        assert_eq!(
            workbench_preferred_height(&WorkbenchState::default(), 120),
            0
        );
    }
}

//! Plan dock snapshot surface rendering and hint policy.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::{dashboard, theme};

pub fn plan_dock_snapshot_height(snapshot: &PlanDisplaySnapshot, width: u16) -> u16 {
    if width == 0 || snapshot.items.is_empty() {
        return 0;
    }
    let item_count = snapshot.items.len() as u16;
    // Rule/header + compact task rows, capped so the plan never crowds out the transcript.
    (1 + item_count.min(6)).clamp(2, 8)
}

pub fn plan_dock_preferred_height(state: &PlanDockState, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    if let Some(active) = state.active.as_ref() {
        plan_dock_snapshot_height(active, width)
    } else if !state.background.is_empty() {
        1
    } else {
        0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanDockState {
    pub active: Option<PlanDisplaySnapshot>,
    pub background: Vec<PlanSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSummary {
    pub id: String,
    pub title: String,
    pub status: PlanLifecycleStatus,
    pub completed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanLifecycleStatus {
    Active,
    Background,
    Waiting,
    Blocked,
    Complete,
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

impl PlanLifecycleStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Background => "background",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Complete => "complete",
        }
    }
}

impl PlanSummary {
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
            let icon = match item.status {
                PlanDisplayStatus::Done => '●',
                PlanDisplayStatus::Active => '◐',
                PlanDisplayStatus::Skipped => '⊘',
                PlanDisplayStatus::Todo => '○',
            };
            lines.push(format!("{}. {icon} {}", idx + 1, item.description));
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
        self.items
            .iter()
            .position(|item| matches!(item.status, PlanDisplayStatus::Todo))
            .is_some_and(|idx| idx < visible_items)
    }
}

pub fn active_plan_dock_snapshot(
    live_snapshot: Option<&PlanDisplaySnapshot>,
    _legacy_plan_text: Option<&str>,
) -> Option<PlanDisplaySnapshot> {
    // Only the live PlanUpdated projection may drive the Plan Dock. Legacy
    // transcript text is durable history, not active state; falling back to it
    // resurrects old unfinished plans after branch/session/task changes.
    live_snapshot
        .filter(|snapshot| !snapshot.is_complete())
        .cloned()
}

pub fn plan_dock_rows(
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
    let text_budget = width.saturating_sub(2) as usize;
    let mut rows = Vec::new();
    for (idx, item) in snapshot.items.iter().take(visible_items).enumerate() {
        let label = item.status.label();
        let line = format!("{}. {label:<7} {}", idx + 1, item.description);
        rows.push(PlanDisplayRow {
            text: crate::util::truncate(&line, text_budget),
            status: Some(item.status),
        });
    }
    if hidden > 0 {
        rows.push(PlanDisplayRow {
            text: format!("+{hidden} more"),
            status: None,
        });
    }
    rows
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
        labels.push(if self.active { "active plan" } else { "no active plan" }.to_string());
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
            SlimPlanHintState::None => {
                "transcript live · PgUp/PgDn scroll · Ctrl+Shift+Y copy answer".to_string()
            }
        }
    }
}

pub fn render_plan_dock_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    state: &PlanDockState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bg = t.surface_bg();
    if let Some(snapshot) = state.active.as_ref() {
        render_active_plan_dock_panel(area, frame, t, snapshot, state.background.len());
    } else {
        render_background_plan_summary(area, frame, t, state.background.as_slice(), bg);
    }
}

fn render_active_plan_dock_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    snapshot: &PlanDisplaySnapshot,
    background_count: usize,
) {
    let bg = t.surface_bg();
    let mut lines: Vec<Line<'_>> = Vec::new();
    let mut summary = snapshot.summary();
    if background_count > 0 {
        summary.push_str(&format!(" · background×{background_count}"));
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

    for row in plan_dock_rows(snapshot, area.width, area.height) {
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

fn render_background_plan_summary(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    background: &[PlanSummary],
    bg: ratatui::style::Color,
) {
    if background.is_empty() {
        return;
    }
    let mut text = format!(" background plans×{}", background.len());
    if let Some(first) = background.first() {
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

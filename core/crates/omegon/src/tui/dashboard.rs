//! Dashboard sidebar — rich design-tree + lifecycle state panel.
//!
//! Rendered as a right-side panel when terminal width >= 120 columns.
//! Uses `tui-tree-widget` for interactive expand/collapse tree navigation.
//!
//! Layout (top → bottom):
//! 1. Header — title + pipeline funnel bar + status counts
//! 2. Focused node — enriched detail for the active design focus
//! 3. Tree — tui-tree-widget with status icons, badges, parent-child hierarchy
//! 4. OpenSpec changes — active change names with stage + progress (bottom-anchored)

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use ratatui::widgets::Scrollbar;
use tui_tree_widget::{Tree, TreeItem, TreeState};

use super::theme::Theme;
use super::widgets;
use crate::lifecycle::types::*;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::features::cleave::CleaveProgress;
use crate::lifecycle::context::LifecycleContextProvider;
use crate::lifecycle::design;
use crate::status::HarnessStatus;

/// Shared session stats — written by the TUI, read by the web API.
#[derive(Default)]
pub struct SharedSessionStats {
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
}

/// Shared handles to feature state, for live dashboard updates.
#[derive(Clone, Default)]
pub struct DashboardHandles {
    pub lifecycle: Option<Arc<Mutex<LifecycleContextProvider>>>,
    pub cleave: Option<Arc<Mutex<CleaveProgress>>>,
    pub session: Arc<Mutex<SharedSessionStats>>,
    pub harness: Option<Arc<Mutex<HarnessStatus>>>,
}

impl DashboardHandles {
    /// Rescan filesystem and refresh dashboard in a single lock acquisition.
    /// Call periodically to pick up changes from external processes
    /// (other Omegon instances, git pull, manual edits).
    /// Combines rescan + refresh to avoid double-locking the lifecycle Mutex.
    pub fn rescan_and_refresh(&self, state: &mut DashboardState) {
        if let Some(ref lp_lock) = self.lifecycle
            && let Ok(mut lp) = lp_lock.lock()
        {
            lp.refresh();
            // Fall through to refresh_from_lifecycle below
            Self::refresh_from_lifecycle(&lp, state);
        }
        self.refresh_non_lifecycle(state);
    }

    /// Refresh dashboard state from the shared feature handles.
    pub fn refresh_into(&self, state: &mut DashboardState) {
        // Lifecycle
        if let Some(ref lp_lock) = self.lifecycle
            && let Ok(lp) = lp_lock.lock()
        {
            Self::refresh_from_lifecycle(&lp, state);
        }
        self.refresh_non_lifecycle(state);
    }

    fn refresh_non_lifecycle(&self, state: &mut DashboardState) {
        // Cleave
        if let Some(ref cp_lock) = self.cleave
            && let Ok(cp) = cp_lock.lock()
        {
            state.cleave = Some(cp.clone());
        }
        // Harness
        if let Some(ref harness_lock) = self.harness
            && let Ok(harness) = harness_lock.lock()
        {
            state.harness = Some(harness.clone());
        }
    }

    fn refresh_from_lifecycle(lp: &LifecycleContextProvider, state: &mut DashboardState) {
        state.focused_node = lp.focused_node_id().and_then(|id| {
            lp.get_node(id).map(|n| {
                let sections = design::read_node_sections(n);
                let assumptions = n.assumption_count();
                let decisions_count = sections
                    .as_ref()
                    .map(|s| s.decisions.iter().filter(|d| d.status == "decided").count())
                    .unwrap_or(0);
                let readiness = sections
                    .as_ref()
                    .map(|s| s.readiness_score())
                    .unwrap_or(0.0);
                FocusedNodeSummary {
                    id: n.id.clone(),
                    title: n.title.clone(),
                    status: n.status,
                    open_questions: n.open_questions.len() - assumptions,
                    assumptions,
                    decisions: decisions_count,
                    readiness,
                    openspec_change: n.openspec_change.clone(),
                }
            })
        });
        state.active_changes = lp
            .changes()
            .iter()
            .filter(|c| !matches!(c.stage, ChangeStage::Archived))
            .map(|c| ChangeSummary {
                name: c.name.clone(),
                stage: c.stage,
                done_tasks: c.done_tasks,
                total_tasks: c.total_tasks,
            })
            .collect();

        // Status counts + node lists
        let nodes = lp.all_nodes();
        let mut counts = StatusCounts {
            total: nodes.len(),
            ..Default::default()
        };
        state.implementing_nodes.clear();
        state.actionable_nodes.clear();
        state.all_nodes.clear();

        for node in nodes.values() {
            match node.status {
                NodeStatus::Implementing => counts.implementing += 1,
                NodeStatus::Decided => counts.decided += 1,
                NodeStatus::Exploring => counts.exploring += 1,
                NodeStatus::Implemented => counts.implemented += 1,
                NodeStatus::Blocked => counts.blocked += 1,
                NodeStatus::Deferred => counts.deferred += 1,
                _ => {}
            }
            counts.open_questions += node.open_questions.len();

            let summary = NodeSummary {
                id: node.id.clone(),
                title: node.title.clone(),
                status: node.status,
                open_questions: node.open_questions.len(),
                parent: node.parent.clone(),
                priority: node.priority,
                issue_type: node.issue_type,
                openspec_change: node.openspec_change.clone(),
            };

            // Collect all non-implemented nodes for tree view
            if !matches!(node.status, NodeStatus::Implemented) {
                state.all_nodes.push(summary.clone());
            }
            if matches!(node.status, NodeStatus::Implementing) {
                state.implementing_nodes.push(summary.clone());
            }
            if matches!(node.status, NodeStatus::Decided) {
                state.actionable_nodes.push(summary);
            }
        }
        state.status_counts = counts;

        // Collect degraded nodes
        state.degraded_nodes = lp
            .degraded_nodes()
            .iter()
            .map(|d| DegradedNodeSummary {
                id: d.id.clone(),
                title: d.title.clone(),
                file_path: d.file_path.display().to_string(),
                reason: d.reason.to_string(),
            })
            .collect();
    }
}

/// Dashboard state — updated from lifecycle scanning.
#[derive(Default)]
pub struct DashboardState {
    pub focused_node: Option<FocusedNodeSummary>,
    pub active_changes: Vec<ChangeSummary>,
    pub cleave: Option<CleaveProgress>,
    pub harness: Option<HarnessStatus>,
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
    // Enriched: status counts + node lists
    pub status_counts: StatusCounts,
    pub implementing_nodes: Vec<NodeSummary>,
    pub actionable_nodes: Vec<NodeSummary>,
    /// All non-implemented nodes for tree rendering.
    pub all_nodes: Vec<NodeSummary>,
    /// Nodes that were valid but are now broken (file exists, parse fails).
    pub degraded_nodes: Vec<DegradedNodeSummary>,
    /// Tree widget selection state (managed by tui-tree-widget).
    pub tree_state: TreeState<String>,
    /// Whether the sidebar is currently receiving keyboard input.
    pub sidebar_active: bool,
    // Context gauge
    pub context_used_pct: f32,
    pub context_window_k: usize,
}

impl DashboardState {
    /// Handle keyboard events when sidebar is active.
    /// Returns true if the event was consumed.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        if !self.sidebar_active {
            return false;
        }
        match key.code {
            KeyCode::Up => {
                self.tree_state.key_up();
                true
            }
            KeyCode::Down => {
                self.tree_state.key_down();
                true
            }
            KeyCode::Left => {
                self.tree_state.key_left();
                true
            }
            KeyCode::Right => {
                self.tree_state.key_right();
                true
            }
            KeyCode::Home => {
                self.tree_state.select_first();
                true
            }
            KeyCode::End => {
                self.tree_state.select_last();
                true
            }
            KeyCode::Esc => {
                self.sidebar_active = false;
                true
            }
            // Enter handled by caller (needs bus access to send design-focus)
            _ => false,
        }
    }

    /// Get the currently selected node ID (if any).
    pub fn selected_node_id(&self) -> Option<&str> {
        let sel = self.tree_state.selected();
        sel.last().map(|s| s.as_str())
    }

    /// Mouse wheel scrolling for the sidebar tree.
    pub fn scroll_up(&mut self, lines: usize) {
        for _ in 0..lines {
            self.tree_state.key_up();
        }
    }

    /// Mouse wheel scrolling for the sidebar tree.
    pub fn scroll_down(&mut self, lines: usize) {
        for _ in 0..lines {
            self.tree_state.key_down();
        }
    }
}

#[derive(Default, Clone)]
pub struct StatusCounts {
    pub total: usize,
    pub implementing: usize,
    pub decided: usize,
    pub exploring: usize,
    pub implemented: usize,
    pub blocked: usize,
    pub deferred: usize,
    pub open_questions: usize,
}

#[derive(Clone)]
pub struct NodeSummary {
    pub id: String,
    pub title: String,
    pub status: NodeStatus,
    pub open_questions: usize,
    pub parent: Option<String>,
    pub priority: Option<u8>,
    pub issue_type: Option<IssueType>,
    pub openspec_change: Option<String>,
}

#[derive(Clone)]
pub struct FocusedNodeSummary {
    pub id: String,
    pub title: String,
    pub status: NodeStatus,
    pub open_questions: usize,
    pub assumptions: usize,
    pub decisions: usize,
    pub readiness: f32,
    pub openspec_change: Option<String>,
}

#[derive(Clone)]
pub struct DegradedNodeSummary {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub reason: String,
}

#[derive(Clone)]
pub struct ChangeSummary {
    pub name: String,
    pub stage: ChangeStage,
    pub done_tasks: usize,
    pub total_tasks: usize,
}

// ─── Rendering ──────────────────────────────────────────────────────

impl DashboardState {
    pub fn render(&mut self, area: Rect, frame: &mut Frame) {
        self.render_themed(area, frame, &super::theme::Alpharius);
    }

    pub fn render_themed(&mut self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        // Clear the dashboard area — prevent stale conversation bleed-through
        frame.render_widget(ratatui::widgets::Clear, area);

        let block = Block::default()
            .borders(Borders::LEFT)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(if self.sidebar_active {
                t.accent()
            } else {
                t.border_dim()
            }))
            .style(Style::default().bg(t.bg()));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width < 4 || inner.height < 4 {
            return;
        }

        // ── Compute section heights ─────────────────────────────
        let header_lines = self.build_header_lines(inner.width as usize, t);
        let focus_lines = self.build_focus_lines(inner.width as usize, t);

        let header_h = header_lines.len() as u16;
        let focus_h = focus_lines.len() as u16;

        // Tree gets all remaining space — OpenSpec changes are now shown
        // inline on their bound tree nodes, not in a separate section.
        let tree_h = inner.height.saturating_sub(header_h + focus_h).max(3);

        let chunks = Layout::vertical([
            Constraint::Length(header_h),
            Constraint::Length(focus_h),
            Constraint::Length(tree_h),
        ])
        .split(inner);

        // ── Render header ───────────────────────────────────────
        if header_h > 0 {
            let para = Paragraph::new(header_lines);
            frame.render_widget(para, chunks[0]);
        }

        // ── Render focused node ─────────────────────────────────
        if focus_h > 0 {
            let para = Paragraph::new(focus_lines);
            frame.render_widget(para, chunks[1]);
        }

        // ── Render tree ─────────────────────────────────────────
        if tree_h > 0 && !self.all_nodes.is_empty() {
            self.render_tree(chunks[2], frame, t);
        } else if tree_h > 0 {
            // No nodes — show hint
            let hint = Paragraph::new(Line::from(Span::styled(
                " no active nodes",
                Style::default().fg(t.dim()),
            )));
            frame.render_widget(hint, chunks[2]);
        }
    }

    // ── Section builders ────────────────────────────────────────

    fn build_header_lines<'a>(&self, w: usize, t: &dyn Theme) -> Vec<Line<'a>> {
        let mut lines: Vec<Line<'a>> = Vec::new();

        // Title
        lines.push(Line::from(Span::styled(
            " Ω Dashboard",
            Style::default().fg(t.accent()).add_modifier(Modifier::BOLD),
        )));

        if self.status_counts.total == 0 {
            lines.push(Line::from(""));
            return lines;
        }

        let c = &self.status_counts;

        // Status count badges — single compact line
        let mut badge_parts: Vec<Span<'a>> = vec![Span::styled(" ", Style::default())];
        // Show active counts only (implementing first, then decided, exploring, blocked)
        let status_items: Vec<(&str, usize, Color)> = vec![
            ("⚙", c.implementing, t.warning()),
            ("●", c.decided, t.success()),
            ("◐", c.exploring, t.accent()),
            ("✕", c.blocked, t.error()),
            ("◑", c.deferred, t.caution()),
        ];
        for (icon, count, color) in status_items {
            if count > 0 {
                badge_parts.push(Span::styled(
                    format!("{icon}{count}"),
                    Style::default().fg(color),
                ));
                badge_parts.push(Span::styled(" ", Style::default()));
            }
        }
        // Total + questions at end
        badge_parts.push(Span::styled(
            format!("Σ{}", c.total),
            Style::default().fg(t.dim()),
        ));
        if c.open_questions > 0 {
            badge_parts.push(Span::styled(
                format!(" ?{}", c.open_questions),
                Style::default().fg(t.warning()),
            ));
        }
        if !self.degraded_nodes.is_empty() {
            badge_parts.push(Span::styled(
                format!(" ⚠{}", self.degraded_nodes.len()),
                Style::default().fg(t.error()),
            ));
        }
        lines.push(Line::from(badge_parts));

        // Pipeline funnel bar
        let funnel_w = w.saturating_sub(2);
        if funnel_w >= 12 && c.total > 0 {
            let total = c.total as f32;
            let seg = |count: usize, ch: &str, color: Color| -> Span<'a> {
                let cw = ((count as f32 / total) * funnel_w as f32)
                    .round()
                    .max(if count > 0 { 1.0 } else { 0.0 }) as usize;
                Span::styled(ch.repeat(cw), Style::default().fg(color))
            };
            // All statuses represented so segments sum to total
            let seed_resolved = c.total.saturating_sub(
                c.exploring + c.decided + c.implementing + c.implemented + c.blocked + c.deferred,
            );
            lines.push(Line::from(vec![
                Span::styled(" ", Style::default()),
                seg(seed_resolved, "·", t.dim()),
                seg(c.exploring, "░", t.accent()),
                seg(c.decided, "▒", t.success()),
                seg(c.implementing, "▓", t.warning()),
                seg(c.blocked, "▓", t.error()),
                seg(c.deferred, "░", t.caution()),
                seg(c.implemented, "█", t.dim()),
            ]));
        }

        lines.push(Line::from(""));
        lines
    }

    fn build_focus_lines<'a>(&self, w: usize, t: &dyn Theme) -> Vec<Line<'a>> {
        let Some(ref node) = self.focused_node else {
            return vec![];
        };

        let mut lines: Vec<Line<'a>> = Vec::new();
        lines.push(widgets::section_divider("focus", w, t));

        // Node ID line: icon + id (bold)
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", node.status.icon()),
                Style::default().fg(status_color(node.status, t)),
            ),
            Span::styled(
                widgets::truncate_str(&node.id, w.saturating_sub(4), "…").to_string(),
                Style::default().fg(t.fg()).add_modifier(Modifier::BOLD),
            ),
        ]));

        // Title line (muted, truncated)
        let title = widgets::truncate_str(&node.title, w.saturating_sub(3), "…");
        lines.push(Line::from(Span::styled(
            format!("   {title}"),
            Style::default().fg(t.muted()),
        )));

        // Badges line: decisions, questions, assumptions, readiness
        let mut parts: Vec<Span<'a>> = vec![Span::styled("   ", Style::default())];
        if node.decisions > 0 {
            parts.push(Span::styled(
                format!("✓{} ", node.decisions),
                Style::default().fg(t.success()),
            ));
        }
        if node.open_questions > 0 {
            parts.push(Span::styled(
                format!("?{} ", node.open_questions),
                Style::default().fg(t.warning()),
            ));
        }
        if node.assumptions > 0 {
            parts.push(Span::styled(
                format!("⚠{} ", node.assumptions),
                Style::default().fg(t.caution()),
            ));
        }
        // Readiness gauge inline
        let pct = (node.readiness * 100.0) as u8;
        let readiness_color = if pct >= 80 {
            t.success()
        } else if pct >= 50 {
            t.warning()
        } else {
            t.error()
        };
        parts.push(Span::styled(
            format!("{pct}%"),
            Style::default().fg(readiness_color),
        ));
        // Bound OpenSpec change
        if let Some(ref change) = node.openspec_change {
            parts.push(Span::styled(" → ", Style::default().fg(t.dim())));
            parts.push(Span::styled(
                widgets::truncate_str(change, 12, "…").to_string(),
                Style::default().fg(t.accent()),
            ));
        }
        lines.push(Line::from(parts));

        lines.push(Line::from(""));
        lines
    }

    fn build_changes_lines<'a>(&self, w: usize, t: &dyn Theme) -> Vec<Line<'a>> {
        if self.active_changes.is_empty() {
            return vec![];
        }

        let mut lines: Vec<Line<'a>> = Vec::new();
        lines.push(widgets::section_divider("openspec", w, t));

        for change in &self.active_changes {
            let (icon, color) = stage_badge(change.stage, t);
            let progress = if change.total_tasks > 0 {
                format!(" {}/{}", change.done_tasks, change.total_tasks)
            } else {
                String::new()
            };
            let name_max = w.saturating_sub(icon.len() + 1 + progress.len() + 2);
            let name = widgets::truncate_str(&change.name, name_max, "…");
            lines.push(Line::from(vec![
                Span::styled(format!(" {icon} "), Style::default().fg(color)),
                Span::styled(name.to_string(), Style::default().fg(t.fg())),
                Span::styled(progress, Style::default().fg(t.dim())),
            ]));
        }
        lines
    }

    // ── Tree rendering ──────────────────────────────────────────

    fn render_tree(&mut self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let focused_id = self.focused_node.as_ref().map(|n| n.id.as_str());
        let changes_by_name: std::collections::HashMap<&str, &ChangeSummary> = self
            .active_changes
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();
        let mut items = build_tree_items(&self.all_nodes, focused_id, &changes_by_name, t);

        // Prepend degraded nodes — files that exist but no longer parse.
        // Shown with ⚠ icon so the operator can trace the breakage.
        for d in self.degraded_nodes.iter().rev() {
            let text = Text::from(Line::from(vec![
                Span::styled("⚠ ", Style::default().fg(t.error())),
                Span::styled(
                    d.id.clone(),
                    Style::default()
                        .fg(t.error())
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(format!(" ({})", d.reason), Style::default().fg(t.dim())),
            ]));
            // Use a distinct ID prefix to avoid collisions with valid nodes
            let item = TreeItem::new_leaf(format!("degraded:{}", d.id), text);
            items.insert(0, item);
        }

        // Auto-open root nodes on first render so tree isn't fully collapsed
        if self.tree_state.opened().is_empty() && !items.is_empty() {
            for item in &items {
                if !item.children().is_empty() {
                    self.tree_state.open(vec![item.identifier().clone()]);
                }
            }
        }

        let Ok(tree) = Tree::new(&items) else {
            // Duplicate identifiers — shouldn't happen but render fallback
            let fallback = Paragraph::new(Line::from(Span::styled(
                " tree error",
                Style::default().fg(t.error()),
            )));
            frame.render_widget(fallback, area);
            return;
        };

        // Highlight style: when active, use surface bg with bright text.
        // The key insight: tui-tree-widget applies highlight AFTER rendering
        // text spans, overriding their fg/bg. We use a clearly contrasting
        // pair — bright fg on a muted surface bg distinct from the tree bg.
        let hl = if self.sidebar_active {
            Style::default()
                .bg(t.border())
                .fg(t.fg())
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default()
        };

        let tree_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width.saturating_sub(2).max(1),
            height: area.height,
        };
        let scrollbar_area = Rect {
            x: area.x + area.width.saturating_sub(1),
            y: area.y,
            width: 1,
            height: area.height,
        };

        let tree = tree
            .style(Style::default().bg(t.bg()))
            .highlight_style(hl)
            .highlight_symbol(if self.sidebar_active { "▸" } else { " " })
            .node_closed_symbol("▸ ")
            .node_open_symbol("▾ ")
            .node_no_children_symbol("  ");

        frame.render_stateful_widget(tree, tree_area, &mut self.tree_state);

        // Render a dedicated scrollbar gutter so tree text never paints under it.
        let visible_rows = self
            .tree_state
            .flatten(&items)
            .len()
            .saturating_sub(tree_area.height as usize);
        let mut scrollbar_state = tui_tree_widget::ScrollbarState::new(visible_rows)
            .position(self.tree_state.get_offset());
        let scrollbar = Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)
            .thumb_symbol("▐")
            .track_symbol(Some("░"))
            .thumb_style(Style::default().fg(t.border_dim()))
            .track_style(Style::default().fg(t.bg()));
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

// ─── Tree item construction ─────────────────────────────────────────

/// Build hierarchical `TreeItem`s from flat `NodeSummary` list.
///
/// Preserves parent-child structure. Nodes whose parents are not in the
/// active set become roots. Sorts within each level: implementing first,
/// then decided, exploring, blocked, deferred, seed.
fn build_tree_items<'a>(
    nodes: &[NodeSummary],
    focused_id: Option<&str>,
    changes: &std::collections::HashMap<&str, &ChangeSummary>,
    t: &dyn Theme,
) -> Vec<TreeItem<'a, String>> {
    // Index children by parent
    let mut children_map: HashMap<Option<&str>, Vec<&NodeSummary>> = HashMap::new();
    let id_set: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    for node in nodes {
        let effective_parent = match &node.parent {
            Some(p) if id_set.contains(p.as_str()) => Some(p.as_str()),
            _ => None, // parent not in active set — treat as root
        };
        children_map.entry(effective_parent).or_default().push(node);
    }

    // Sort children: by status priority, then alphabetical
    for children in children_map.values_mut() {
        children.sort_by(|a, b| {
            status_sort_key(a.status)
                .cmp(&status_sort_key(b.status))
                .then(a.id.cmp(&b.id))
        });
    }

    fn build_recursive<'a>(
        parent_key: Option<&str>,
        children_map: &HashMap<Option<&str>, Vec<&NodeSummary>>,
        focused_id: Option<&str>,
        changes: &std::collections::HashMap<&str, &ChangeSummary>,
        t: &dyn Theme,
    ) -> Vec<TreeItem<'a, String>> {
        let Some(children) = children_map.get(&parent_key) else {
            return vec![];
        };
        children
            .iter()
            .filter_map(|node| {
                let child_items =
                    build_recursive(Some(&node.id), children_map, focused_id, changes, t);
                let text = node_text(node, focused_id, changes, t);
                if child_items.is_empty() {
                    Some(TreeItem::new_leaf(node.id.clone(), text))
                } else {
                    TreeItem::new(node.id.clone(), text, child_items).ok()
                }
            })
            .collect()
    }

    build_recursive(None, &children_map, focused_id, changes, t)
}

/// Build the rich `Text` for a single tree node line.
///
/// Format: `icon id [?N] [P1]`
/// - icon: status icon in status color
/// - id: node id (bold if focused, normal otherwise)
/// - ?N: question count badge (if > 0)
/// - P1-P5: priority badge (if set)
fn node_text<'a>(
    node: &NodeSummary,
    focused_id: Option<&str>,
    changes: &std::collections::HashMap<&str, &ChangeSummary>,
    t: &dyn Theme,
) -> Text<'a> {
    let (icon, color) = status_icon_color(node.status, t);
    let is_focused = focused_id == Some(node.id.as_str());

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(6);

    // Status icon
    spans.push(Span::styled(format!("{icon} "), Style::default().fg(color)));

    // Node ID
    let id_style = if is_focused {
        Style::default()
            .fg(t.accent_bright())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(t.fg())
    };
    spans.push(Span::styled(node.id.clone(), id_style));

    // Question count badge
    if node.open_questions > 0 {
        spans.push(Span::styled(
            format!(" ?{}", node.open_questions),
            Style::default().fg(t.warning()),
        ));
    }

    // Priority badge
    if let Some(p) = node.priority {
        let (label, pcolor) = priority_badge(p, t);
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(pcolor),
        ));
    }

    // Inline OpenSpec: stage icon + task progress, not a bare ◈
    if let Some(ref change_name) = node.openspec_change {
        if let Some(change) = changes.get(change_name.as_str()) {
            let (stage_icon, stage_color) = stage_badge(change.stage, t);
            spans.push(Span::styled(
                format!(" {stage_icon}"),
                Style::default().fg(stage_color),
            ));
            if change.total_tasks > 0 {
                spans.push(Span::styled(
                    format!(" {}/{}", change.done_tasks, change.total_tasks),
                    Style::default().fg(t.dim()),
                ));
            }
        } else {
            // Not in active_changes (archived/unknown) — dim indicator only
            spans.push(Span::styled(" ◈", Style::default().fg(t.accent_muted())));
        }
    }

    Text::from(Line::from(spans))
}

fn status_icon_color(status: NodeStatus, t: &dyn Theme) -> (&'static str, Color) {
    match status {
        NodeStatus::Seed => ("◌", t.dim()),
        NodeStatus::Exploring => ("◐", t.accent()),
        NodeStatus::Resolved => ("◉", t.success()),
        NodeStatus::Decided => ("●", t.success()),
        NodeStatus::Implementing => ("⚙", t.warning()),
        NodeStatus::Implemented => ("✓", t.dim()),
        NodeStatus::Blocked => ("✕", t.error()),
        NodeStatus::Deferred => ("◑", t.caution()),
    }
}

fn status_sort_key(status: NodeStatus) -> u8 {
    match status {
        NodeStatus::Implementing => 0,
        NodeStatus::Blocked => 1,
        NodeStatus::Decided => 2,
        NodeStatus::Exploring => 3,
        NodeStatus::Resolved => 4,
        NodeStatus::Seed => 5,
        NodeStatus::Deferred => 6,
        NodeStatus::Implemented => 7,
    }
}

fn priority_badge(p: u8, t: &dyn Theme) -> (&'static str, Color) {
    match p {
        1 => ("P1", t.error()),
        2 => ("P2", t.warning()),
        3 => ("P3", t.fg()),
        4 => ("P4", t.dim()),
        5 => ("P5", t.dim()),
        _ => ("P?", t.dim()),
    }
}

fn status_color(status: NodeStatus, t: &dyn Theme) -> Color {
    match status {
        NodeStatus::Seed => t.dim(),
        NodeStatus::Exploring => t.accent(),
        NodeStatus::Resolved | NodeStatus::Decided | NodeStatus::Implemented => t.success(),
        NodeStatus::Implementing => t.warning(),
        NodeStatus::Blocked => t.error(),
        NodeStatus::Deferred => t.caution(),
    }
}

fn stage_badge(stage: ChangeStage, t: &dyn Theme) -> (&'static str, Color) {
    match stage {
        ChangeStage::Proposed => ("◌", t.dim()),
        ChangeStage::Specified => ("◐", t.dim()),
        ChangeStage::Planned => ("▸", t.muted()),
        ChangeStage::Implementing => ("⟳", t.warning()),
        ChangeStage::Verifying => ("◉", t.success()),
        ChangeStage::Archived => ("✓", t.success()),
    }
}

#[cfg(test)]
fn format_k(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", tokens / 1_000_000)
    } else {
        format!("{}k", tokens / 1000)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::cleave::ChildProgress;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn buf_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let area = buf.area;
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| buf[(x, y)].symbol().to_string()))
            .collect()
    }

    #[test]
    fn empty_dashboard_renders() {
        let mut state = DashboardState::default();
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();
    }

    #[test]
    fn dashboard_with_focused_node() {
        let mut state = DashboardState::default();
        state.focused_node = Some(FocusedNodeSummary {
            id: "test-node".into(),
            title: "Test Node".into(),
            status: NodeStatus::Exploring,
            open_questions: 3,
            assumptions: 1,
            decisions: 2,
            readiness: 0.33,
            openspec_change: None,
        });
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        assert!(text.contains("test-node"), "should render node id: {text}");
    }

    #[test]
    fn dashboard_with_focused_node_openspec() {
        let mut state = DashboardState::default();
        state.focused_node = Some(FocusedNodeSummary {
            id: "my-feat".into(),
            title: "My Feature".into(),
            status: NodeStatus::Implementing,
            open_questions: 0,
            assumptions: 0,
            decisions: 3,
            readiness: 0.75,
            openspec_change: Some("my-feat-change".into()),
        });
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        assert!(text.contains("my-feat"), "should render node id: {text}");
    }

    #[test]
    fn dashboard_with_changes() {
        // OpenSpec changes are now shown inline on their bound tree nodes,
        // not in a separate sidebar section.  A change with no bound node
        // produces no visible text — that is the correct new behaviour.
        // A change bound to a node should render the stage icon on that node.
        let mut state = DashboardState::default();
        state.active_changes = vec![ChangeSummary {
            name: "my-change".into(),
            stage: ChangeStage::Implementing,
            done_tasks: 3,
            total_tasks: 8,
        }];
        // Add a node that references the change
        state.all_nodes.push(NodeSummary {
            id: "feat-node".into(),
            title: "Feature node".into(),
            status: NodeStatus::Exploring,
            parent: None,
            open_questions: 0,
            openspec_change: Some("my-change".into()),
            priority: None,
            issue_type: None,
        });
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        // The node id should appear, and 3/8 task progress should be inline
        assert!(
            text.contains("feat-node"),
            "should render bound node: {text}"
        );
        assert!(
            text.contains("3/8"),
            "should render task progress inline: {text}"
        );
    }

    #[test]
    fn dashboard_with_cleave_state() {
        // Cleave progress is stored in DashboardState but rendered in the
        // instruments panel, not the sidebar. Verify it's populated.
        let mut state = DashboardState::default();
        state.cleave = Some(CleaveProgress {
            active: true,
            run_id: "clv-test".into(),
            total_children: 3,
            completed: 1,
            failed: 0,
            children: vec![ChildProgress {
                label: "task-a".into(),
                status: "completed".into(),
                duration_secs: Some(12.0),
                last_tool: None,
                last_turn: None,
                started_at: None,
                tokens_in: 0,
                tokens_out: 0,
            }],
            total_tokens_in: 0,
            total_tokens_out: 0,
        });
        assert!(state.cleave.as_ref().unwrap().active);
        assert_eq!(state.cleave.as_ref().unwrap().total_children, 3);
    }

    #[test]
    fn dashboard_handles_refresh_empty() {
        let handles = DashboardHandles::default();
        let mut state = DashboardState::default();
        handles.refresh_into(&mut state);
        assert!(state.focused_node.is_none());
        assert!(state.active_changes.is_empty());
    }

    #[test]
    fn session_stats_stored() {
        // Session stats are stored in DashboardState but rendered in the
        // footer engine panel, not the sidebar. Verify they're populated.
        let mut state = DashboardState::default();
        state.turns = 15;
        state.tool_calls = 42;
        state.compactions = 2;
        assert_eq!(state.turns, 15);
        assert_eq!(state.tool_calls, 42);
        assert_eq!(state.compactions, 2);
    }

    #[test]
    fn status_color_mapping() {
        let t = super::super::theme::Alpharius;
        assert_eq!(status_color(NodeStatus::Seed, &t), t.dim());
        assert_eq!(status_color(NodeStatus::Exploring, &t), t.accent());
        assert_eq!(status_color(NodeStatus::Implemented, &t), t.success());
        assert_eq!(status_color(NodeStatus::Blocked, &t), t.error());
    }

    #[test]
    fn stage_badge_mapping() {
        let t = super::super::theme::Alpharius;
        let (icon, _) = stage_badge(ChangeStage::Implementing, &t);
        assert_eq!(icon, "⟳");
        let (icon, _) = stage_badge(ChangeStage::Archived, &t);
        assert_eq!(icon, "✓");
    }

    #[test]
    fn dashboard_with_status_counts() {
        let mut state = DashboardState::default();
        state.status_counts = StatusCounts {
            total: 140,
            implementing: 7,
            decided: 5,
            exploring: 5,
            implemented: 100,
            blocked: 0,
            deferred: 3,
            open_questions: 24,
        };
        let backend = TestBackend::new(36, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        assert!(text.contains("140"), "should show total: {text}");
    }

    #[test]
    fn dashboard_with_tree_nodes() {
        let mut state = DashboardState::default();
        state.status_counts.total = 10;
        state.all_nodes = vec![
            NodeSummary {
                id: "rust-tui".into(),
                title: "Rust TUI".into(),
                status: NodeStatus::Implementing,
                open_questions: 2,
                parent: None,
                priority: Some(1),
                issue_type: None,
                openspec_change: None,
            },
            NodeSummary {
                id: "web-dash".into(),
                title: "Web Dashboard".into(),
                status: NodeStatus::Exploring,
                open_questions: 0,
                parent: Some("rust-tui".into()),
                priority: None,
                issue_type: None,
                openspec_change: Some("web-dash-change".into()),
            },
        ];
        let backend = TestBackend::new(36, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        assert!(text.contains("rust-tui"), "should show tree node: {text}");
    }

    #[test]
    fn tree_items_sorted_by_status() {
        let t = super::super::theme::Alpharius;
        let nodes = vec![
            NodeSummary {
                id: "exploring-node".into(),
                title: "E".into(),
                status: NodeStatus::Exploring,
                open_questions: 0,
                parent: None,
                priority: None,
                issue_type: None,
                openspec_change: None,
            },
            NodeSummary {
                id: "implementing-node".into(),
                title: "I".into(),
                status: NodeStatus::Implementing,
                open_questions: 0,
                parent: None,
                priority: None,
                issue_type: None,
                openspec_change: None,
            },
            NodeSummary {
                id: "decided-node".into(),
                title: "D".into(),
                status: NodeStatus::Decided,
                open_questions: 0,
                parent: None,
                priority: None,
                issue_type: None,
                openspec_change: None,
            },
        ];
        let empty_changes = std::collections::HashMap::new();
        let items = build_tree_items(&nodes, None, &empty_changes, &t);
        assert_eq!(items.len(), 3);
        // Implementing should come first
        assert_eq!(items[0].identifier(), &"implementing-node".to_string());
        assert_eq!(items[1].identifier(), &"decided-node".to_string());
        assert_eq!(items[2].identifier(), &"exploring-node".to_string());
    }

    #[test]
    fn tree_items_with_children() {
        let t = super::super::theme::Alpharius;
        let nodes = vec![
            NodeSummary {
                id: "parent".into(),
                title: "Parent".into(),
                status: NodeStatus::Exploring,
                open_questions: 1,
                parent: None,
                priority: None,
                issue_type: None,
                openspec_change: None,
            },
            NodeSummary {
                id: "child-a".into(),
                title: "Child A".into(),
                status: NodeStatus::Decided,
                open_questions: 0,
                parent: Some("parent".into()),
                priority: Some(2),
                issue_type: None,
                openspec_change: None,
            },
            NodeSummary {
                id: "child-b".into(),
                title: "Child B".into(),
                status: NodeStatus::Implementing,
                open_questions: 3,
                parent: Some("parent".into()),
                priority: None,
                issue_type: None,
                openspec_change: Some("change-b".into()),
            },
        ];
        let empty_changes = std::collections::HashMap::new();
        let items = build_tree_items(&nodes, None, &empty_changes, &t);
        assert_eq!(items.len(), 1, "should have one root");
        assert_eq!(items[0].children().len(), 2, "root should have 2 children");
        // implementing child comes first
        assert_eq!(items[0].children()[0].identifier(), &"child-b".to_string());
    }

    #[test]
    fn node_text_focused_styling() {
        let t = super::super::theme::Alpharius;
        let node = NodeSummary {
            id: "my-node".into(),
            title: "Test".into(),
            status: NodeStatus::Decided,
            open_questions: 2,
            parent: None,
            priority: Some(1),
            issue_type: None,
            openspec_change: Some("change".into()),
        };

        // Not focused
        let empty_changes = std::collections::HashMap::new();
        let text_normal = node_text(&node, None, &empty_changes, &t);
        let line = &text_normal.lines[0];
        assert!(
            line.spans.len() >= 4,
            "should have icon, id, questions, priority spans"
        );

        // Focused
        let text_focused = node_text(&node, Some("my-node"), &empty_changes, &t);
        let line = &text_focused.lines[0];
        // The id span should be bold+accent_bright when focused
        let id_span = &line.spans[1];
        assert!(
            id_span.style.add_modifier.contains(Modifier::BOLD),
            "focused node id should be bold"
        );
    }

    #[test]
    fn priority_badge_colors() {
        let t = super::super::theme::Alpharius;
        let (label, color) = priority_badge(1, &t);
        assert_eq!(label, "P1");
        assert_eq!(color, t.error());
        let (label, _) = priority_badge(3, &t);
        assert_eq!(label, "P3");
        let (label, _) = priority_badge(5, &t);
        assert_eq!(label, "P5");
    }

    #[test]
    fn status_sort_order() {
        assert!(status_sort_key(NodeStatus::Implementing) < status_sort_key(NodeStatus::Decided));
        assert!(status_sort_key(NodeStatus::Decided) < status_sort_key(NodeStatus::Exploring));
        assert!(status_sort_key(NodeStatus::Blocked) < status_sort_key(NodeStatus::Decided));
        assert!(status_sort_key(NodeStatus::Deferred) > status_sort_key(NodeStatus::Seed));
    }

    #[test]
    fn sidebar_key_handling() {
        let mut state = DashboardState::default();
        state.all_nodes = vec![
            NodeSummary {
                id: "node-a".into(),
                title: "A".into(),
                status: NodeStatus::Exploring,
                open_questions: 0,
                parent: None,
                priority: None,
                issue_type: None,
                openspec_change: None,
            },
            NodeSummary {
                id: "node-b".into(),
                title: "B".into(),
                status: NodeStatus::Decided,
                open_questions: 0,
                parent: None,
                priority: None,
                issue_type: None,
                openspec_change: None,
            },
        ];

        // Not active — should not consume events
        let key_down = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!state.handle_key(key_down));

        // Activate
        state.sidebar_active = true;

        // Down should consume
        assert!(state.handle_key(key_down));

        // Esc should deactivate
        let key_esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(state.handle_key(key_esc));
        assert!(!state.sidebar_active);
    }

    #[test]
    fn selected_node_id_empty() {
        let state = DashboardState::default();
        assert!(state.selected_node_id().is_none());
    }

    #[test]
    fn format_k_values() {
        assert_eq!(format_k(200_000), "200k");
        assert_eq!(format_k(1_000_000), "1M");
    }

    #[test]
    fn dashboard_with_harness_status() {
        let mut state = DashboardState::default();
        state.harness = Some(crate::status::HarnessStatus {
            active_persona: Some(crate::status::PersonaSummary {
                id: "syseng".into(),
                name: "System Engineer".into(),
                badge: "⚙".into(),
                mind_facts_count: 42,
                activated_skills: vec!["rust".into(), "debugging".into()],
                disabled_tools: vec![],
            }),
            active_tone: Some(crate::status::ToneSummary {
                id: "concise".into(),
                name: "Concise".into(),
                intensity_mode: "full".into(),
            }),
            providers: vec![
                crate::status::ProviderStatus {
                    name: "Anthropic".into(),
                    authenticated: true,
                    auth_method: Some("oauth".into()),
                    model: Some("claude-sonnet-4-6".into()),
                },
                crate::status::ProviderStatus {
                    name: "OpenAI".into(),
                    authenticated: false,
                    auth_method: None,
                    model: None,
                },
            ],
            mcp_servers: vec![crate::status::McpServerStatus {
                name: "filesystem".into(),
                transport_mode: crate::status::McpTransportMode::LocalProcess,
                tool_count: 8,
                connected: true,
                error: None,
            }],
            secret_backend: Some(crate::status::SecretBackendStatus {
                backend: "keyring".into(),
                stored_count: 5,
                locked: false,
            }),
            inference_backends: vec![crate::status::InferenceBackendStatus {
                name: "Ollama".into(),
                kind: crate::status::InferenceKind::External,
                available: true,
                models: vec![crate::status::InferenceModelInfo {
                    name: "llama3.2:3b".into(),
                    params: Some("3B".into()),
                    context_window: Some(131072),
                }],
            }],
            container_runtime: Some(crate::status::ContainerRuntimeStatus {
                runtime: "podman".into(),
                version: Some("5.3.1".into()),
                available: true,
            }),
            ..Default::default()
        });

        let backend = TestBackend::new(50, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        // Harness info is no longer rendered in the sidebar (it's in the footer
        // engine panel). Just verify the dashboard renders without panic.
        let text = buf_text(&terminal);
        assert!(
            text.contains("Dashboard"),
            "should render dashboard title: {text}"
        );
    }

    #[test]
    fn dashboard_handles_refresh_with_harness() {
        let harness_status = Arc::new(Mutex::new(crate::status::HarnessStatus {
            active_persona: Some(crate::status::PersonaSummary {
                id: "test".into(),
                name: "Test Persona".into(),
                badge: "🧪".into(),
                mind_facts_count: 10,
                activated_skills: vec!["rust".into()],
                disabled_tools: vec![],
            }),
            ..Default::default()
        }));

        let handles = DashboardHandles {
            harness: Some(harness_status),
            ..Default::default()
        };
        let mut state = DashboardState::default();
        handles.refresh_into(&mut state);

        assert!(state.harness.is_some());
        assert_eq!(
            state.harness.unwrap().active_persona.unwrap().name,
            "Test Persona"
        );
    }

    #[test]
    fn cleave_not_rendered_in_sidebar() {
        // Cleave progress is shown in the instruments panel, not the sidebar.
        // The sidebar should never contain "cleave" regardless of state.
        let mut state = DashboardState::default();
        state.cleave = Some(CleaveProgress {
            active: true,
            run_id: "test-run".into(),
            total_children: 2,
            completed: 1,
            failed: 0,
            children: vec![ChildProgress {
                label: "task-1".into(),
                status: "completed".into(),
                duration_secs: Some(5.0),
                last_tool: None,
                last_turn: None,
                started_at: None,
                tokens_in: 0,
                tokens_out: 0,
            }],
            total_tokens_in: 0,
            total_tokens_out: 0,
        });

        let backend = TestBackend::new(50, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        assert!(
            !text.contains("cleave"),
            "cleave should not appear in sidebar: {text}"
        );
    }

    #[test]
    fn orphan_nodes_become_roots() {
        let t = super::super::theme::Alpharius;
        let nodes = vec![NodeSummary {
            id: "orphan".into(),
            title: "Orphan".into(),
            status: NodeStatus::Exploring,
            open_questions: 0,
            parent: Some("implemented-parent".into()), // parent not in active set
            priority: None,
            issue_type: None,
            openspec_change: None,
        }];
        let empty_changes = std::collections::HashMap::new();
        let items = build_tree_items(&nodes, None, &empty_changes, &t);
        assert_eq!(items.len(), 1, "orphan should become root");
        assert_eq!(items[0].identifier(), &"orphan".to_string());
    }

    #[test]
    fn empty_tree_renders_hint() {
        let mut state = DashboardState::default();
        // No nodes at all
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        assert!(
            text.contains("no active") || text.contains("Dashboard"),
            "should show hint or title: {text}"
        );
    }

    #[test]
    fn degraded_nodes_render_in_tree() {
        let mut state = DashboardState::default();
        state.status_counts.total = 5;
        state.all_nodes = vec![NodeSummary {
            id: "good-node".into(),
            title: "Good".into(),
            status: NodeStatus::Exploring,
            open_questions: 0,
            parent: None,
            priority: None,
            issue_type: None,
            openspec_change: None,
        }];
        state.degraded_nodes = vec![DegradedNodeSummary {
            id: "broken-node".into(),
            title: "Was Good".into(),
            file_path: "docs/broken-node.md".into(),
            reason: "frontmatter parse failed".into(),
        }];

        let backend = TestBackend::new(50, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        assert!(
            text.contains("broken-node"),
            "should render degraded node in tree: {text}"
        );
        assert!(
            text.contains("good-node"),
            "should still render valid nodes: {text}"
        );
    }

    #[test]
    fn degraded_count_in_header() {
        let mut state = DashboardState::default();
        state.status_counts.total = 10;
        state.degraded_nodes = vec![
            DegradedNodeSummary {
                id: "a".into(),
                title: "A".into(),
                file_path: "a.md".into(),
                reason: "parse failed".into(),
            },
            DegradedNodeSummary {
                id: "b".into(),
                title: "B".into(),
                file_path: "b.md".into(),
                reason: "missing id".into(),
            },
        ];

        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text = buf_text(&terminal);
        // Should show ⚠2 in the header
        assert!(
            text.contains("⚠2") || text.contains("⚠ 2"),
            "should show degraded count badge: {text}"
        );
    }
}
